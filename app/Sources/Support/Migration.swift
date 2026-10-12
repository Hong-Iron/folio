import AppKit

/// The app used to be called NoDistraction. On first launch as Folio, bring
/// its history, calibration, face template and settings along.
enum Migration {
    static let legacyBundleID = "app.nodistraction.NoDistraction"
    static let legacyFolder = "NoDistraction"
    private static let defaultsKeys = ["onboarded", "labelMode", "labelThumbnails", "nudgeChime", "readBrowserAddresses"]

    /// Returns false if the old app is still running and wouldn't quit, in
    /// which case nothing was moved and Folio shouldn't start.
    static func run() -> Bool {
        let fm = FileManager.default
        let legacy = AppPaths.base.appendingPathComponent(legacyFolder, isDirectory: true)
        let target = AppPaths.base.appendingPathComponent("Folio", isDirectory: true)
        guard fm.fileExists(atPath: legacy.path), !fm.fileExists(atPath: target.appendingPathComponent("nd.sqlite").path) else {
            return true
        }

        // Ask the old app to quit (it saves any open session as it does) so
        // two apps never write to the same history.
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: legacyBundleID)
        running.forEach { $0.terminate() }
        let deadline = Date().addingTimeInterval(8)
        while running.contains(where: { !$0.isTerminated }) && Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        guard running.allSatisfy(\.isTerminated) else { return false }

        if fm.fileExists(atPath: target.path) {
            try? fm.removeItem(at: target)
        }
        do {
            try fm.moveItem(at: legacy, to: target)
        } catch {
            return true
        }
        if let old = UserDefaults(suiteName: legacyBundleID) {
            for key in defaultsKeys {
                if let v = old.object(forKey: key), UserDefaults.standard.object(forKey: key) == nil {
                    UserDefaults.standard.set(v, forKey: key)
                }
            }
        }
        return true
    }
}
