import AppKit
import Combine
import IOKit.pwr_mgt
import SwiftUI
import Vision

/// Computes a face embedding for identity checks. Supplied once a face
/// recognition model is installed; without one, identity checks are skipped.
protocol FaceEmbedder: AnyObject, Sendable {
    func embedding(_ pixel: CVPixelBuffer, face: VNFaceObservation) -> [Float]?
}

/// Ties sensing, the Rust engine and the UI together. Ticks once a second
/// during a session (or slower when the engine says so) and does nothing at
/// all between sessions: no camera, no timers.
@MainActor
final class AppModel: ObservableObject {
    static let shared = AppModel()

    let engine: Engine
    let camera = CameraSource()
    let vision = VisionPipeline()
    let activity = ActivityWatcher()
    let recorder = LabelRecorder()
    var faceEmbedder: FaceEmbedder?
    lazy var nudges = NudgeController(model: self)

    @Published private(set) var update: EngineUpdate?
    @Published private(set) var sessionActive = false
    @Published private(set) var liveSegments: [SegmentView] = []
    @Published private(set) var todayFocusMs: Int64 = 0
    @Published private(set) var calibrated: Bool
    @Published private(set) var labelModeLabel: String?
    @Published private(set) var faceEnrolled = false
    @Published private(set) var faceModelLoading = true
    @Published var draftLabel = ""
    @Published var goalMin: UInt32? = 90
    @Published var interrupted: InterruptedSession?
    @Published var problem: String?

    private var timer: Timer?
    private var plan = SensingPlan(tickMs: 1000, landmarks: true, body: false, hands: false, objects: true, verify: false)
    private let sensingQueue = DispatchQueue(label: "nd.sensing", qos: .utility)
    private var busy = false
    private var systemObservers: [NSObjectProtocol] = []
    private var sleepAssertion: IOPMAssertionID = 0
    private var sessionStartWall: Int64 = 0

    init() {
        // ND_DB_PATH points the app at a scratch database (snapshots, testing).
        let dbURL = ProcessInfo.processInfo.environment["ND_DB_PATH"].map { URL(fileURLWithPath: $0) }
            ?? AppPaths.support.appendingPathComponent("nd.sqlite")
        var problem: String?
        var opened: Engine
        do {
            opened = try Engine(dbPath: dbURL.path)
        } catch {
            // Keep the unreadable file for recovery and start a fresh one.
            let aside = dbURL.deletingLastPathComponent()
                .appendingPathComponent("nd-unreadable-\(Int(Date().timeIntervalSince1970)).sqlite")
            try? FileManager.default.moveItem(at: dbURL, to: aside)
            opened = (try? Engine(dbPath: dbURL.path)) ?? (try! Engine(dbPath: nil))
            problem = "Your history couldn't be opened, so a fresh one was started. The old file is kept next to it."
        }
        engine = opened
        calibrated = opened.calibration().calibrated
        if problem == nil && !opened.integrityOk() {
            problem = "Your history file reported a problem. Export it from Settings to be safe."
        }
        self.problem = problem
        interrupted = (try? opened.interruptedSession()) ?? nil
        faceEnrolled = opened.hasFace()
        loadFaceModel()
        refreshToday()
        observeSystem()
        LabelRecorder.pruneOldThumbnails()
    }

    // MARK: Session

    func toggleSession() {
        sessionActive ? endSession() : startSession()
    }

    func startSession() {
        guard !sessionActive else { return }
        guard CameraSource.authorized, calibrated else {
            WindowRouter.shared.showOnboarding()
            return
        }
        let label = draftLabel.trimmingCharacters(in: .whitespacesAndNewlines)
        let wall = Clock.wallMs()
        do {
            _ = try engine.startSession(
                label: label.isEmpty ? nil : label, goalMin: goalMin,
                tMs: Clock.nowMs(), wallMs: wall, tzOffsetMin: Clock.tzOffsetMin()
            )
        } catch {
            problem = "Couldn't start the session: \(error.localizedDescription)"
            return
        }
        sessionStartWall = wall
        sessionActive = true
        interrupted = nil
        camera.start()
        activity.start()
        recorder.sessionStarted(calibration: engine.calibration())
        labelModeLabel = nil
        holdSystemAwake(true)
        plan = SensingPlan(tickMs: 1000, landmarks: true, body: false, hands: false, objects: true, verify: engine.hasFace())
        schedule()
    }

