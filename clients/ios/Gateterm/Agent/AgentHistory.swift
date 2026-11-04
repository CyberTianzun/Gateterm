import Foundation

struct HistoryEntry: Identifiable, Equatable, Hashable {
    let agentId: String
    let agentName: String
    let sessionId: String
    let title: String
    let cwd: String?
    let updatedAt: Int64
    let terminalSession: Int64?
    let canResume: Bool
    let unavailableReason: String?
    var description: String? = nil
    var model: String? = nil

    var id: String { "\(agentId):\(sessionId)" }
}

struct HistoryAgent: Hashable { let id: String; let name: String }

struct HistoryPage {
    var sessions: [HistoryEntry]
    var agents: [HistoryAgent]
    var total: Int
    var nextOffset: Int?
    var truncated: Bool
    var skipped: Int
    var indexing = false
    var maxSessions: Int? = nil
    var workspaces: [String] = []
}

struct HistoryFailure: Error { let reason: String }

enum AgentHistory {
    static func parsePage(_ data: Data) throws -> HistoryPage {
        guard let root = JSON(data: data), root.array("sessions") != nil else { throw HistoryFailure(reason: "unreadable") }
        var seen = Set<String>()
        let sessions: [HistoryEntry] = root.objects("sessions").compactMap { row in
            let entry = HistoryEntry(
                agentId: row.string("agent_id"), agentName: row.string("agent_name"), sessionId: row.string("agent_session_id"),
                title: row.string("title"), cwd: row.nullableString("cwd"), updatedAt: row.int64("updated_at"),
                terminalSession: row.nullableInt64("terminal_session"), canResume: row.isTrue("can_resume"),
                unavailableReason: row.nullableString("unavailable_reason"), description: row.nullableString("description"),
                model: row.nullableString("model"))
            return seen.insert(entry.id).inserted ? entry : nil
        }
        return HistoryPage(
            sessions: sessions,
            agents: root.objects("agents").map { HistoryAgent(id: $0.string("id"), name: $0.string("name")) },
            total: root.int("total"), nextOffset: root.nullableInt("next_offset"), truncated: root.bool("truncated"),
            skipped: root.int("skipped"), indexing: root.bool("indexing"), maxSessions: root.nullableInt("max_sessions"),
            workspaces: (root.array("workspaces") ?? []).compactMap { ($0 as? String).flatMap { $0.isEmpty ? nil : $0 } })
    }

    private static func call(_ http: ServerHTTP, _ path: String, query: [(String, String)] = [], body: Data? = nil) async throws -> Data {
        let result: (Int, Data)
        do {
            result = try await http.request(path, query: query, method: body == nil ? "GET" : "POST", body: body)
        } catch { throw HistoryFailure(reason: "unreadable") }
        guard (200..<300).contains(result.0) else {
            let reason = JSON(data: result.1)?.nullableString("error") ?? "unreadable"
            throw HistoryFailure(reason: reason)
        }
        return result.1
    }

    static func fetch(_ http: ServerHTTP, query: String, agent: String, offset: Int, refresh: Bool, workspace: String) async throws -> HistoryPage {
        try parsePage(try await call(http, "/api/agents/history", query: [
            ("q", query), ("agent_id", agent), ("offset", String(offset)), ("refresh", String(refresh)), ("workspace", workspace),
        ]))
    }

    static func messages(_ http: ServerHTTP, _ entry: HistoryEntry) async throws -> [ChatMessage] {
        let window = Chat.parseWindow(try await call(http, "/api/agents/history/messages",
                                                     query: [("agent_id", entry.agentId), ("agent_session_id", entry.sessionId)]))
        guard case let .transcript(messages, _, _, _, _, _) = window else { throw HistoryFailure(reason: "unreadable") }
        return messages
    }

    static func open(_ http: ServerHTTP, _ entry: HistoryEntry) async throws -> SessionInfo {
        let body = try JSONSerialization.data(withJSONObject: ["agent_id": entry.agentId, "agent_session_id": entry.sessionId])
        guard let row = JSON(data: try await call(http, "/api/agents/history/open", body: body))?.object("session"),
              let session = ControlJson.parseSession(row) else { throw HistoryFailure(reason: "unreadable") }
        return session
    }
}

/// 新建会话菜单里的一条启动项。清单来自 `GET /api/launchers`,客户端只显示,并把选中的 argv 原样发回去。
struct Launcher: Identifiable, Equatable, Hashable {
    let id: String
    let provider: String
    let name: String
    let detail: String
    let command: [String]
    let cwd: String?

    static let fallback = Launcher(id: "fallback:default", provider: "builtin", name: "", detail: "", command: [], cwd: nil)
}

enum Launchers {
    /// 结构不完整的条目丢掉。空 argv 也丢掉 —— 那是拉取失败时兜底项的约定,不来自服务端。
    static func sanitize(_ data: Data) -> [Launcher] {
        guard let root = JSON(data: data) else { return [] }
        return root.objects("launchers").compactMap { item in
            let id = item.string("id"), name = item.string("name")
            guard !id.isEmpty, !name.isEmpty, let raw = item.array("command") else { return nil }
            let command = raw.compactMap { $0 as? String }
            guard command.count == raw.count, !command.isEmpty else { return nil }
            let detail = item.string("detail")
            let provider = item.string("provider")
            let cwd = item.string("cwd")
            return Launcher(id: id, provider: provider.isEmpty ? "unknown" : provider, name: name,
                            detail: detail.isEmpty ? command.joined(separator: " ") : detail, command: command,
                            cwd: cwd.isEmpty ? nil : cwd)
        }
    }

    static func fetch(_ http: ServerHTTP) async -> [Launcher] {
        guard let (status, body) = try? await http.request("/api/launchers"), (200..<300).contains(status) else { return [.fallback] }
        let list = sanitize(body)
        return list.isEmpty ? [.fallback] : list
    }
}
