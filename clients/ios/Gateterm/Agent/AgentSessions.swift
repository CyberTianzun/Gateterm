import Foundation

/// 托管会话里跑着的 coding agent。增量来自 `agent-event`,全量来自 `GET /api/agents/sessions`
/// —— 连上时和每次重连后都要拉一次全量(协议 A5)。与 Android `AgentSessions.kt`、web `agents.ts` 一一对应。
enum AgentState: String, CaseIterable {
    // `waiting` 永远排第一 —— 它是唯一需要用户**动手**的状态
    case waiting, error, working, thinking, idle, done

    var priority: Int {
        switch self {
        case .waiting: 5
        case .error: 4
        case .working: 3
        case .thinking: 2
        case .idle: 1
        case .done: 0
        }
    }

    /// 认不出的状态当 idle:新版服务端加了状态不该让旧客户端把会话丢掉
    static func parse(_ raw: String) -> AgentState { AgentState(rawValue: raw) ?? .idle }
}

struct AgentSession: Equatable, Hashable {
    let agentId: String
    let agentSessionId: String
    /// 关联到的 gateterm 托管会话 id
    var session: Int64?
    var state: AgentState
    var detail: String?
    /// 有值 = 正在等人回答
    var pending: String?
    /// 这家 agent 的对话记录能不能读。`agent-event` 不带它,只有 HTTP 快照给 —— 所以可为 nil
    var supportsTranscript: Bool?

    var key: String { "\(agentId) \(agentSessionId)" }
}

struct AgentSnapshot {
    let sessions: [AgentSession]
    let transcriptEnabled: Bool
    static let unavailable = AgentSnapshot(sessions: [], transcriptEnabled: false)
}

enum AgentSessions {
    /// `done` 的直接移除;事件不携带 `supports_transcript`,保留快照已经给出的值。
    static func applyEvent(_ map: [String: AgentSession], _ ev: AgentSession) -> [String: AgentSession] {
        var next = map
        if ev.state == .done {
            next.removeValue(forKey: ev.key)
        } else {
            var merged = ev
            merged.supportsTranscript = ev.supportsTranscript ?? map[ev.key]?.supportsTranscript
            next[ev.key] = merged
        }
        return next
    }

    static func replaceAll(_ list: [AgentSession]) -> [String: AgentSession] {
        var next: [String: AgentSession] = [:]
        for s in list where s.state != .done { next[s.key] = s }
        return next
    }

    /// 这个托管会话该显示哪个 agent 的状态:状态优先级最高的那个。
    static func forSession(_ map: [String: AgentSession], _ sessionId: Int64) -> AgentSession? {
        map.values.filter { $0.session == sessionId }.max { $0.state.priority < $1.state.priority }
    }

    /// 聊天视图该看哪个 agent:读得了记录的优先;都读不了才回退到状态最高的那个。
    static func chatAgentFor(_ map: [String: AgentSession], _ sessionId: Int64) -> AgentSession? {
        map.values.filter { $0.session == sessionId && $0.supportsTranscript == true }
            .max { $0.state.priority < $1.state.priority } ?? forSession(map, sessionId)
    }

    /// 聊天入口既要本会话里有读得了记录的 agent,也要服务端明确允许读。
    static func chatAvailable(_ map: [String: AgentSession], _ sessionId: Int64?, transcriptEnabled: Bool) -> Bool {
        guard transcriptEnabled, let sessionId else { return false }
        return chatAgentFor(map, sessionId)?.supportsTranscript == true
    }

    /// 协议里的一条 agent 会话(快照条目与 `agent-event` 同形)。缺关键字段就丢掉。
    static func parseSession(_ obj: JSON) -> AgentSession? {
        let agentId = obj.string("agent_id"), agentSessionId = obj.string("agent_session_id")
        guard !agentId.isEmpty, !agentSessionId.isEmpty else { return nil }
        return AgentSession(
            agentId: agentId, agentSessionId: agentSessionId, session: obj.nullableInt64("session"),
            state: .parse(obj.string("state")), detail: obj.nullableString("detail"), pending: obj.nullableString("pending"),
            supportsTranscript: obj.has("supports_transcript") ? obj.bool("supports_transcript") : nil
        )
    }

    /// 坏响应、旧服务端一律当「没有会话、不许读」—— 聊天入口隐藏,而不是卡住。
    static func parseSnapshot(_ data: Data) -> AgentSnapshot {
        guard let root = JSON(data: data) else { return .unavailable }
        // 只认明确的 true:旧服务端没有这个字段
        return AgentSnapshot(sessions: root.objects("sessions").compactMap(parseSession), transcriptEnabled: root.isTrue("transcript_enabled"))
    }

    /// 协议 T12:路由不存在或失败都降级,不报错。
    static func fetch(_ http: ServerHTTP) async -> AgentSnapshot {
        guard let (status, body) = try? await http.request("/api/agents/sessions"), (200..<300).contains(status) else {
            return .unavailable
        }
        return parseSnapshot(body)
    }
}

/// Counts each server separately: identical agent/session keys can exist on different hosts.
func connectedAgentCounts(_ servers: [(ServerConnectionStatus, [AgentSession])]) -> [AgentState: Int] {
    var counts: [AgentState: Int] = [:]
    for (status, agents) in servers where status == .connected {
        for agent in agents where agent.state != .done { counts[agent.state, default: 0] += 1 }
    }
    return counts
}
