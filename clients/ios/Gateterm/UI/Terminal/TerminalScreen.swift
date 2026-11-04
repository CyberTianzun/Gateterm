import SwiftUI

/// Detail column: the attached terminal (or its chat view) with open-session tabs on wide layouts.
struct TerminalScreen: View {
    static let topBarAutoHideMs: UInt64 = 3000

    @Environment(AppModel.self) private var model
    @Environment(\.horizontalSizeClass) private var sizeClass
    let sessionId: Int64
    @State private var handle = TerminalHandle()
    @State private var ctrl = false
    @State private var alt = false
    @State private var shift = false
    @State private var keyboardShown = false
    @State private var keyboardSession: Int64 = -1
    @State private var keyboard = TerminalKeyboard(serverOS: "")
    @State private var topBarVisible = true
    @State private var topBarRevealTick = 0

    private var softModifiers: KeyModifiers { KeyModifiers(shift: shift, ctrl: ctrl, alt: alt) }
    private var agentId: String? { AgentKeyboardLayouts.agentForTerminal(model.agents, sessionId) }
    private var titleText: String { model.attachedSession.map(sessionTitle) ?? model.selectedConnection?.title ?? "" }

    private func revealTopBar() {
        withAnimation(.easeInOut(duration: 0.2)) {
            topBarVisible = true
        }
        topBarRevealTick &+= 1
    }

    private func hideTopBar() {
        guard topBarVisible else { return }
        withAnimation(.easeInOut(duration: 0.2)) {
            topBarVisible = false
        }
    }

