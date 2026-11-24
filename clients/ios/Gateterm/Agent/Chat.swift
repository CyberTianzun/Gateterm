import Foundation

/// 聊天视图的数据层。消息来自 agent 自己写的对话记录,走 HTTP 拉取;增量由 `agent-event` 触发,不轮询。
/// 与 Android `Chat.kt`、web `chat.ts` 一一对应。
enum ChatRole: Sendable { case user, assistant, system }

enum ChatBlock: Equatable, Sendable {
    case text(String)
    case thinking(String)
    /// `input` 是服务端给的「给人看的一行」,不是原始 JSON
    case toolUse(name: String, input: String)
    case toolResult(ok: Bool, text: String)
}

struct ChatMessage: Identifiable, Equatable, Sendable {
    let id: String
    let role: ChatRole
    let ts: Int64?
    let blocks: [ChatBlock]
}

extension ChatMessage {
    /// 仅提取用户主动发出的非空正文块，剔除同条里的 tool-result、空白或框架注入上下文。
    var userPromptTextBlocks: [String] {
        guard role == .user else { return [] }
        return blocks.compactMap { block in
            guard case let .text(text) = block, !Chat.isHarnessUserText(text) else { return nil }
            return text
        }
    }

    /// 判断一条消息是否为用户真正输入的一轮提示（而非工具回传/空白占位的 user turn）。
    /// 不少 agent 的记录会把 tool-result 甚至 Pi 的 [tool-use, tool-result] 记成 role="user"。
    var isUserPrompt: Bool { !userPromptTextBlocks.isEmpty }
}

/// 读不到对话记录时的原因。服务端给的是机器可读的键,文案在 UI 层按语言渲染。
enum ChatFallback: String, Sendable {
    case disabled
    case noTranscriptPath = "no-transcript-path"
    case unsupportedAgent = "unsupported-agent"
    case unreadable
    case parseFailed = "parse-failed"

    /// 认不出的原因也要说人话 —— 归到「读不到」
    static func parse(_ raw: String) -> ChatFallback { ChatFallback(rawValue: raw) ?? .unreadable }
}

enum ChatWindow: Equatable, Sendable {
    /// `cursor` 是不透明的:原样带回,**不解析**(协议 A9)
    case transcript(messages: [ChatMessage], cursor: String, reset: Bool, more: Bool, olderCursor: String?, historySupported: Bool)
    case terminal(ChatFallback)
}

enum OlderLoadStop: Equatable, Sendable {
    case foundUserPrompt
    case beginning
    case pageLimit
    case reset
    case unsupportedOldServer
    case failed
    case cancelled
}

struct OlderLoadResult: Equatable, Sendable {
    let stop: OlderLoadStop
    let pagesFetched: Int
}

enum Chat {
    static let maxAutoOlderPages = 16

    private static let harnessUserPrefixes = [
        "# AGENTS.md instructions",
        "<environment_context>",
        "<permissions instructions>",
        "<developer_instructions>",
        "<system-reminder>",
        "<local-command-caveat>",
        "<command-name>",
        "This session is being continued from a previous conversation",
    ]
    private static let codexRequestMarker = "## My request for Codex:"

