import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var menuBar: MenuBarController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        NSApp.mainMenu = Self.makeMainMenu()
        let model = AppModel.shared
        menuBar = MenuBarController(model: model)
        model.installHotKeys()
        if !model.calibrated || !CameraSource.authorized {
            WindowRouter.shared.showOnboarding()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Quitting mid-session saves it like a normal end.
        AppModel.shared.endSession(showSummary: false)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    /// Accessory apps have no visible menu bar, but text fields still need
    /// the standard edit shortcuts.
    private static func makeMainMenu() -> NSMenu {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit Folio", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)

        let editItem = NSMenuItem()
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "Z")
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        main.addItem(editItem)

        let windowItem = NSMenuItem()
        let window = NSMenu(title: "Window")
        window.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowItem.submenu = window
        main.addItem(windowItem)
        return main
    }
}

let app = NSApplication.shared

if let i = CommandLine.arguments.firstIndex(of: "--snapshots"), i + 1 < CommandLine.arguments.count {
    // Never touch the real history: snapshots always use a scratch database.
    let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("nd-snapshots-\(getpid()).sqlite")
    setenv("ND_DB_PATH", scratch.path, 1)
    let dir = URL(fileURLWithPath: CommandLine.arguments[i + 1])
    app.setActivationPolicy(.accessory)
    DispatchQueue.main.async {
        MainActor.assumeIsolated { Snapshots.run(to: dir) }
        try? FileManager.default.removeItem(at: scratch)
        exit(0)
    }
    app.run()
} else {
    if !Migration.run() {
        let alert = NSAlert()
        alert.messageText = "Quit NoDistraction first"
        alert.informativeText = "Folio is the new name for NoDistraction. Quit the old app, then open Folio again, and your history will come along."
        alert.runModal()
        exit(0)
    }
    let delegate = MainActor.assumeIsolated { AppDelegate() }
    app.delegate = delegate
    app.run()
}
