import AppKit
import SwiftUI

/// `Folio --snapshots <dir>` renders every screen, filled with
/// synthetic history from a scratch database, to PNGs in light and dark.
/// Used to check the UI without a camera or any permissions.
@MainActor
enum Snapshots {
    static func run(to dir: URL) {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let model = AppModel.shared
        let engine = model.engine
        let last = (try? engine.seedDemo(days: 28, nowWallMs: Clock.wallMs(), tzOffsetMin: Clock.tzOffsetMin())) ?? nil
        try? engine.setCalibration(cal: Calibration(
            screenYaw: 0, screenPitch: 0, deskPitch: 0.4, screenEyeOpen: 0.3, deskEyeOpen: 0.2,
            deskFaceVisible: true, calibrated: true
        ))
        model.markCalibrated()
        model.refreshToday()
        let summary = last.flatMap { (try? engine.sessionSummary(sessionId: $0)) ?? nil }
        let lastWeek = Calendar.current.date(byAdding: .day, value: -7, to: Date()) ?? Date()
        var cal = Calendar(identifier: .gregorian)
        cal.firstWeekday = 2
        let weekStart = cal.dateInterval(of: .weekOfYear, for: lastWeek)?.start ?? lastWeek
        let monthStart = cal.dateInterval(of: .month, for: lastWeek)?.start ?? lastWeek
        let monthDays = cal.range(of: .day, in: .month, for: lastWeek)?.count ?? 30
        let week = try? engine.report(fromDate: Clock.dateString(weekStart), days: 7)
        let month = try? engine.report(fromDate: Clock.dateString(monthStart), days: UInt32(monthDays))

        for (name, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
            let ap = NSAppearance(named: name)!
            func shot<V: View>(_ file: String, width: CGFloat, height: CGFloat? = nil, _ view: V) {
                render(view.environmentObject(model), width: width, height: height, appearance: ap,
                       to: dir.appendingPathComponent("\(file)-\(suffix).png"))
            }

            model.debugPreview(update: nil, segments: [], label: "")
            shot("popover-idle", width: 300, PopoverView())

            if let s = summary {
                let u = EngineUpdate(
                    mode: .running, activity: .deskFocus, raw: .deskFocus, zone: .desk, detail: nil,
                    focusMs: UInt64(s.totals.focusMs), elapsedMs: UInt64(s.endedWall - s.startedWall),
                    presentMs: UInt64(s.presentMs), goalMs: 90 * 60_000, breakLeftMs: nil, nudge: nil,
                    askCategory: nil, welcomeBack: false, tooDark: false, notYou: false,
                    plan: SensingPlan(tickMs: 1000, landmarks: true, body: false, hands: false, objects: false, verify: false)
                )
                model.debugPreview(update: u, segments: s.segments, label: s.label ?? "")
                shot("popover-session", width: 300, PopoverView())
                model.debugPreview(update: nil, segments: [], label: "")
                shot("summary", width: 640, SummaryContent(summary: .constant(s)).background(Palette.paper))
            }

            shot("nudge", width: 330, NudgeView(nudge: Nudge(activity: .social, detail: "youtube.com", driftMs: 41_000), model: model, close: {}).padding(4))
            shot("ask", width: 330, AskCategoryView(host: "wikipedia.org", model: model, close: {}).padding(4))
            if let week {
                shot("analytics-week", width: 720, height: 820, AnalyticsView(range: .week, anchor: weekStart, preloaded: week))
            }
            if let month {
                shot("analytics-month", width: 720, height: 900, AnalyticsView(range: .month, anchor: monthStart, preloaded: month))
            }
            shot("onboarding-welcome", width: 560, height: 600, OnboardingView(start: .welcome))
            shot("onboarding-calibrate", width: 560, height: 600, OnboardingView(start: .calibrate))
            shot("onboarding-permissions", width: 560, height: 600, OnboardingView(start: .permissions))
            shot("settings", width: 600, height: 1500, SettingsView())
        }
    }

    static func render<V: View>(_ view: V, width: CGFloat, height: CGFloat?, appearance: NSAppearance, to url: URL) {
        let host = NSHostingView(rootView: AnyView(view.frame(width: width)))
        host.appearance = appearance
        let h = height ?? host.fittingSize.height
        let window = NSWindow(
            contentRect: NSRect(x: -30_000, y: -30_000, width: width, height: h),
            styleMask: [.borderless], backing: .buffered, defer: false
        )
        window.appearance = appearance
        window.backgroundColor = Palette.paperNS
        window.contentView = host
        window.orderFrontRegardless()
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.5))
        host.layoutSubtreeIfNeeded()
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return }
        host.cacheDisplay(in: host.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: url)
        window.orderOut(nil)
    }
}
