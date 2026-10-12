import AppKit
import SwiftUI

/// One window per kind (summary, analytics, settings, onboarding), reused.
@MainActor
final class WindowRouter {
    static let shared = WindowRouter()
    private var windows: [String: NSWindow] = [:]

    func show<V: View>(_ id: String, title: String, size: NSSize, resizable: Bool = false, @ViewBuilder content: () -> V) {
        let root = content()
            .environmentObject(AppModel.shared)
            .frame(minWidth: size.width, minHeight: size.height)
            .background(Palette.paper)
        let window: NSWindow
        if let existing = windows[id] {
            window = existing
            window.contentView = NSHostingView(rootView: root)
        } else {
            var style: NSWindow.StyleMask = [.titled, .closable, .miniaturizable, .fullSizeContentView]
            if resizable { style.insert(.resizable) }
            window = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: style, backing: .buffered, defer: false)
            window.title = title
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            window.isReleasedWhenClosed = false
            window.backgroundColor = Palette.paperNS
            window.contentView = NSHostingView(rootView: root)
            window.center()
            windows[id] = window
        }
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func close(_ id: String) {
        windows[id]?.close()
    }

    func showSummary(_ summary: SessionSummary) {
        show("summary", title: "Session summary", size: NSSize(width: 640, height: 600)) {
            SummaryView(summary: summary)
        }
    }

    func showAnalytics() {
        show("analytics", title: "Analytics", size: NSSize(width: 720, height: 720), resizable: true) {
            AnalyticsView()
        }
    }

    func showSettings() {
        show("settings", title: "Settings", size: NSSize(width: 600, height: 680), resizable: true) {
            SettingsView()
        }
    }

    func showOnboarding(startAt step: OnboardingStep = .welcome, single: Bool = false, faceMode: FaceMode = .replace) {
        show("onboarding", title: "Welcome", size: NSSize(width: 560, height: 600)) {
            OnboardingView(start: step, single: single, faceMode: faceMode)
        }
    }
}