    func endSession(showSummary: Bool = true) {
        guard sessionActive else { return }
        timer?.invalidate()
        timer = nil
        let t = Clock.nowMs()
        camera.stop()
        activity.stop()
        nudges.dismissAll()
        recorder.sessionEnded(at: t)
        holdSystemAwake(false)
        let summary = (try? engine.endSession(tMs: t)) ?? nil
        sessionActive = false
        update = nil
        liveSegments = []
        labelModeLabel = nil
        refreshToday()
        if showSummary, let summary {
            WindowRouter.shared.showSummary(summary)
        }
    }

    func takeBreak(minutes: UInt32) {
        try? engine.startBreak(minutes: minutes, tMs: Clock.nowMs())
        nudges.dismissAll()
        tickSoon()
    }

    func endBreak() {
        try? engine.endBreak(tMs: Clock.nowMs())
        tickSoon()
    }

    func resumeInterrupted() {
        guard let i = interrupted else { return }
        _ = try? engine.closeInterrupted()
        draftLabel = i.label ?? ""
        goalMin = i.goalMin
        interrupted = nil
        startSession()
    }

    func endInterrupted() {
        let summary = (try? engine.closeInterrupted()) ?? nil
        interrupted = nil
        refreshToday()
        if let summary { WindowRouter.shared.showSummary(summary) }
    }

    func refreshToday() {
        todayFocusMs = ((try? engine.report(fromDate: Clock.dateString(), days: 1))?.totals.focusMs) ?? 0
    }

    func markCalibrated() {
        calibrated = engine.calibration().calibrated
    }

    func refreshFaceState() {
        faceEnrolled = engine.hasFace()
    }

    func forgetFace() {
        try? engine.clearFace()
        refreshFaceState()
    }

    /// Loading and optimizing the model takes a moment; do it off the main thread.
    private func loadFaceModel() {
        DispatchQueue.global(qos: .utility).async {
            let embedder = SFaceEmbedder()
            Task { @MainActor in
                self.faceEmbedder = embedder
                self.faceModelLoading = false
            }
        }
    }

    /// Shows a session state without running one (snapshot mode only).
    func debugPreview(update: EngineUpdate?, segments: [SegmentView], label: String) {
        self.update = update
        liveSegments = segments
        sessionActive = update != nil
        draftLabel = label
    }

    // MARK: Ticking

