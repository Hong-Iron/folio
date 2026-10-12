import AppKit
import SwiftUI

struct SettingsView: View {
    @EnvironmentObject var model: AppModel
    @State private var config = Config.fallback
    @State private var rules: [Rule] = []
    @State private var newPattern = ""
    @State private var newCategory: Category = .drift
    @State private var filter = ""
    @State private var confirmDelete = false
    @State private var notice: String?
    @State private var labelMode = false
    @State private var thumbnails = false
    @State private var chime = true
    @State private var browserAddresses = true

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                Text("Settings").font(Fonts.serif(28)).foregroundStyle(Palette.ink)

                section("Sites and apps", note: "Focus and neutral count as work while you're active. Distractions count as drift, wherever you're looking.") {
                    HStack(spacing: 8) {
                        TextField("", text: $newPattern, prompt: Text("youtube.com or an app name").foregroundStyle(Palette.ink3))
                            .textFieldStyle(.plain).font(Fonts.ui(13))
                            .padding(.horizontal, 10).padding(.vertical, 6)
                            .background(RoundedRectangle(cornerRadius: 7).fill(Palette.raised))
                            .overlay(RoundedRectangle(cornerRadius: 7).stroke(Palette.rule, lineWidth: 0.5))
                            .onSubmit(addRule)
                        CategoryPicker(selection: $newCategory)
                        Button("Add", action: addRule).buttonStyle(QuietButtonStyle(small: true))
                    }
                    TextField("", text: $filter, prompt: Text("Filter").foregroundStyle(Palette.ink3))
                        .textFieldStyle(.plain).font(Fonts.ui(12))
                        .padding(.top, 10)
                    VStack(spacing: 0) {
                        ForEach(filteredRules, id: \.self) { rule in
                            HStack(spacing: 10) {
                                Image(systemName: rule.kind == .domain ? "globe" : "app")
                                    .font(.system(size: 11)).foregroundStyle(Palette.ink3).frame(width: 14)
                                if rule.kind == .app, let name = AppNames.display(rule.pattern), name != rule.pattern {
                                    VStack(alignment: .leading, spacing: 1) {
                                        Text(name).font(Fonts.ui(13)).foregroundStyle(Palette.ink).lineLimit(1)
                                        Text(rule.pattern).font(Fonts.ui(10)).foregroundStyle(Palette.ink3).lineLimit(1)
                                    }
                                } else {
                                    Text(rule.pattern).font(Fonts.ui(13)).foregroundStyle(Palette.ink).lineLimit(1)
                                }
                                Spacer()
                                CategoryPicker(selection: Binding(
                                    get: { rule.category },
                                    set: { c in model.setRule(rule.pattern, kind: rule.kind, category: c); reload() }
                                ))
                                Button { model.removeRule(rule); reload() } label: { Image(systemName: "minus.circle") }
                                    .buttonStyle(.plain).foregroundStyle(Palette.ink3)
                                    .accessibilityLabel("Remove \(rule.pattern)")
                            }
                            .padding(.vertical, 6)
                            .overlay(alignment: .top) { Hairline() }
                        }
                        Hairline()
                    }
                    .padding(.top, 6)
                }

                section("Nudges", note: "A small note in the corner when you drift. It never blocks anything.") {
                    Toggle("Nudge me when I drift", isOn: Binding(get: { config.nudges }, set: { config.nudges = $0; save() }))
                    Toggle("Play a soft chime", isOn: $chime).onChange(of: chime) { _, v in model.nudges.chime = v }
                    SliderRow(title: "Nudge after", value: $config.nudgeAfterS, range: 15...180, unit: "s", onCommit: save)
                    SliderRow(title: "Phone grace", value: $config.gracePhoneS, range: 10...90, unit: "s", onCommit: save)
                    SliderRow(title: "Looking away counts after", value: $config.graceZonedS, range: 20...180, unit: "s", onCommit: save)
                    SliderRow(title: "Social grace", value: $config.graceSocialS, range: 5...60, unit: "s", onCommit: save)
                }

                section("Your face", note: model.faceEnrolled
                        ? "Enrolled. Folio checks it's you when a session starts, when you come back, and every couple of minutes. Time with someone else in the chair doesn't count."
                        : "Not enrolled, so Folio counts whoever is in front of the camera.") {
                    HStack(spacing: 8) {
                        if model.faceEnrolled {
                            Button("Enroll again") { WindowRouter.shared.showOnboarding(startAt: .face, single: true) }.buttonStyle(QuietButtonStyle(small: true))
                            Button("Add a look") { WindowRouter.shared.showOnboarding(startAt: .face, single: true, faceMode: .add) }.buttonStyle(QuietButtonStyle(small: true))
                            Button("Forget my face") { model.forgetFace() }.buttonStyle(QuietButtonStyle(small: true))
                        } else {
                            Button("Enroll your face") { WindowRouter.shared.showOnboarding(startAt: .face, single: true) }.buttonStyle(QuietButtonStyle(small: true))
                        }
                    }
                }

