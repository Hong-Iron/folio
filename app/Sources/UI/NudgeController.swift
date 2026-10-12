import AppKit
import SwiftUI

/// The gentle note in the top-right corner. It never takes focus, never
/// blocks, and goes away by itself once you're back on task.
@MainActor
final class NudgeController {
    private weak var model: AppModel?
    private var panel: NSPanel?
    private var kind: Kind?

    enum Kind: Equatable {
        case drift
        case ask
    }

    var chime: Bool {
        get { UserDefaults.standard.object(forKey: "nudgeChime") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "nudgeChime") }
    }

    init(model: AppModel) {
        self.model = model
    }

    func show(_ n: Nudge) {
        guard let model else { return }
        present(.drift, NudgeView(nudge: n, model: model, close: { [weak self] in self?.dismissAll() }))
        if chime, let s = NSSound(named: "Purr") {
            s.volume = 0.35
            s.play()
        }
    }

    func ask(host: String) {
        guard let model, kind == nil else { return }
        present(.ask, AskCategoryView(host: host, model: model, close: { [weak self] in self?.dismissAll() }))
    }

    func dismissDrift() {
        if kind == .drift { dismissAll() }
    }

    func dismissAll() {
        guard let panel else { return }
        self.panel = nil
        kind = nil
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.18
            panel.animator().alphaValue = 0
        }, completionHandler: {
            panel.orderOut(nil)
        })
    }

    private func present<V: View>(_ kind: Kind, _ view: V) {
        panel?.orderOut(nil)
        let host = NSHostingView(rootView: view)
        host.frame.size = host.fittingSize
        let p = NSPanel(
            contentRect: NSRect(origin: .zero, size: host.fittingSize),
            styleMask: [.nonactivatingPanel, .borderless],
            backing: .buffered,
            defer: false
        )
        p.isFloatingPanel = true
        p.level = .statusBar
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        p.backgroundColor = .clear
        p.isOpaque = false
        p.hasShadow = false
        p.hidesOnDeactivate = false
        p.contentView = host
        if let screen = NSScreen.main {
            let vf = screen.visibleFrame
            p.setFrameOrigin(NSPoint(x: vf.maxX - host.fittingSize.width - 16, y: vf.maxY - host.fittingSize.height - 12))
        }
        p.alphaValue = 0
        p.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.2
            p.animator().alphaValue = 1
        }
        panel = p
        self.kind = kind
    }
}

private struct NoteFrame<Content: View>: View {
    let dot: Color
    @ViewBuilder let content: Content

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Circle().fill(dot).frame(width: 8, height: 8).padding(.top, 6)
            VStack(alignment: .leading, spacing: 0) { content }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .frame(width: 340, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).fill(Palette.raised))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Palette.rule, lineWidth: 0.5))
        .padding(1)
    }
}

struct NudgeView: View {
    let nudge: Nudge
    let model: AppModel
    let close: () -> Void

    private var headline: String {
        switch nudge.activity {
        case .social: return "You've wandered to \(nudge.detail ?? "a distracting site")"
        case .phone: return "Your phone's been out for a bit"
        case .dozing: return "Your eyes have been closed a while"
        case .zoned: return "You've been looking away for a bit"
        default: return "You've drifted"
        }
    }

    var body: some View {
        NoteFrame(dot: Palette.clay) {
            Text(headline).font(Fonts.ui(14)).foregroundStyle(Palette.ink)
            Text("About \(Fmt.duration(nudge.driftMs)) so far.")
                .font(Fonts.ui(12)).foregroundStyle(Palette.ink2).padding(.top, 2)
            HStack(spacing: 6) {
                Button("Back to it", action: close).buttonStyle(InkButtonStyle())
                Button("Break, 5 min") {
                    model.takeBreak(minutes: 5)
                    close()
                }
                .buttonStyle(QuietButtonStyle(small: true))
                if nudge.activity == .social, let site = nudge.detail {
                    Button("This is work") {
                        let isDomain = site.contains(".") && !site.contains(" ")
                        model.setRule(site, kind: isDomain ? .domain : .app, category: .focus)
                        close()
                    }
                    .buttonStyle(QuietButtonStyle(small: true))
                }
            }
            .padding(.top, 10)
        }
    }
}

struct AskCategoryView: View {
    let host: String
    let model: AppModel
    let close: () -> Void

    var body: some View {
        NoteFrame(dot: Palette.ink3) {
            Text("How should \(host) count?").font(Fonts.ui(14)).foregroundStyle(Palette.ink)
            Text("You've spent a while there this session. You can change this later in Settings.")
                .font(Fonts.ui(12)).foregroundStyle(Palette.ink2).padding(.top, 2)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 6) {
                Button("Focus") { pick(.focus) }.buttonStyle(QuietButtonStyle(small: true))
                Button("Neutral") { pick(.neutral) }.buttonStyle(QuietButtonStyle(small: true))
                Button("Distraction") { pick(.drift) }.buttonStyle(QuietButtonStyle(small: true))
            }
            .padding(.top, 10)
        }
    }

    private func pick(_ c: Category) {
        model.setRule(host, kind: .domain, category: c)
        close()
    }
}