    var body: some View {
        let _ = model.localeRevision
        VStack(spacing: 0) {
            if sizeClass == .regular && model.openSessionIds.count > 1 && (topBarVisible || model.chatOpen) { tabs }
            if model.reconnecting { ReconnectingBanner() }
            if model.chatOpen {
                ChatView(chat: model.chat, onOpenTerminal: model.toggleChat, onLoadOlder: model.loadOlderChat)
            } else {
                TerminalSurface(
                    handle: handle,
                    keyboard: currentKeyboard,
                    softModifiers: softModifiers,
                    topBarVisible: topBarVisible,
                    onModifiersConsumed: clearModifiers,
                    onHideTopBar: hideTopBar,
                    onRevealTopBar: revealTopBar
                )
                ExtraKeysBar(
                    rows: model.extraKeyRows, sessionId: sessionId,
                    agentId: agentId, layouts: model.agentKeyLayouts,
                    ctrl: $ctrl, alt: $alt, shift: $shift, enabled: !model.reconnecting, copyEnabled: model.selection != nil,
                    scrollOffset: model.scrollOffset, keyboardShown: keyboardShown,
                    onKey: tap, onKeyboard: toggleKeyboard, onCopy: model.copySelection, onPaste: model.pasteClipboard,
                    onJumpToBottom: model.jumpToBottom)
            }
        }
        .background(Color(rgb: model.appearance.background))
        .navigationTitle(titleText)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(topBarVisible || model.chatOpen ? .visible : .hidden, for: .navigationBar)
        .toolbar { toolbar }
        .task(id: "\(sessionId)|\(topBarVisible)|\(topBarRevealTick)|\(model.chatOpen)") {
            guard topBarVisible && !model.chatOpen else { return }
            try? await Task.sleep(for: .milliseconds(Self.topBarAutoHideMs))
            guard !Task.isCancelled else { return }
            hideTopBar()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in keyboardShown = true }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in keyboardShown = false }
        .onChange(of: model.reconnecting) { _, reconnecting in if reconnecting { keyboard.reset(); clearModifiers() } }
        .onChange(of: keyboardIdentity, initial: true) { _, _ in replaceKeyboard() }
        .onChange(of: agentId) { _, _ in clearModifiers() }
        .onChange(of: sessionId) { _, _ in
            clearModifiers()
            topBarVisible = true
            topBarRevealTick &+= 1
        }
        .onDisappear {
            // Releases go to the session that owned the pressed keys, never to the next one.
            let released = keyboard.releaseAll()
            if !released.isEmpty { model.sendInput(sessionId, released) }
            handle.blur()
        }
    }

    private var currentKeyboard: TerminalKeyboard { keyboard }

    /// One keyboard state machine per session and server mode; win32 key-up needs the key-down identity.
    private var keyboardIdentity: String { "\(sessionId)|\(model.serverOS)|\(model.win32InputMode)" }

    private func replaceKeyboard() {
        let released = keyboard.releaseAll()
        if !released.isEmpty && keyboardSession >= 0 { model.sendInput(keyboardSession, released) }
        keyboard = TerminalKeyboard(serverOS: model.serverOS, win32InputMode: model.win32InputMode)
        keyboardSession = sessionId
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            HStack(spacing: 8) {
                if let agentId, !agentId.isEmpty {
                    AgentBrandIcon(agentId: agentId)
                }
                Text(titleText)
                    .font(.headline)
                    .lineLimit(1)
            }
        }
        ToolbarItemGroup(placement: .topBarTrailing) {
            // 只有本会话里有读得了记录的 agent 才给入口;已经在聊天里时回终端永远可以
            if model.chatOpen || model.chatAvailable {
                Button {
                    if !model.chatOpen { handle.blur(); clearModifiers() }
                    model.toggleChat()
                } label: {
                    Label(L.t(model.chatOpen ? "ios_terminal_view" : "ios_chat_view"),
                          systemImage: model.chatOpen ? "terminal" : "bubble.left.and.text.bubble.right")
                }
                .accessibilityLabel(L.t(model.chatOpen ? "chat_back_to_terminal_desc" : "chat_open_desc"))
            }
            Menu {
                if let session = model.attachedSession {
                    Button { model.copySelection() } label: { Label(L.t("term_copy"), systemImage: "doc.on.doc") }
                        .disabled(model.selection == nil)
                    Button { model.pasteClipboard() } label: { Label(L.t("term_paste"), systemImage: "doc.on.clipboard") }
                    Divider()
                    Button { model.setFontSize(model.appearance.fontSize + 1) } label: { Label("A+", systemImage: "textformat.size.larger") }
                    Button { model.setFontSize(model.appearance.fontSize - 1) } label: { Label("A−", systemImage: "textformat.size.smaller") }
                    Divider()
                    if model.openSessionIds.count > 1 {
                        Button { model.closeOpenSession(session.id) } label: { Label(L.t("ios_close_tab"), systemImage: "xmark.square") }
                    }
                    Button(role: .destructive) { model.killSession(session.id) } label: {
                        Label(L.t("session_kill"), systemImage: "xmark.octagon")
                    }
                }
            } label: {
                Label(L.t("ios_more"), systemImage: "ellipsis.circle")
            }
        }
    }

    private var tabs: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(model.openSessionIds, id: \.self) { id in
                    let session = model.sessions.first { $0.id == id }
                    let active = id == model.attachedId
                    HStack(spacing: 6) {
                        if let agent = AgentSessions.forSession(model.agents, id) { AgentStatusBadge(agent: agent, size: 16) }
                        Text(session.map(sessionTitle) ?? "#\(id)").lineLimit(1)
                        Button { model.closeOpenSession(id) } label: { Image(systemName: "xmark").font(.caption2.weight(.bold)) }
                            .buttonStyle(.plain)
                            .foregroundStyle(.secondary)
                            .accessibilityLabel(L.t("term_tab_close"))
                    }
                    .font(.subheadline)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(active ? Color.accentColor.opacity(0.18) : Color(uiColor: .secondarySystemFill), in: Capsule())
                    .contentShape(Capsule())
                    .onTapGesture { model.openSession(id) }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
        .background(.bar)
    }

    private func tap(_ stroke: KeyStroke) {
        guard !model.reconnecting, let term = model.activeTerm else { return }
        hideTopBar()
        var next = stroke
        next.modifiers = stroke.modifiers + softModifiers
        model.sendInput(
            sessionId,
            currentKeyboard.tap(
                next,
                applicationCursor: term.applicationCursor,
                modifyOtherKeys: term.modifyOtherKeys,
                kittyKeyboardFlags: term.kittyKeyboardFlags
            )
        )
        clearModifiers()
    }

    private func toggleKeyboard() {
        if handle.isEditing { handle.blur() } else { handle.focus() }
    }

    private func clearModifiers() {
        ctrl = false
        alt = false
        shift = false
    }
}

func sessionTitle(_ session: SessionInfo) -> String { session.name.isEmpty ? "#\(session.id)" : session.name }

struct ReconnectingBanner: View {
    var body: some View {
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(L.t("ios_reconnecting_banner")).font(.footnote)
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(.yellow.opacity(0.18))
    }
}

extension Color {
    init(rgb: Int) {
        self.init(red: Double((rgb >> 16) & 0xFF) / 255, green: Double((rgb >> 8) & 0xFF) / 255, blue: Double(rgb & 0xFF) / 255)
    }
}