    /// 判断 role="user" 的文本块是否为空白或框架注入上下文（对齐服务端 history::prompt_body）。
    static func isHarnessUserText(_ text: String) -> Bool {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return true }
        if harnessUserPrefixes.contains(where: { trimmed.hasPrefix($0) }) { return true }
        if let range = trimmed.range(of: codexRequestMarker),
           trimmed[range.upperBound...].trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return true
        }
        return false
    }

    static func parseWindow(_ data: Data) -> ChatWindow {
        guard let root = JSON(data: data) else { return .terminal(.unreadable) }
        guard root.string("source") == "transcript" else { return .terminal(.parse(root.string("reason"))) }
        let older = root.nullableString("older_cursor").flatMap { $0.isEmpty ? nil : $0 }
        return .transcript(
            messages: root.objects("messages").compactMap(parseMessage), cursor: root.string("cursor"),
            reset: root.bool("reset"), more: root.bool("more"), olderCursor: older,
            historySupported: root.raw["older_cursor"] != nil
        )
    }

    private static func parseMessage(_ obj: JSON) -> ChatMessage? {
        let id = obj.string("id")
        guard !id.isEmpty else { return nil }
        let role: ChatRole = switch obj.string("role") {
        case "user": .user
        case "assistant": .assistant
        default: .system
        }
        // 认不出的块跳过:新版服务端加了块类型,旧客户端照样能看其余部分
        let blocks: [ChatBlock] = obj.objects("blocks").compactMap { b in
            switch b.string("kind") {
            case "text": .text(b.string("text"))
            case "thinking": .thinking(b.string("text"))
            case "tool-use": .toolUse(name: b.string("name"), input: b.string("input"))
            case "tool-result": .toolResult(ok: b.bool("ok", default: true), text: b.string("text"))
            default: nil
            }
        }
        return ChatMessage(id: id, role: role, ts: obj.nullableInt64("ts"), blocks: blocks)
    }

    private static func uniqueIncoming(_ base: [ChatMessage], _ messages: [ChatMessage]) -> [ChatMessage] {
        var seen = Set(base.map(\.id))
        return messages.filter { seen.insert($0.id).inserted }
    }

    /// `reset` 是整段替换;追加时按 id 去重(窗口边界上的那条可能被重复送来),同一批里也去重。
    static func merge(_ prev: [ChatMessage], _ win: ChatWindow, prepend: Bool = false) -> [ChatMessage] {
        guard case let .transcript(messages, _, reset, _, _, _) = win else { return prev }
        let base = reset ? [] : prev
        let incoming = uniqueIncoming(base, messages)
        return prepend ? incoming + base : base + incoming
    }

    /// 计算本次向后翻页真正新增（未与已有消息重复）的前置消息集合。
    static func newlyPrepended(_ prev: [ChatMessage], _ win: ChatWindow) -> [ChatMessage] {
        guard case let .transcript(messages, _, reset, _, _, _) = win else { return [] }
        let base = reset ? [] : prev
        return uniqueIncoming(base, messages)
    }

    /// 老版本服务端没有这个路由:降级成「读不到」,不是报错(协议 T12)。
    static func fetch(_ http: ServerHTTP, agentId: String, agentSessionId: String, after: String?, before: String? = nil) async -> ChatWindow {
        var query = [("agent_id", agentId), ("agent_session_id", agentSessionId)]
        if let after { query.append(("after", after)) }
        if let before, !before.isEmpty { query.append(("before", before)) }
        guard let (status, body) = try? await http.request("/api/agents/messages", query: query), (200..<300).contains(status) else {
            return .terminal(.unreadable)
        }
        return parseWindow(body)
    }

    /// 筛出所有用户输入的消息并保持原始时间顺序。
    static func userPrompts(_ messages: [ChatMessage]) -> [ChatMessage] {
        messages.filter(\.isUserPrompt)
    }

    /// 判断消息列表中是否已包含至少一条用户主动提问。
    static func hasUserPrompt(_ messages: [ChatMessage]) -> Bool {
        messages.contains(where: \.isUserPrompt)
    }

    /// 自动连续向后拉取更早消息，直到新前置的消息中包含用户主动提问、到达开头、
    /// 触及单次上限（16 页）、遇到 reset、失败或被取消为止。
    @MainActor
    static func loadOlderUntilUserPrompt(
        initialCursor: String?,
        maxPages: Int = maxAutoOlderPages,
        isHistoryPreview: Bool = false,
        isCancelled: @MainActor () -> Bool,
        fetchPage: @MainActor (String) async -> ChatWindow?,
        getMessages: @MainActor () -> [ChatMessage],
        onPage: @MainActor (ChatWindow, [ChatMessage]) -> Void,
        onReset: @MainActor (ChatWindow) -> Void
    ) async -> OlderLoadResult {
        var cursor = initialCursor
        var pagesFetched = 0
        while true {
            if Task.isCancelled || isCancelled() {
                return OlderLoadResult(stop: .cancelled, pagesFetched: pagesFetched)
            }
            guard let activeCursor = cursor, !activeCursor.isEmpty else {
                return OlderLoadResult(stop: .beginning, pagesFetched: pagesFetched)
            }
            if pagesFetched >= maxPages {
                return OlderLoadResult(stop: .pageLimit, pagesFetched: pagesFetched)
            }

            let win = await fetchPage(activeCursor)
            if Task.isCancelled || isCancelled() {
                return OlderLoadResult(stop: .cancelled, pagesFetched: pagesFetched)
            }
            guard let win, case let .transcript(_, _, reset, _, olderCursor, _) = win else {
                return OlderLoadResult(stop: .failed, pagesFetched: pagesFetched)
            }
            if reset {
                if olderCursor == activeCursor {
                    return OlderLoadResult(
                        stop: isHistoryPreview ? .unsupportedOldServer : .failed,
                        pagesFetched: pagesFetched
                    )
                }
                onReset(win)
                return OlderLoadResult(stop: .reset, pagesFetched: pagesFetched)
            }
            if olderCursor == activeCursor {
                return OlderLoadResult(stop: .failed, pagesFetched: pagesFetched)
            }

            let incoming = newlyPrepended(getMessages(), win)
            pagesFetched += 1
            onPage(win, incoming)
            if hasUserPrompt(incoming) {
                return OlderLoadResult(stop: .foundUserPrompt, pagesFetched: pagesFetched)
            }
            cursor = olderCursor
        }
    }
}

