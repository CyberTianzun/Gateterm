import SwiftUI

/// Terminal shortcut keys that sit directly above the software keyboard (SwiftUI keeps the
/// bottom bar above the keyboard, like an input accessory view). Scrolls horizontally;
/// agent-aware ordering comes from `AgentKeyboardLayouts`.
struct ExtraKeysBar: View {
    let rows: Int
    let sessionId: Int64
    let agentId: String?
    let layouts: [String: AgentKeyLayout]
    @Binding var ctrl: Bool
    @Binding var alt: Bool
    @Binding var shift: Bool
    let enabled: Bool
    let copyEnabled: Bool
    let scrollOffset: Int
    let keyboardShown: Bool
    let onKey: (KeyStroke) -> Void
    let onKeyboard: () -> Void
    let onCopy: () -> Void
    let onPaste: () -> Void
    let onJumpToBottom: () -> Void

    private var keys: [KeyboardShortcut] {
        AgentKeyboardLayouts.order(agentId: agentId, profiles: layouts).filter {
            !($0 == .copy && !copyEnabled) && !($0 == .jumpBottom && scrollOffset <= 0)
        }
    }

    var body: some View {
        let keys = keys
        let rowCount = min(2, max(1, rows))
        ScrollViewReader { proxy in
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(spacing: 6) {
                    ForEach(Array(stride(from: 0, to: keys.count, by: rowCount)), id: \.self) { start in
                        VStack(spacing: 6) {
                            ForEach(keys[start..<min(keys.count, start + rowCount)]) { key in button(key) }
                        }
                        .id(start)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
            }
            .onChange(of: "\(sessionId)|\(agentId ?? "")|\(rowCount)") { proxy.scrollTo(0, anchor: .leading) }
        }
        .frame(height: CGFloat(rowCount) * 38 + CGFloat(rowCount - 1) * 6 + 12)
        .background(.bar)
        .overlay(alignment: .top) { Divider() }
    }

    @ViewBuilder
    private func button(_ key: KeyboardShortcut) -> some View {
        let selected: Bool? = switch key {
        case .ctrl: ctrl
        case .alt: alt
        case .shift: shift
        default: nil
        }
        let local = [.keyboard, .copy, .jumpBottom].contains(key)
        Button {
            UIImpactFeedbackGenerator(style: .light).impactOccurred(intensity: 0.6)
            switch key {
            case .keyboard: onKeyboard()
            case .ctrl: ctrl.toggle()
            case .alt: alt.toggle()
            case .shift: shift.toggle()
            case .paste: onPaste()
            case .copy: onCopy()
            case .jumpBottom: onJumpToBottom()
            default: if let stroke = key.stroke { onKey(stroke) }
            }
        } label: {
            label(for: key)
                .font(.system(size: 14, weight: selected == true ? .bold : .medium, design: .rounded))
                .frame(minWidth: 40, minHeight: 38)
                .padding(.horizontal, 2)
                .background(selected == true ? Color.accentColor.opacity(0.25) : Color(uiColor: .secondarySystemFill),
                            in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                .overlay {
                    if selected == true { RoundedRectangle(cornerRadius: 7, style: .continuous).stroke(Color.accentColor, lineWidth: 1) }
                }
        }
        .buttonStyle(.plain)
        .disabled(!(local || enabled))
        .opacity(local || enabled ? 1 : 0.4)
        .accessibilityLabel(shortcutDescription(key))
        .accessibilityAddTraits(selected == true ? .isSelected : [])
    }

    @ViewBuilder
    private func label(for key: KeyboardShortcut) -> some View {
        switch key {
        case .keyboard: Image(systemName: keyboardShown ? "keyboard.chevron.compact.down" : "keyboard")
        case .paste: Image(systemName: "doc.on.clipboard")
        case .copy: Image(systemName: "doc.on.doc")
        case .jumpBottom: Image(systemName: "arrow.down.to.line")
        case .tab: Image(systemName: "arrow.right.to.line")
        case .backspace: Image(systemName: "delete.left")
        case .enter: Image(systemName: "return")
        default: Text(key.label)
        }
    }
}

func shortcutDescription(_ key: KeyboardShortcut) -> String {
    switch key {
    case .keyboard: L.t("term_keyboard")
    case .paste: L.t("term_paste")
    case .copy: L.t("term_copy")
    case .jumpBottom: L.t("term_jump_bottom")
    case .up: L.t("term_key_up")
    case .down: L.t("term_key_down")
    case .left: L.t("term_key_left")
    case .right: L.t("term_key_right")
    default: key.englishDescription
    }
}

func shortcutLabel(_ key: KeyboardShortcut) -> String {
    switch key {
    case .paste: L.t("term_paste")
    case .copy: L.t("term_copy")
    default: key.label
    }
}
