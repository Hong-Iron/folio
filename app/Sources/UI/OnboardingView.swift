import AVFoundation
import SwiftUI

enum OnboardingStep: Int, CaseIterable {
    case welcome, camera, face, calibrate, permissions, done
}

enum FaceMode {
    case replace, add
}

struct OnboardingView: View {
    @EnvironmentObject var model: AppModel
    @State var step: OnboardingStep
    @StateObject private var lab = CalibrationLab()
    @StateObject private var enroll = FaceEnrollment()
    /// Opened from Settings for one step: finish closes the window.
    let single: Bool
    let faceMode: FaceMode

    init(start: OnboardingStep, single: Bool = false, faceMode: FaceMode = .replace) {
        _step = State(initialValue: start)
        self.single = single
        self.faceMode = faceMode
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if step != .welcome && step != .done && !single {
                Text("Step \(step.rawValue) of 4").font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
            }
            Group {
                switch step {
                case .welcome: welcome
                case .camera: camera
                case .face: face
                case .calibrate: calibrate
                case .permissions: permissions
                case .done: done
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .padding(.horizontal, 32)
        .padding(.top, 40)
        .padding(.bottom, 28)
        .background(Palette.paper)
        .onAppear { lab.attach(model) }
        .onDisappear {
            lab.detach()
            enroll.stop()
        }
    }

    // MARK: Steps

    private var welcome: some View {
        VStack(alignment: .leading, spacing: 0) {
            Image(nsImage: NSApp.applicationIconImage).resizable().frame(width: 72, height: 72)
            Text("Folio counts the time you really work.")
                .font(Fonts.serif(28)).foregroundStyle(Palette.ink)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 18)
            Text("It watches where you're looking, notices a phone in your hands or a social site in front of you, and keeps an honest tally. Reading a textbook counts. Staring at the wall doesn't.")
                .font(Fonts.ui(14)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 10)
            VStack(alignment: .leading, spacing: 8) {
                promise("camera", "Frames are looked at in memory and thrown away. No images are saved.")
                promise("lock", "Everything stays on this Mac. No account, no cloud.")
                promise("questionmark.circle", "When it can't tell, it says so and asks you, instead of guessing.")
            }
            .padding(.top, 22)
            Spacer()
            HStack {
                Spacer()
                Button("Get started") { step = .camera }.buttonStyle(InkButtonStyle())
            }
        }
    }

    private func promise(_ icon: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).font(.system(size: 13)).foregroundStyle(Palette.ink3).frame(width: 18)
            Text(text).font(Fonts.ui(13)).foregroundStyle(Palette.ink).fixedSize(horizontal: false, vertical: true)
        }
    }

    private var camera: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Let's check your camera").font(Fonts.serif(24)).foregroundStyle(Palette.ink).padding(.top, 4)
            Text("Tilt your screen back a little so the edge of your desk shows at the bottom. That's where books and phones show up.")
                .font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 6)
            ZStack(alignment: .bottomLeading) {
                if CameraSource.authorized {
                    CameraPreview(session: model.camera.session)
                    Rectangle().stroke(style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                        .foregroundStyle(Color.white.opacity(0.7))
                        .frame(height: 0.5)
                        .frame(maxHeight: .infinity, alignment: .bottom)
                        .padding(.bottom, 60)
                    Text(lab.faceSeen ? "We can see you" : "Looking for your face…")
                        .font(Fonts.ui(11)).foregroundStyle(.white)
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(Capsule().fill(Color.black.opacity(0.45)))
                        .padding(10)
                } else {
                    VStack(spacing: 10) {
                        Text(CameraSource.denied
                             ? "Camera access is off. Turn it on for Folio in System Settings → Privacy & Security → Camera."
                             : "Folio needs your camera to see where you're looking.")
                            .font(Fonts.ui(13)).foregroundStyle(Palette.ink2).multilineTextAlignment(.center)
                        if !CameraSource.denied {
                            Button("Allow camera") {
                                CameraSource.requestAccess { ok in if ok { lab.startPreview() } }
                            }
                            .buttonStyle(InkButtonStyle())
                        } else {
                            Button("Open System Settings") {
                                NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera")!)
                            }
                            .buttonStyle(QuietButtonStyle(small: true))
                        }
                    }
                    .padding(24)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Palette.sunk)
                }
            }
            .aspectRatio(4 / 3, contentMode: .fit)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .padding(.top, 16)
            Spacer()
            nav(back: .welcome, next: .face, enabled: CameraSource.authorized)
        }
        .onAppear { lab.startPreview() }
    }

    private var calibrate: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Show us how you study").font(Fonts.serif(24)).foregroundStyle(Palette.ink).padding(.top, 4)
            Text("These two poses tell \"reading your notes\" apart from \"looking away\". Sit the way you normally do.")
                .font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 6)

            VStack(spacing: 0) {
                calibrationRow(1, "Look at the middle of your screen", phase: .screen)
                calibrationRow(2, "Now look down at your book or notes", phase: .desk)
            }
            .padding(.top, 18)

            if let message = lab.message {
                Text(message).font(Fonts.ui(12)).foregroundStyle(lab.failed ? Palette.clay : Palette.ink2)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 12)
            }

            if CameraSource.authorized {
                ZStack(alignment: .bottomLeading) {
                    CameraPreview(session: model.camera.session)
                    Text(lab.faceSeen ? "We can see you" : "Looking for your face…")
                        .font(Fonts.ui(11)).foregroundStyle(.white)
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(Capsule().fill(Color.black.opacity(0.45)))
                        .padding(8)
                }
                .frame(width: 240, height: 180)
                .background(Palette.sunk)
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .padding(.top, 16)
            }

            HStack(spacing: 8) {
                if lab.phase == .idle || lab.phase == .finished {
                    Button(lab.phase == .finished ? "Do it again" : "Start") { lab.run() }
                        .buttonStyle(lab.phase == .finished ? AnyButtonStyle(QuietButtonStyle()) : AnyButtonStyle(InkButtonStyle()))
                }
            }
            .padding(.top, 16)
            Spacer()
            nav(back: .face, next: .permissions, enabled: lab.phase == .finished && !lab.failed || model.calibrated)
        }
        .onAppear { lab.startPreview() }
    }

    private func calibrationRow(_ n: Int, _ text: String, phase: CalibrationLab.Phase) -> some View {
        let done = lab.completed.contains(phase)
        let active = lab.phase == phase
        return HStack(alignment: .top, spacing: 10) {
            if done {
                Image(systemName: "checkmark").font(.system(size: 13, weight: .medium)).foregroundStyle(Palette.moss).frame(width: 18)
            } else {
                Text("\(n)").font(Fonts.serif(15)).foregroundStyle(active ? Palette.ink : Palette.ink3).frame(width: 18)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(text).font(Fonts.ui(13)).foregroundStyle(active || done ? Palette.ink : Palette.ink2)
                if active {
                    Text(lab.countdown > 0 ? "Hold still… \(lab.countdown)" : "Get ready").font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                }
            }
            Spacer()
        }
        .padding(.vertical, 10)
        .overlay(alignment: .top) { Hairline() }
    }

    private var permissions: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("A few permissions").font(Fonts.serif(24)).foregroundStyle(Palette.ink).padding(.top, 4)
            Text("So the app can tell which app and site you're in. You can change these any time.")
                .font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 6)
            VStack(spacing: 0) {
                permissionRow("camera", "Camera", "Sees where you're looking.", granted: CameraSource.authorized, action: nil)
                permissionRow("macwindow", "Accessibility", "Reads which app and tab you're in.", granted: lab.axTrusted) {
                    ActivityWatcher.promptAccessibility()
                }
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: "globe").font(.system(size: 13)).foregroundStyle(Palette.ink3).frame(width: 18)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Browser address").font(Fonts.ui(13)).foregroundStyle(Palette.ink)
                        Text("Optional. Tells instagram.com from your course site. Your browser asks once.")
                            .font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer()
                    Toggle("", isOn: Binding(get: { model.activity.readBrowserAddresses }, set: { model.activity.readBrowserAddresses = $0; lab.objectWillChange.send() }))
                        .toggleStyle(.switch).controlSize(.small).labelsHidden().tint(Palette.moss)
                }
                .padding(.vertical, 10)
                .overlay(alignment: .top) { Hairline() }
                Hairline()
            }
            .padding(.top, 18)
            Text("Everything stays on this Mac. No account, no cloud.")
                .font(Fonts.ui(12)).foregroundStyle(Palette.ink2).padding(.top, 14)
            Spacer()
            nav(back: .calibrate, next: .done, enabled: true)
        }
    }

    private func permissionRow(_ icon: String, _ title: String, _ note: String, granted: Bool, action: (() -> Void)?) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).font(.system(size: 13)).foregroundStyle(Palette.ink3).frame(width: 18)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(Fonts.ui(13)).foregroundStyle(Palette.ink)
                Text(note).font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
            }
            Spacer()
            if granted {
                Label("On", systemImage: "checkmark").font(Fonts.ui(12)).foregroundStyle(Palette.moss)
            } else if let action {
                Button("Open settings", action: action).buttonStyle(QuietButtonStyle(small: true))
            }
        }
        .padding(.vertical, 10)
        .overlay(alignment: .top) { Hairline() }
    }

    private var done: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("You're set.").font(Fonts.serif(30)).foregroundStyle(Palette.ink).padding(.top, 8)
            Text("Start a session from the book in your menu bar, or press ⌥⌘F anywhere. When something's unclear, you'll be asked at the end instead of it being guessed.")
                .font(Fonts.ui(14)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 8)
            Spacer()
            HStack {
                Button("Close") { finish(start: false) }.buttonStyle(QuietButtonStyle())
                Spacer()
                Button("Start a session now") { finish(start: true) }.buttonStyle(InkButtonStyle())
            }
        }
    }

    private func finish(start: Bool) {
        UserDefaults.standard.set(true, forKey: "onboarded")
        lab.detach()
        WindowRouter.shared.close("onboarding")
        if start { model.startSession() }
    }

    private func nav(back: OnboardingStep, next: OnboardingStep, enabled: Bool, skippable: Bool = false) -> some View {
        HStack {
            if !single {
                Button("Back") { step = back }.buttonStyle(LinkButtonStyle())
            }
            Spacer()
            if skippable && !enabled && !single {
                Button("Skip for now") { step = next }.buttonStyle(LinkButtonStyle()).padding(.trailing, 12)
            }
            Button(single ? "Done" : "Continue") {
                if single {
                    lab.detach()
                    WindowRouter.shared.close("onboarding")
                } else {
                    step = next
                }
            }
            .buttonStyle(InkButtonStyle())
            .opacity(enabled ? 1 : 0.4)
            .allowsHitTesting(enabled)
        }
    }

    // MARK: Face

    private var face: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(faceMode == .add ? "Add another look" : "Let's learn your face")
                .font(Fonts.serif(24)).foregroundStyle(Palette.ink).padding(.top, 4)
            Text(faceMode == .add
                 ? "Glasses on, hair up, a different lamp: whatever you look like now gets added to what Folio already knows."
                 : "So time only counts when it's you at the desk. Folio keeps 128 numbers that describe your face, never a photo.")
                .font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 6)

            if model.faceEmbedder == nil {
                Text(model.faceModelLoading ? "Loading the face model…" : "Face recognition isn't available in this build, so this step is skipped.")
                    .font(Fonts.ui(13)).foregroundStyle(Palette.ink3).padding(.top, 18)
            } else {
                HStack(alignment: .top, spacing: 18) {
                    ZStack {
                        if CameraSource.authorized {
                            CameraPreview(session: model.camera.session)
                        }
                        Ellipse().stroke(style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
                            .foregroundStyle(Color.white.opacity(0.75))
                            .frame(width: 96, height: 128)
                    }
                    .frame(width: 220, height: 165)
                    .background(Palette.sunk)
                    .clipShape(RoundedRectangle(cornerRadius: 10))

                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 3) {
                            ForEach(0..<enroll.target, id: \.self) { i in
                                RoundedRectangle(cornerRadius: 1)
                                    .fill(i < enroll.collected ? Palette.ink : Palette.rule)
                                    .frame(height: 4)
                            }
                        }
                        Text(enroll.statusText).font(Fonts.ui(12)).foregroundStyle(enroll.failed ? Palette.clay : Palette.ink2)
                            .fixedSize(horizontal: false, vertical: true)
                        if enroll.state == .idle || enroll.state == .done || enroll.failed {
                            Button(enroll.state == .idle ? "Start" : "Do it again") {
                                lab.startPreview()
                                enroll.start(model: model, mode: faceMode)
                            }
                            .buttonStyle(enroll.state == .idle ? AnyButtonStyle(InkButtonStyle()) : AnyButtonStyle(QuietButtonStyle(small: true)))
                            .padding(.top, 4)
                        }
                    }
                }
                .padding(.top, 18)
            }
            Spacer()
            nav(back: .camera, next: .calibrate,
                enabled: enroll.state == .done || model.faceEmbedder == nil && !model.faceModelLoading || (model.faceEnrolled && !single),
                skippable: true)
        }
        .onAppear { lab.startPreview() }
    }
}

