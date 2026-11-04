import SwiftUI

/// Agent chat view: same presentation as web/Android — a role stripe per message, thinking and
/// tool blocks collapsed into disclosure groups. When the transcript cannot be read, say why and
/// offer the terminal instead (protocol A9).
struct ChatView: View {
    let chat: ChatUiState
    var onOpenTerminal: () -> Void
    var onLoadOlder: () -> Void = {}

    var body: some View {
        if let notice = chat.notice {
            VStack(alignment: .leading, spacing: 16) {
                Text(noticeText(notice)).font(.body)
                Button(L.t("chat_open_terminal_instead"), action: onOpenTerminal).buttonStyle(.bordered)
                Spacer()
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if chat.messages.isEmpty && chat.loading {
            ContentUnavailableView { ProgressView() } description: { Text(L.t("chat_loading")) }
        } else if chat.messages.isEmpty && chat.olderCursor == nil {
            ContentUnavailableView(L.t("chat_empty"), systemImage: "bubble.left.and.bubble.right")
        } else {
            messages
        }
    }

    private var messages: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if chat.historySupported {
                        HStack {
                            Spacer()
                            if chat.olderCursor != nil {
                                Button(L.t(chat.olderLoading ? "chat_loading_older" : chat.olderFailed ? "chat_older_failed" : "chat_load_older"),
                                       action: onLoadOlder)
                                    .buttonStyle(.bordered)
                                    .disabled(chat.olderLoading)
                            } else {
                                Text(L.t("chat_beginning")).font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                        }
                    }
                    ForEach(chat.messages) { MessageRow(message: $0).id($0.id) }
                    Color.clear.frame(height: 1).id("end")
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: 820, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .defaultScrollAnchor(.bottom)
            .onChange(of: chat.messages.last?.id) { _, _ in
                if chat.historyPage == 0 { withAnimation { proxy.scrollTo("end", anchor: .bottom) } }
            }
        }
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

private struct MessageRow: View {
    let message: ChatMessage

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(stripe)
                .frame(width: 3)
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(message.blocks.enumerated()), id: \.offset) { _, block in BlockView(block: block) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .fixedSize(horizontal: false, vertical: true)
        .opacity(message.role == .system ? 0.8 : 1)
    }

    private var stripe: Color {
        switch message.role {
        case .user: Color(rgb: 0x4EA1FF)
        case .assistant: Color.secondary.opacity(0.5)
        case .system: Color(rgb: 0x9A8CFF)
        }
    }
}

private struct BlockView: View {
    let block: ChatBlock
    @State private var expanded = false

    var body: some View {
        switch block {
        case let .text(text):
            Text(Self.markdown(text)).font(.body).textSelection(.enabled)
        case let .thinking(text):
            disclosure(L.t("chat_thinking"), preview: "") {
                Text(text).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
            }
            .opacity(0.85)
        case let .toolUse(name, input):
            disclosure(name, preview: excerpt(input)) { mono(input) }
        case let .toolResult(ok, text):
            disclosure(L.t("chat_tool_result"), preview: excerpt(text), error: !ok) { mono(text) }
                .padding(.horizontal, 8)
                .background(ok ? Color(uiColor: .secondarySystemFill) : Color.red.opacity(0.15), in: RoundedRectangle(cornerRadius: 6))
        }
    }

    private func disclosure<Content: View>(_ label: String, preview: String, error: Bool = false,
                                           @ViewBuilder content: () -> Content) -> some View {
        let body = content()
        return DisclosureGroup(isExpanded: $expanded) {
            body.frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
        } label: {
            HStack(spacing: 8) {
                Text(label).font(.footnote.weight(.semibold))
                if error { Text(L.t("chat_tool_result_error")).font(.caption).foregroundStyle(.red) }
                Text(preview).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .tint(.secondary)
    }

    /// Inline Markdown (code, emphasis, links) while keeping the agent's line breaks.
    private static func markdown(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace,
                                                              failurePolicy: .returnPartiallyParsedIfPossible)))
            ?? AttributedString(text)
    }

    private func mono(_ text: String) -> some View {
        Text(text).font(.caption.monospaced()).textSelection(.enabled)
    }

    private func excerpt(_ text: String) -> String {
        String(text.split(separator: "\n").first { !$0.trimmingCharacters(in: .whitespaces).isEmpty }?
            .trimmingCharacters(in: .whitespaces).prefix(160) ?? "")
    }
}