                section("Camera and calibration", note: "Calibration teaches the app what your screen and your desk look like from your camera.") {
                    HStack(spacing: 8) {
                        Button("Recalibrate") { WindowRouter.shared.showOnboarding(startAt: .calibrate, single: true) }.buttonStyle(QuietButtonStyle(small: true))
                        Button("Check camera framing") { WindowRouter.shared.showOnboarding(startAt: .camera, single: true) }.buttonStyle(QuietButtonStyle(small: true))
                    }
                    Toggle("Read the address of the browser tab in front", isOn: $browserAddresses)
                        .onChange(of: browserAddresses) { _, v in model.activity.readBrowserAddresses = v }
                    Text(ActivityWatcher.accessibilityTrusted
                         ? "Accessibility access is on, so window titles can be read."
                         : "Accessibility access is off, so only app names are used. Turn it on in System Settings.")
                        .font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
                    if !ActivityWatcher.accessibilityTrusted {
                        Button("Open Accessibility settings") { ActivityWatcher.promptAccessibility() }.buttonStyle(QuietButtonStyle(small: true))
                    }
                }

                section("Accuracy lab", note: "Label what you're really doing while you study, then check how well the app agreed. Traces hold numbers only, never images.") {
                    Toggle("Label mode", isOn: $labelMode).onChange(of: labelMode) { _, v in
                        model.recorder.enabled = v
                        model.updateLabelHotKeys()
                    }
                    if labelMode {
                        VStack(alignment: .leading, spacing: 3) {
                            ForEach(LabelRecorder.labels, id: \.key) { l in
                                Text("⌃⌥\(l.key)  \(l.title)").font(Fonts.ui(12).monospaced()).foregroundStyle(Palette.ink2)
                            }
                        }
                        Toggle("Also keep small thumbnails for reviewing labels (deleted after 14 days)", isOn: $thumbnails)
                            .onChange(of: thumbnails) { _, v in model.recorder.keepThumbnails = v }
                        HStack(spacing: 8) {
                            Button("Open traces folder") {
                                try? FileManager.default.createDirectory(at: LabelRecorder.folder, withIntermediateDirectories: true)
                                NSWorkspace.shared.open(LabelRecorder.folder)
                            }
                            .buttonStyle(QuietButtonStyle(small: true))
                            Button("Delete traces") { LabelRecorder.deleteAll(); notice = "Traces deleted." }.buttonStyle(QuietButtonStyle(small: true))
                        }
                        Text("Check a trace with: nd-replay run <file>.jsonl").font(Fonts.ui(11).monospaced()).foregroundStyle(Palette.ink3)
                    }
                }

                section("Your data", note: "Everything stays on this Mac. No account, no cloud, no images.") {
                    HStack(spacing: 8) {
                        Button("Export as CSV…", action: exportCSV).buttonStyle(QuietButtonStyle(small: true))
                        Button("Delete all data…") { confirmDelete = true }.buttonStyle(QuietButtonStyle(small: true))
                    }
                    Text("Folio \(coreVersion())").font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                }