struct AnyButtonStyle: ButtonStyle {
    private let make: (Configuration) -> AnyView
    init<S: ButtonStyle>(_ style: S) {
        make = { AnyView(style.makeBody(configuration: $0)) }
    }
    func makeBody(configuration: Configuration) -> some View { make(configuration) }
}

// MARK: - Calibration

/// Samples head pose while you look at the screen, then at your desk, and
/// stores the two poses (plus eye openness for each) in the engine.
@MainActor
final class CalibrationLab: ObservableObject {
    enum Phase: Hashable { case idle, screen, desk, finished }

    @Published var phase: Phase = .idle
    @Published var completed: Set<Phase> = []
    @Published var countdown = 0
    @Published var message: String?
    @Published var failed = false
    @Published var faceSeen = false
    @Published var axTrusted = ActivityWatcher.accessibilityTrusted

    private weak var model: AppModel?
    private let vision = VisionPipeline()
    private let queue = DispatchQueue(label: "nd.calibration", qos: .userInitiated)
    private var timer: Timer?
    private var samples: [Phase: [VisionPipeline.FaceOnly]] = [:]
    private var collecting: Phase?
    private var startedCamera = false

    func attach(_ model: AppModel) {
        self.model = model
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.poll() }
        }
    }

    func detach() {
        timer?.invalidate()
        timer = nil
        if startedCamera, let model, !model.sessionActive {
            model.camera.stop()
        }
        startedCamera = false
    }

    func startPreview() {
        guard let model, CameraSource.authorized, !model.sessionActive else { return }
        model.camera.start()
        startedCamera = true
    }

    private var tick = 0
    private func poll() {
        tick += 1
        if tick % 5 == 0 { axTrusted = ActivityWatcher.accessibilityTrusted }
        guard let model, let px = model.camera.latestFrame() else { return }
        let phase = collecting
        if phase == nil && tick % 3 != 0 { return }
        let box = PixelBox(px)
        queue.async { [vision] in
            guard let buffer = box.buffer else { return }
            let f = vision.faceOnly(buffer)
            Task { @MainActor in
                self.faceSeen = (f?.faces ?? 0) > 0
                if let phase, let f { self.samples[phase, default: []].append(f) }
            }
        }
    }

    func run() {
        startPreview()
        samples = [:]
        completed = []
        message = nil
        failed = false
        Task { @MainActor in
            await collect(.screen)
            await collect(.desk)
            finish()
        }
    }

    private func collect(_ p: Phase) async {
        phase = p
        countdown = 0
        try? await Task.sleep(for: .seconds(1.5))
        collecting = p
        for s in stride(from: 3, through: 1, by: -1) {
            countdown = s
            try? await Task.sleep(for: .seconds(1))
        }
        collecting = nil
        completed.insert(p)
    }

    private func median(_ xs: [Float]) -> Float? {
        guard !xs.isEmpty else { return nil }
        let s = xs.sorted()
        return s[s.count / 2]
    }

    private func finish() {
        guard let model else { return }
        let screen = (samples[.screen] ?? []).filter { $0.faces > 0 }
        let deskAll = samples[.desk] ?? []
        let desk = deskAll.filter { $0.faces > 0 }
        guard screen.count >= 5, let sy = median(screen.map(\.yaw)), let sp = median(screen.map(\.pitch)) else {
            phase = .finished
            failed = true
            message = "We couldn't see your face clearly while you looked at the screen. Check the light and the framing, then try again."
            return
        }
        let screenEye = median(screen.compactMap(\.eyeOpen)) ?? 0.3
        let deskVisible = !deskAll.isEmpty && Double(desk.count) / Double(deskAll.count) > 0.6
        var deskPitch: Float
        var deskEye: Float
        if desk.count >= 5, let dp = median(desk.map(\.pitch)) {
            deskPitch = dp
            deskEye = median(desk.compactMap(\.eyeOpen)) ?? screenEye * 0.6
        } else {
            // The face dropped out of view while looking down. Use whatever
            // direction we saw it start moving in.
            let lastSeen = desk.last?.pitch ?? sp
            let sign: Float = abs(lastSeen - sp) > 0.05 ? (lastSeen > sp ? 1 : -1) : 1
            deskPitch = sp + sign * 0.45
            deskEye = screenEye * 0.6
        }
        if abs(deskPitch - sp) < 0.08 {
            message = "Looking down didn't change your head angle much. That's fine, but if you can, tilt your head a little more toward your notes and run it again."
        } else {
            message = deskVisible
                ? "Got it. Both poses are saved."
                : "Got it. Your face drops out of view when you look down, so the app will also go by your posture there."
        }
        let cal = Calibration(
            screenYaw: sy, screenPitch: sp, deskPitch: deskPitch,
            screenEyeOpen: screenEye, deskEyeOpen: deskEye,
            deskFaceVisible: deskVisible, calibrated: true
        )
        do {
            try model.engine.setCalibration(cal: cal)
            model.markCalibrated()
            failed = false
        } catch {
            failed = true
            message = "Couldn't save the calibration: \(error.localizedDescription)"
        }
        phase = .finished
    }
}

