import AppKit
import ApplicationServices
import CoreGraphics

struct ActivitySnapshot {
    var bundle: String?
    var name: String?
    var title: String?
    var host: String?
    var idleSeconds: Float
    var locked: Bool
}

/// What's in front on the Mac: app (event-driven), window title (one cheap
/// accessibility read per tick), and the browser's address (asked only when
/// the title changes, never by walking the page's accessibility tree).
@MainActor
final class ActivityWatcher {
    private var bundle: String?
    private var name: String?
    private var pid: pid_t = 0
    private var title: String?
    private var host: String?
    private var hostTitle: String?
    private var locked = false
    private var observers: [NSObjectProtocol] = []
    private var distributed: [NSObjectProtocol] = []
    private var deniedBrowsers: Set<String> = []
    private var scripts: [String: NSAppleScript] = [:]

    static var accessibilityTrusted: Bool { AXIsProcessTrusted() }

    static func promptAccessibility() {
        let key = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
        _ = AXIsProcessTrustedWithOptions([key: true] as CFDictionary)
    }

    var readBrowserAddresses: Bool {
        get { UserDefaults.standard.object(forKey: "readBrowserAddresses") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "readBrowserAddresses") }
    }

    func start() {
        guard observers.isEmpty else { return }
        setFrontmost(NSWorkspace.shared.frontmostApplication)
        observers.append(NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            MainActor.assumeIsolated { self?.setFrontmost(app) }
        })
        let dc = DistributedNotificationCenter.default()
        distributed.append(dc.addObserver(forName: .init("com.apple.screenIsLocked"), object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.locked = true }
        })
        distributed.append(dc.addObserver(forName: .init("com.apple.screenIsUnlocked"), object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.locked = false }
        })
    }

    func stop() {
        observers.forEach { NSWorkspace.shared.notificationCenter.removeObserver($0) }
        distributed.forEach { DistributedNotificationCenter.default().removeObserver($0) }
        observers.removeAll()
        distributed.removeAll()
    }

    private func setFrontmost(_ app: NSRunningApplication?) {
        bundle = app?.bundleIdentifier
        name = app?.localizedName
        pid = app?.processIdentifier ?? 0
        title = nil
        host = nil
        hostTitle = nil
    }

    func snapshot() -> ActivitySnapshot {
        let anyInput = CGEventType(rawValue: ~0)!
        let idle = Float(CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: anyInput))
        let newTitle = Self.accessibilityTrusted ? readTitle() : nil
        if newTitle != title {
            title = newTitle
        }
        if let b = bundle, Self.isScriptableBrowser(b), readBrowserAddresses, !deniedBrowsers.contains(b), hostTitle != title {
            hostTitle = title
            host = fetchHost(bundle: b)
        } else if let b = bundle, !Self.isScriptableBrowser(b) {
            host = nil
        }
        return ActivitySnapshot(bundle: bundle, name: name, title: title, host: host, idleSeconds: idle, locked: locked)
    }

    private func readTitle() -> String? {
        guard pid != 0 else { return nil }
        let app = AXUIElementCreateApplication(pid)
        AXUIElementSetMessagingTimeout(app, 0.2)
        var win: CFTypeRef?
        guard AXUIElementCopyAttributeValue(app, kAXFocusedWindowAttribute as CFString, &win) == .success,
              let w = win, CFGetTypeID(w) == AXUIElementGetTypeID() else { return nil }
        var t: CFTypeRef?
        guard AXUIElementCopyAttributeValue(w as! AXUIElement, kAXTitleAttribute as CFString, &t) == .success else { return nil }
        return t as? String
    }

    static let chromium: Set<String> = [
        "com.google.Chrome", "com.google.Chrome.canary", "company.thebrowser.Browser", "com.brave.Browser",
        "com.microsoft.edgemac", "com.vivaldi.Vivaldi", "org.chromium.Chromium", "com.operasoftware.Opera",
    ]
    static let safari: Set<String> = ["com.apple.Safari", "com.apple.SafariTechnologyPreview"]

    static func isScriptableBrowser(_ b: String) -> Bool { chromium.contains(b) || safari.contains(b) }

    private func fetchHost(bundle b: String) -> String? {
        let script: NSAppleScript
        if let s = scripts[b] {
            script = s
        } else {
            let body = Self.safari.contains(b) ? "return URL of front document" : "return URL of active tab of front window"
            let source = "with timeout of 1 second\ntell application id \"\(b)\"\n\(body)\nend tell\nend timeout"
            guard let s = NSAppleScript(source: source) else { return nil }
            scripts[b] = s
            script = s
        }
        var error: NSDictionary?
        let result = script.executeAndReturnError(&error)
        if let code = error?[NSAppleScript.errorNumber] as? Int, code == -1743 {
            deniedBrowsers.insert(b)
            return nil
        }
        guard error == nil, let s = result.stringValue, let url = URL(string: s) else { return nil }
        return url.host
    }
}
