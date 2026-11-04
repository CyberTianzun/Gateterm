import SwiftUI

/// A handle SwiftUI uses to focus/blur the UIKit terminal (keyboard button, chat toggle).
@MainActor
final class TerminalHandle {
    weak var view: TerminalHostView?
    func focus() { view?.focus() }
    func blur() { view?.blur() }
    func releaseKeys() { view?.releaseKeys() }
    var isEditing: Bool { view?.isEditing ?? false }
}

struct TerminalSurface: UIViewRepresentable {
    @Environment(AppModel.self) private var model
    let handle: TerminalHandle
    let keyboard: TerminalKeyboard
    let softModifiers: KeyModifiers
    var topBarVisible: Bool = true
    let onModifiersConsumed: () -> Void
    var onHideTopBar: () -> Void = {}
    var onRevealTopBar: () -> Void = {}

    func makeUIView(context: Context) -> TerminalHostView {
        let view = TerminalHostView(frame: .zero)
        handle.view = view
        return view
    }

    func updateUIView(_ view: TerminalHostView, context: Context) {
        let model = model
        view.term = model.activeTerm
        view.appearance = model.appearance
        view.frameState = model.painted
        view.selection = model.selection
        view.mouseTracking = model.mouseTracking
        view.inputEnabled = !model.reconnecting
        view.keyboard = keyboard
        view.applicationCursor = model.activeTerm?.applicationCursor ?? false
        view.modifyOtherKeys = model.activeTerm?.modifyOtherKeys ?? 0
        view.kittyKeyboardFlags = model.activeTerm?.kittyKeyboardFlags ?? 0
        view.softModifiers = softModifiers
        view.topBarVisible = topBarVisible
        view.accessibilityHint = L.t("term_keys_hint")
        view.accessibilityLabel = model.attachedSession?.name
        view.onModifiersConsumed = onModifiersConsumed
        view.onHideTopBar = onHideTopBar
        view.onRevealTopBar = onRevealTopBar
        view.onInput = { [weak model] bytes in
            onHideTopBar()
            model?.sendInput(bytes)
        }
        view.onProposeSize = { [weak model] cols, rows in model?.proposeSize(cols: cols, rows: rows) }
        view.onScroll = { [weak model] lines in model?.scroll(by: lines) }
        view.onSelection = { [weak model] selection in model?.selection = selection }
        view.onMouse = { [weak model] col, row, pressed, moving in model?.sendMouse(col: col, row: row, pressed: pressed, moving: moving) }
        view.onCopy = { [weak model] in model?.copySelection() }
        view.onPaste = { [weak model] in model?.pasteClipboard() }
        view.onFontSize = { [weak model] size in model?.setFontSize(size) }
    }

    static func dismantleUIView(_ view: TerminalHostView, coordinator: ()) {
        view.blur()
    }
}
