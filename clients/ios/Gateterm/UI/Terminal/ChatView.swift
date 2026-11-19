import SwiftUI

/// Agent chat view: same presentation as web/Android — a role stripe per message, thinking and
/// tool blocks collapsed into disclosure groups. When the transcript cannot be read, say why and
/// offer the terminal instead (protocol A9).
struct ChatView: View {
    let chat: ChatUiState
    var appearance: TermAppearance
    @Binding var userOnly: Bool
    var onOpenTerminal: () -> Void
    var onLoadOlder: () -> Void

    @State private var followBottom = true
    @State private var pendingJumpId: String?
    @State private var highlightedId: String?
    @State private var jumping = false
    @State private var jumpTask: Task<Void, Never>?
    @State private var highlightClearTask: Task<Void, Never>?

    init(
        chat: ChatUiState,
        appearance: TermAppearance = AppearanceCodec.default(),
        userOnly: Binding<Bool> = .constant(false),
        onOpenTerminal: @escaping () -> Void,
        onLoadOlder: @escaping () -> Void = {}
    ) {
        self.chat = chat
        self.appearance = appearance
        self._userOnly = userOnly
        self.onOpenTerminal = onOpenTerminal
        self.onLoadOlder = onLoadOlder
    }

    private var palette: UiPalette { appearance.palette }

