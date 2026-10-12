import AppKit
import Combine
import SwiftUI

/// The menu bar item: the app's mark when idle; a state dot and a quiet
/// focus clock (minutes only, so nothing ticks) during a session.
@MainActor
final class MenuBarController: NSObject, NSPopoverDelegate {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let popover = NSPopover()
    private var bag: Set<AnyCancellable> = []
    private let model: AppModel

    init(model: AppModel) {
        self.model = model
        super.init()
        popover.behavior = .transient
        popover.animates = true
        popover.delegate = self
        popover.contentViewController = NSHostingController(rootView: PopoverView().environmentObject(model))

        if let button = item.button {
            button.target = self
            button.action = #selector(toggle)
            button.imagePosition = .imageOnly
            button.image = Glyph.menuBarImage()
            button.setAccessibilityLabel("Folio")
        }

        model.$update.combineLatest(model.$sessionActive)
            .receive(on: RunLoop.main)
            .sink { [weak self] update, active in self?.render(update, active: active) }
            .store(in: &bag)
    }

    private func render(_ u: EngineUpdate?, active: Bool) {
        guard let button = item.button else { return }
        guard active else {
            button.image = Glyph.menuBarImage()
            button.attributedTitle = NSAttributedString(string: "")
            button.imagePosition = .imageOnly
            return
        }
        let paused = u?.mode == .paused || u?.mode == .break
        button.image = Glyph.stateDot(u?.activity, paused: paused)
        button.imagePosition = .imageLeading
        let text = " " + Fmt.clock(u?.focusMs ?? 0)
        button.attributedTitle = NSAttributedString(string: text, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 13, weight: .regular),
        ])
        button.setAccessibilityLabel("Folio, \(Fmt.duration(u?.focusMs ?? 0)) focused")
    }

    @objc private func toggle() {
        if popover.isShown {
            popover.performClose(nil)
        } else if let button = item.button {
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
            popover.contentViewController?.view.window?.makeKey()
        }
    }

    func popoverDidShow(_ notification: Notification) {
        model.refreshToday()
    }

    func close() {
        popover.performClose(nil)
    }
}