                if let notice {
                    Text(notice).font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
                }
            }
            .padding(.horizontal, 28)
            .padding(.top, 36)
            .padding(.bottom, 28)
            .toggleStyle(.checkbox)
            .tint(Palette.moss)
            .font(Fonts.ui(13))
            .foregroundStyle(Palette.ink)
        }
        .background(Palette.paper)
        .onAppear {
            config = model.engine.config()
            labelMode = model.recorder.enabled
            thumbnails = model.recorder.keepThumbnails
            chime = model.nudges.chime
            browserAddresses = model.activity.readBrowserAddresses
            reload()
        }
        .alert("Delete all data?", isPresented: $confirmDelete) {
            Button("Delete", role: .destructive) {
                if model.sessionActive { model.endSession(showSummary: false) }
                try? model.engine.deleteAllData()
                LabelRecorder.deleteAll()
                UserDefaults.standard.removeObject(forKey: "onboarded")
                model.markCalibrated()
                model.refreshFaceState()
                model.refreshToday()
                notice = "All sessions, your face template and calibration were deleted."
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This removes every session, your face template and your calibration. Site rules stay. This can't be undone.")
        }
    }

    private var filteredRules: [Rule] {
        let f = filter.trimmingCharacters(in: .whitespaces).lowercased()
        func key(_ r: Rule) -> String { (AppNames.display(r.pattern) ?? r.pattern).lowercased() }
        let sorted = rules.sorted { ($0.category.order, key($0)) < ($1.category.order, key($1)) }
        return f.isEmpty ? sorted : sorted.filter { $0.pattern.lowercased().contains(f) || key($0).contains(f) }
    }

    private func reload() {
        rules = model.engine.rules()
    }

    private func save() {
        try? model.engine.setConfig(cfg: config)
    }

    private func addRule() {
        let p = newPattern.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !p.isEmpty else { return }
        var pattern = p
        if let url = URL(string: p), let host = url.host { pattern = host }
        let isDomain = pattern.contains(".") && !pattern.contains(" ") && !pattern.hasPrefix("com.")
        model.setRule(pattern, kind: isDomain ? .domain : .app, category: newCategory)
        newPattern = ""
        reload()
    }

    private func exportCSV() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "folio-\(Clock.dateString()).csv"
        panel.allowedContentTypes = [.commaSeparatedText]
        NSApp.activate(ignoringOtherApps: true)
        if panel.runModal() == .OK, let url = panel.url {
            let n = (try? model.engine.exportCsv(path: url.path)) ?? 0
            notice = "Exported \(n) segments."
        }
    }

    @ViewBuilder
    private func section<C: View>(_ title: String, note: String, @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(Fonts.serif(18)).foregroundStyle(Palette.ink)
            Text(note).font(Fonts.ui(12)).foregroundStyle(Palette.ink2).fixedSize(horizontal: false, vertical: true)
            content()
        }
        .padding(.top, 16)
        .overlay(alignment: .top) { Hairline() }
    }
}

private struct CategoryPicker: View {
    @Binding var selection: Category

    var body: some View {
        HStack(spacing: 2) {
            ForEach([Category.focus, .neutral, .drift], id: \.self) { c in
                Button(c.title) { selection = c }
                    .buttonStyle(.plain)
                    .font(Fonts.ui(11))
                    .foregroundStyle(selection == c ? Palette.ink : Palette.ink3)
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(RoundedRectangle(cornerRadius: 5).fill(selection == c ? Palette.raised : Color.clear))
            }
        }
        .padding(2)
        .background(RoundedRectangle(cornerRadius: 7).fill(Palette.sunk))
    }
}

private struct SliderRow: View {
    let title: String
    @Binding var value: Float
    let range: ClosedRange<Float>
    let unit: String
    let onCommit: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Text(title).font(Fonts.ui(13)).foregroundStyle(Palette.ink).frame(width: 180, alignment: .leading)
            Slider(value: $value, in: range, step: 5) { editing in if !editing { onCommit() } }
                .controlSize(.small)
            Text("\(Int(value)) \(unit)").font(Fonts.serif(13)).foregroundStyle(Palette.ink2).frame(width: 52, alignment: .trailing)
        }
    }
}

extension Category {
    var title: String {
        switch self {
        case .focus: return "Focus"
        case .neutral: return "Neutral"
        case .drift: return "Distraction"
        }
    }

    var order: Int {
        switch self {
        case .drift: return 0
        case .focus: return 1
        case .neutral: return 2
        }
    }
}

extension Config {
    /// Placeholder until the engine's real config loads.
    @MainActor static var fallback: Config { AppModel.shared.engine.config() }
}

/// Friendly names for app rules, which are stored as bundle ids.
enum AppNames {
    private static let known: [String: String] = [
        "com.hnc.Discord": "Discord", "com.valvesoftware.steam": "Steam", "com.apple.dt.Xcode": "Xcode",
        "com.microsoft.VSCode": "Visual Studio Code", "com.todesktop.230313mzl4w4u92": "Cursor",
        "com.jetbrains.*": "JetBrains apps", "notion.id": "Notion", "md.obsidian": "Obsidian",
        "com.goodnotesapp.x": "GoodNotes", "net.ankiweb.dtop": "Anki", "org.zotero.zotero": "Zotero",
        "com.apple.iWork.Pages": "Pages", "com.apple.iWork.Numbers": "Numbers", "com.apple.iWork.Keynote": "Keynote",
        "com.microsoft.Word": "Microsoft Word", "com.microsoft.Excel": "Microsoft Excel",
        "com.microsoft.Powerpoint": "Microsoft PowerPoint", "com.readdle.PDFExpert-Mac": "PDF Expert",
        "com.apple.Preview": "Preview", "com.apple.Notes": "Notes",
    ]
    private static var cache: [String: String?] = [:]

    static func display(_ pattern: String) -> String? {
        if let k = known[pattern] { return k }
        if let c = cache[pattern] { return c }
        var name: String?
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: pattern) {
            name = FileManager.default.displayName(atPath: url.path).replacingOccurrences(of: ".app", with: "")
        }
        cache[pattern] = name
        return name
    }
}