    var body: some View {
        let p = palette
        Group {
            if let notice = chat.notice {
                VStack(alignment: .leading, spacing: 16) {
                    Text(noticeText(notice))
                        .font(.system(size: 13.5, design: .monospaced))
                    Button(L.t("chat_open_terminal_instead"), action: onOpenTerminal)
                        .font(.system(size: 13.5, design: .monospaced))
                        .buttonStyle(.bordered)
                    Spacer()
                }
                .padding(24)
                .frame(maxWidth: .infinity, alignment: .leading)
            } else if chat.messages.isEmpty && chat.loading {
                ContentUnavailableView {
                    ProgressView()
                } description: {
                    Text(L.t("chat_loading"))
                        .font(.system(size: 13.5, design: .monospaced))
                }
            } else if chat.messages.isEmpty && chat.olderCursor == nil {
                ContentUnavailableView(L.t("chat_empty"), systemImage: "bubble.left.and.bubble.right")
            } else {
                messagesView(palette: p)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(rgb: p.background))
        .foregroundStyle(Color(rgb: p.foreground))
        .tint(Color(rgb: p.accent))
        .environment(\.colorScheme, p.light ? .light : .dark)
    }

    private func messagesView(palette: UiPalette) -> some View {
        let userPrompts = Chat.userPrompts(chat.messages)
        return VStack(spacing: 0) {
            if userOnly {
                FilterStatusBanner(
                    count: userPrompts.count,
                    palette: palette,
                    onClear: { userOnly = false }
                )
            }
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        if chat.historySupported {
                            HStack {
                                Spacer()
                                if chat.olderCursor != nil {
                                    Button(
                                        L.t(chat.olderLoading ? "chat_loading_older" : chat.olderFailed ? "chat_older_failed" : "chat_load_older"),
                                        action: onLoadOlder
                                    )
                                    .font(.system(size: 13, design: .monospaced))
                                    .buttonStyle(.bordered)
                                    .disabled(chat.olderLoading)
                                } else {
                                    Text(L.t("chat_beginning"))
                                        .font(.system(size: 12, design: .monospaced))
                                        .foregroundStyle(Color(rgb: palette.dim))
                                }
                                Spacer()
                            }
                        }
                        if userOnly && userPrompts.isEmpty {
                            EmptyFilterNotice(palette: palette, onClear: { userOnly = false })
                        } else if userOnly {
                            ForEach(userPrompts) { message in
                                UserPromptFilterRow(message: message, palette: palette) {
                                    followBottom = false
                                    pendingJumpId = message.id
                                    userOnly = false
                                }
                                .id(message.id)
                            }
                        } else {
                            ForEach(chat.messages) { message in
                                MessageRow(
                                    message: message,
                                    palette: palette,
                                    highlighted: highlightedId == message.id
                                )
                                .id(message.id)
                            }
                        }
                        Color.clear
                            .frame(height: 1)
                            .id("end")
                            .onAppear {
                                guard !userOnly, pendingJumpId == nil, !jumping else { return }
                                followBottom = true
                            }
                            .onDisappear {
                                guard !userOnly else { return }
                                followBottom = false
                            }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .frame(maxWidth: 820, alignment: .leading)
                    .frame(maxWidth: .infinity)
                }
                .defaultScrollAnchor(.bottom)
                .onChange(of: chat.messages.last?.id) { _, _ in
                    if followBottom && !userOnly && chat.historyPage == 0 {
                        withAnimation { proxy.scrollTo("end", anchor: .bottom) }
                    }
                }
                .onChange(of: userOnly) { oldUserOnly, nowUserOnly in
                    guard oldUserOnly && !nowUserOnly else { return }
                    if pendingJumpId != nil {
                        performJumpIfNeeded(proxy: proxy)
                    } else if followBottom {
                        jumpTask?.cancel()
                        jumpTask = Task { @MainActor in
                            await Task.yield()
                            guard !Task.isCancelled, !userOnly, pendingJumpId == nil, !jumping else { return }
                            proxy.scrollTo("end", anchor: .bottom)
                        }
                    }
                }
                .onChange(of: pendingJumpId) { _, _ in
                    performJumpIfNeeded(proxy: proxy)
                }
                .onChange(of: chat.sessionId) { _, _ in
                    resetScrollState()
                }
                .onDisappear {
                    jumpTask?.cancel()
                    jumpTask = nil
                    highlightClearTask?.cancel()
                    highlightClearTask = nil
                    jumping = false
                }
            }
        }
    }

    private func performJumpIfNeeded(proxy: ScrollViewProxy) {
        guard !userOnly, let targetId = pendingJumpId else { return }
        pendingJumpId = nil
        followBottom = false
        jumping = true
        jumpTask?.cancel()
        jumpTask = Task { @MainActor in
            await Task.yield()
            guard !Task.isCancelled else {
                jumping = false
                return
            }
            withAnimation(.easeInOut(duration: 0.2)) {
                proxy.scrollTo(targetId, anchor: .center)
            }
            if chat.messages.contains(where: { $0.id == targetId }) {
                highlightedId = targetId
                highlightClearTask?.cancel()
                highlightClearTask = Task { @MainActor in
                    try? await Task.sleep(for: .milliseconds(1800))
                    guard !Task.isCancelled else { return }
                    if highlightedId == targetId {
                        highlightedId = nil
                    }
                }
            }
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else {
                jumping = false
                return
            }
            jumping = false
        }
    }

    private func resetScrollState() {
        jumpTask?.cancel()
        jumpTask = nil
        highlightClearTask?.cancel()
        highlightClearTask = nil
        followBottom = true
        pendingJumpId = nil
        highlightedId = nil
        jumping = false
    }

    /// `unsupported-agent` 带上是哪一家:「这个 AI 读不了」比「读不了」有用得多。
    private func noticeText(_ notice: ChatNotice) -> String {
        switch notice {
        case .noAgent: L.t("chat_fallback_no_agent")
        case let .server(reason, agentId):
            switch reason {
            case .disabled: L.t("chat_fallback_disabled")
            case .noTranscriptPath: L.t("chat_fallback_no_path")
            case .unsupportedAgent: L.t("chat_fallback_unsupported", agentId)
            case .unreadable: L.t("chat_fallback_unreadable")
            case .parseFailed: L.t("chat_fallback_parse_failed")
            }
        }
    }
}

private struct FilterStatusBanner: View {
    let count: Int
    let palette: UiPalette
    let onClear: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text(L.t("chat_filter_banner", count))
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.dim))
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button(L.t("chat_filter_clear"), action: onClear)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.accent))
                    .buttonStyle(.plain)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .frame(maxWidth: 820)
            .frame(maxWidth: .infinity)
            .background(Color(rgb: palette.panel))

            Rectangle()
                .fill(Color(rgb: palette.line))
                .frame(height: 1)
        }
    }
}

private struct EmptyFilterNotice: View {
    let palette: UiPalette
    let onClear: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            Text(L.t("chat_filter_empty"))
                .font(.system(size: 13.5, design: .monospaced))
                .foregroundStyle(Color(rgb: palette.dim))
                .multilineTextAlignment(.center)
            Button(L.t("chat_filter_clear"), action: onClear)
                .font(.system(size: 12.5, design: .monospaced))
                .buttonStyle(.bordered)
                .tint(Color(rgb: palette.accent))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 28)
        .frame(maxWidth: .infinity)
    }
}