struct CameraPreview: NSViewRepresentable {
    let session: AVCaptureSession

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        let layer = AVCaptureVideoPreviewLayer(session: session)
        layer.videoGravity = .resizeAspectFill
        view.layer = layer
        view.wantsLayer = true
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

// MARK: - Face enrollment

/// Collects 20 good frames of your face from slightly different angles,
/// then checks they agree with each other before saving the template.
@MainActor
final class FaceEnrollment: ObservableObject {
    enum State: Equatable { case idle, collecting, done }

    let target = 20
    @Published private(set) var state: State = .idle
    @Published private(set) var collected = 0
    @Published private(set) var statusText = "Look at the screen, then turn your head slowly, a little left and right."
    @Published private(set) var failed = false

    private weak var model: AppModel?
    private var mode: FaceMode = .replace
    private let vision = VisionPipeline()
    private let queue = DispatchQueue(label: "nd.enroll", qos: .userInitiated)
    private var timer: Timer?
    private var busy = false
    private var samples: [[Float]] = []
    private var lastYaw: Float?
    private var lastAccepted = Date.distantPast

    func start(model: AppModel, mode: FaceMode) {
        self.model = model
        self.mode = mode
        samples = []
        collected = 0
        failed = false
        lastYaw = nil
        state = .collecting
        statusText = "Turn your head slowly, a little left and right."
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.poll() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    private func poll() {
        guard state == .collecting, !busy, let model, let embedder = model.faceEmbedder,
              let px = model.camera.latestFrame() else { return }
        busy = true
        let box = PixelBox(px)
        queue.async { [vision] in
            guard let pixel = box.buffer else { return }
            var result: (emb: [Float], yaw: Float)?
            var hint: String?
            if let f = vision.enrollmentFace(pixel) {
                if f.quality < 0.3 {
                    hint = "A little more light on your face would help."
                } else if f.size < 0.12 {
                    hint = "Come a bit closer to the camera."
                } else if let emb = embedder.embedding(pixel, face: f.face) {
                    result = (emb, f.yaw)
                }
            } else {
                hint = "Looking for your face…"
            }
            Task { @MainActor in
                self.busy = false
                self.accept(result, hint: hint)
            }
        }
    }

    private func accept(_ r: (emb: [Float], yaw: Float)?, hint: String?) {
        guard state == .collecting else { return }
        if let hint { statusText = hint }
        guard let r else { return }
        // Prefer variety: take a new angle right away, the same angle at most twice a second.
        let varied = lastYaw.map { abs($0 - r.yaw) > 0.02 } ?? true
        guard varied || Date().timeIntervalSince(lastAccepted) > 0.5 else { return }
        samples.append(r.emb)
        lastYaw = r.yaw
        lastAccepted = Date()
        collected = samples.count
        statusText = "\(collected) of \(target). Keep turning slowly."
        if collected >= target { finish() }
    }

    private func finish() {
        stop()
        guard let model else { return }
        do {
            if mode == .add {
                try model.engine.addFaceSamples(samples: samples)
            } else {
                try model.engine.setFaceSamples(samples: samples)
            }
        } catch {
            failed = true
            state = .done
            statusText = "Couldn't save your face: \(error.localizedDescription)"
            return
        }
        // Every sample should match the template it built; if some don't,
        // the light or angle was off.
        let scores = samples.compactMap { model.engine.matchFace(embedding: $0)?.score }
        let weak = scores.filter { $0 < model.engine.config().faceMatchThreshold + 0.1 }.count
        model.refreshFaceState()
        state = .done
        failed = false
        statusText = weak == 0
            ? "Saved. All \(scores.count) samples agree with each other."
            : "Saved, but \(weak) of \(scores.count) samples matched weakly. If the light was dim, do it again in better light."
    }
}