/// Allow only http:, https:, and mailto: URLs without ASCII control characters.
func sanitizeMarkdownLinkUrl(_ raw: String?) -> URL? {
    guard let raw else { return nil }
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return nil }
    guard !trimmed.unicodeScalars.contains(where: { $0.value <= 0x1F || $0.value == 0x7F }) else {
        return nil
    }
    let lower = trimmed.lowercased()
    guard let prefix = ["https://", "http://", "mailto:"].first(where: { lower.hasPrefix($0) }),
          trimmed.count > prefix.count,
          let url = URL(string: trimmed),
          let scheme = url.scheme?.lowercased(),
          ["http", "https", "mailto"].contains(scheme) else {
        return nil
    }
    return url
}

func markdownHeadingFontSize(_ level: Int) -> Double {
    switch level {
    case 1: 15.5
    case 2: 14.5
    default: 13.5
    }
}

/// Iteratively flattens nested blockquotes and lists into leaf blocks (propagating task
/// strikethrough) so deep markdown nesting never overflows the view stack or drops content.
func flattenDeepMarkdownBlocks(
    indices: [UInt32],
    in doc: FfiMdDocument,
    inheritedStrikeThrough: Bool = false
) -> [(blockIndex: Int, strikeThrough: Bool)] {
    var result: [(blockIndex: Int, strikeThrough: Bool)] = []
    var stack: [(blockIndex: Int, strikeThrough: Bool)] = indices.reversed().map {
        (Int($0), inheritedStrikeThrough)
    }
    while let (blockIndex, strikeThrough) = stack.popLast() {
        guard doc.blocks.indices.contains(blockIndex) else { continue }
        switch doc.blocks[blockIndex] {
        case let .blockQuote(children):
            for child in children.reversed() {
                stack.append((Int(child), strikeThrough))
            }
        case let .listBlock(_, _, items):
            for item in items.reversed() {
                let childStrike = strikeThrough || (item.checked == true)
                for child in item.children.reversed() {
                    stack.append((Int(child), childStrike))
                }
            }
        case .heading, .paragraph, .codeBlock, .table, .thematicBreak:
            result.append((blockIndex: blockIndex, strikeThrough: strikeThrough))
        }
    }
    return result
}