private struct UserPromptFilterRow: View {
    let message: ChatMessage
    let palette: UiPalette
    let onTap: () -> Void

    var body: some View {
        let promptTexts = message.userPromptTextBlocks
        Button(action: onTap) {
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(promptTexts.enumerated()), id: \.offset) { _, text in
                        Text(text)
                            .font(.system(size: 13.5, design: .monospaced))
                            .foregroundStyle(Color(rgb: palette.foreground))
                            .lineSpacing(3)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 12, weight: .medium, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.dim))
                    .padding(.top, 2)
                    .accessibilityHidden(true)
            }
            .padding(.leading, 13)
            .padding(.trailing, 8)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, minHeight: 48, alignment: .leading)
            .background(
                Color(rgb: palette.panel).opacity(0.45),
                in: UnevenRoundedRectangle(
                    topLeadingRadius: 0,
                    bottomLeadingRadius: 0,
                    bottomTrailingRadius: 6,
                    topTrailingRadius: 6
                )
            )
            .overlay(alignment: .leading) {
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(Color(rgb: 0x4EA1FF))
                    .frame(width: 3)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

private struct MessageRow: View {
    let message: ChatMessage
    let palette: UiPalette
    let highlighted: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(message.blocks.enumerated()), id: \.offset) { _, block in
                BlockView(block: block, role: message.role, palette: palette)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.leading, 13)
        .padding(.vertical, 2)
        .padding(.trailing, 4)
        .background(
            highlighted ? Color(rgb: palette.accent).opacity(0.22) : Color.clear,
            in: UnevenRoundedRectangle(
                topLeadingRadius: 0,
                bottomLeadingRadius: 0,
                bottomTrailingRadius: 6,
                topTrailingRadius: 6
            )
        )
        .animation(.easeInOut(duration: highlighted ? 0.18 : 0.6), value: highlighted)
        .overlay(alignment: .leading) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(stripe)
                .frame(width: 3)
        }
        .opacity(message.role == .system ? 0.8 : 1)
    }

    private var stripe: Color {
        switch message.role {
        case .user: Color(rgb: 0x4EA1FF)
        case .assistant: Color(rgb: palette.dim).opacity(0.5)
        case .system: Color(rgb: 0x9A8CFF)
        }
    }
}

private struct BlockView: View {
    let block: ChatBlock
    let role: ChatRole
    let palette: UiPalette
    @State private var expanded = false

    var body: some View {
        switch block {
        case let .text(text):
            if role == .assistant {
                ChatMarkdownView(text: text, palette: palette)
            } else {
                Text(text)
                    .font(.system(size: 13.5, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.foreground))
                    .lineSpacing(3)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        case let .thinking(text):
            disclosure(L.t("chat_thinking"), preview: "") {
                Text(text)
                    .font(.system(size: 12.5, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.dim))
                    .textSelection(.enabled)
            }
            .opacity(0.85)
        case let .toolUse(name, input):
            disclosure(name, preview: excerpt(input)) { mono(input) }
        case let .toolResult(ok, text):
            disclosure(L.t("chat_tool_result"), preview: excerpt(text), error: !ok) { mono(text) }
                .padding(.horizontal, 8)
                .background(
                    ok ? Color(rgb: palette.panel2) : Color(rgb: palette.danger).opacity(0.15),
                    in: RoundedRectangle(cornerRadius: 6)
                )
        }
    }

    private func disclosure<Content: View>(
        _ label: String,
        preview: String,
        error: Bool = false,
        @ViewBuilder content: () -> Content
    ) -> some View {
        let body = content()
        return DisclosureGroup(isExpanded: $expanded) {
            body.frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
        } label: {
            HStack(spacing: 8) {
                Text(label)
                    .font(.system(size: 12.5, weight: .semibold, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.foreground))
                if error {
                    Text(L.t("chat_tool_result_error"))
                        .font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(Color(rgb: palette.danger))
                }
                Text(preview)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.dim))
                    .lineLimit(1)
            }
        }
        .tint(Color(rgb: palette.dim))
    }

    private func mono(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12.5, design: .monospaced))
            .foregroundStyle(Color(rgb: palette.foreground))
            .textSelection(.enabled)
    }

    private func excerpt(_ text: String) -> String {
        String(text.split(separator: "\n").first { !$0.trimmingCharacters(in: .whitespaces).isEmpty }?
            .trimmingCharacters(in: .whitespaces).prefix(160) ?? "")
    }
}
