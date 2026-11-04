import Foundation

/// 聊天视图的数据层。消息来自 agent 自己写的对话记录,走 HTTP 拉取;增量由 `agent-event` 触发,不轮询。
/// 与 Android `Chat.kt`、web `chat.ts` 一一对应。
enum ChatRole { case user, assistant, system }

enum ChatBlock: Equatable {
    case text(String)
    case thinking(String)
    /// `input` 是服务端给的「给人看的一行」,不是原始 JSON
    case toolUse(name: String, input: String)
    case toolResult(ok: Bool, text: String)
}

struct ChatMessage: Identifiable, Equatable {
    let id: String
    let role: ChatRole
    let ts: Int64?
    let blocks: [ChatBlock]
}

/// 读不到对话记录时的原因。服务端给的是机器可读的键,文案在 UI 层按语言渲染。
enum ChatFallback: String {
    case disabled
    case noTranscriptPath = "no-transcript-path"
    case unsupportedAgent = "unsupported-agent"
    case unreadable
    case parseFailed = "parse-failed"

    /// 认不出的原因也要说人话 —— 归到「读不到」
    static func parse(_ raw: String) -> ChatFallback { ChatFallback(rawValue: raw) ?? .unreadable }
}

enum ChatWindow: Equatable {
    /// `cursor` 是不透明的:原样带回,**不解析**(协议 A9)
    case transcript(messages: [ChatMessage], cursor: String, reset: Bool, more: Bool, olderCursor: String?, historySupported: Bool)
    case terminal(ChatFallback)
}

enum Chat {
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

    /// `reset` 是整段替换;追加时按 id 去重(窗口边界上的那条可能被重复送来),同一批里也去重。
    static func merge(_ prev: [ChatMessage], _ win: ChatWindow, prepend: Bool = false) -> [ChatMessage] {
        guard case let .transcript(messages, _, reset, _, _, _) = win else { return prev }
        let base = reset ? [] : prev
        var seen = Set(base.map(\.id))
        let incoming = messages.filter { seen.insert($0.id).inserted }
        return prepend ? incoming + base : base + incoming
    }

    /// 老版本服务端没有这个路由:降级成「读不到」,不是报错(协议 T12)。
    static func fetch(_ http: ServerHTTP, agentId: String, agentSessionId: String, after: String?, before: String? = nil) async -> ChatWindow {
        var query = [("agent_id", agentId), ("agent_session_id", agentSessionId)]
        if let after { query.append(("after", after)) }
        if let before { query.append(("before", before)) }
        guard let (status, body) = try? await http.request("/api/agents/messages", query: query), (200..<300).contains(status) else {
            return .terminal(.unreadable)
        }
        return parseWindow(body)
    }
}