    private func schedule() {
        timer?.invalidate()
        var interval = Double(plan.tickMs) / 1000
        if ProcessInfo.processInfo.isLowPowerModeEnabled { interval *= 2 }
        let t = Timer(timeInterval: interval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        t.tolerance = interval * 0.2
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    private func tickSoon() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    private func tick() {
        guard sessionActive else { return }
        camera.watchdog()
        guard !busy else { return }
        busy = true

        let snap = activity.snapshot()
        let box = PixelBox(camera.latestFrame())
        let plan = self.plan
        var frame = SignalFrame.empty(t: Clock.nowMs())
        frame.inputIdleS = snap.idleSeconds
        frame.appBundle = snap.bundle
        frame.appName = snap.name
        frame.windowTitle = snap.title
        frame.urlHost = snap.host
        frame.screenLocked = snap.locked

        let engine = self.engine
        let vision = self.vision
        let recorder = self.recorder
        let embedder = self.faceEmbedder
        let keepThumbs = recorder.keepThumbnails
        sensingQueue.async { [weak self] in
            var frame = frame
            let pixel = box.buffer
            if let pixel {
                let face = vision.analyze(pixel, plan: plan, into: &frame)
                frame.cameraOk = true
                if plan.verify, let face, let embedder, let emb = embedder.embedding(pixel, face: face) {
                    frame.isUser = engine.matchFace(embedding: emb)?.isUser
                }
            }
            let result = Result { try engine.ingest(frame: frame) }
            recorder.record(frame, pixel: keepThumbs ? pixel : nil)
            let segments = engine.liveSegments()
            Task { @MainActor in
                self?.busy = false
                self?.apply(result, segments: segments)
            }
        }
    }

    private func apply(_ result: Result<EngineUpdate, Error>, segments: [SegmentView]) {
        guard sessionActive else { return }
        switch result {
        case .failure(let error):
            problem = "Something went wrong while sensing: \(error.localizedDescription)"
        case .success(let u):
            update = u
            liveSegments = segments
            let reschedule = u.plan.tickMs != plan.tickMs
            plan = u.plan
            if reschedule { schedule() }
            if let n = u.nudge { nudges.show(n) }
            if let a = u.activity, a.isFocus || a == .break || a == .away { nudges.dismissDrift() }
            if let host = u.askCategory { nudges.ask(host: host) }
            if let e = engine.lastStorageError() { problem = "Couldn't save the last few seconds: \(e)" }
        }
    }

    // MARK: Rules and corrections

    func setRule(_ pattern: String, kind: RuleKind, category: Category) {
        try? engine.setRule(rule: Rule(pattern: pattern, kind: kind, category: category))
        objectWillChange.send()
    }

    func removeRule(_ rule: Rule) {
        try? engine.removeRule(pattern: rule.pattern, kind: rule.kind)
        objectWillChange.send()
    }

    /// Records the answer to "were you working?" and returns the refreshed summary.
    func resolveUnclear(_ span: UnclearSpan, worked: Bool, in summary: SessionSummary) -> SessionSummary {
        try? engine.resolveUnclear(segmentId: span.segmentId, worked: worked, wallMs: Clock.wallMs())
        refreshToday()
        return ((try? engine.sessionSummary(sessionId: summary.sessionId)) ?? nil) ?? summary
    }

    // MARK: Label mode

    func installHotKeys() {
        HotKeys.shared.register(id: 100, keyCode: kVK_ANSI_F_value, modifiers: optionKeyMask | cmdKeyMask) { [weak self] in
            MainActor.assumeIsolated { self?.toggleSession() }
        }
        updateLabelHotKeys()
    }

    func updateLabelHotKeys() {
        for (i, entry) in LabelRecorder.labels.enumerated() {
            let id = UInt32(i + 1)
            if recorder.enabled {
                HotKeys.shared.register(id: id, keyCode: HotKeys.digitKeys[i], modifiers: controlKeyMask | optionKeyMask) { [weak self] in
                    MainActor.assumeIsolated { self?.mark(entry.label) }
                }
            } else {
                HotKeys.shared.unregister(id: id)
            }
        }
    }

    private func mark(_ label: String) {
        guard sessionActive, recorder.recording else { return }
        recorder.mark(label, at: Clock.nowMs())
        labelModeLabel = recorder.currentLabel
        NSSound(named: "Tink")?.play()
    }

    // MARK: System

    /// Keeps the Mac from idle-sleeping mid-session (you may be reading a
    /// book without touching it). The display can still turn off; closing
    /// the lid still sleeps and pauses the session.
    private func holdSystemAwake(_ on: Bool) {
        if on, sleepAssertion == 0 {
            IOPMAssertionCreateWithName(
                kIOPMAssertionTypePreventUserIdleSystemSleep as CFString,
                IOPMAssertionLevel(kIOPMAssertionLevelOn),
                "Study session in progress" as CFString,
                &sleepAssertion
            )
        } else if !on, sleepAssertion != 0 {
            IOPMAssertionRelease(sleepAssertion)
            sleepAssertion = 0
        }
    }

    private func observeSystem() {
        let ws = NSWorkspace.shared.notificationCenter
        for name in [NSWorkspace.willSleepNotification, NSWorkspace.sessionDidResignActiveNotification] {
            systemObservers.append(ws.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.systemPaused() }
            })
        }
        for name in [NSWorkspace.didWakeNotification, NSWorkspace.sessionDidBecomeActiveNotification] {
            systemObservers.append(ws.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.systemResumed() }
            })
        }
    }

    private func systemPaused() {
        guard sessionActive else { return }
        try? engine.systemPause(tMs: Clock.nowMs())
        camera.stop()
        nudges.dismissAll()
    }

    private func systemResumed() {
        guard sessionActive else { return }
        camera.start()
    }
}

// Carbon constants, spelled out to keep Carbon out of the model's imports.
let kVK_ANSI_F_value = 0x03
let cmdKeyMask = 1 << 8
let optionKeyMask = 1 << 11
let controlKeyMask = 1 << 12
