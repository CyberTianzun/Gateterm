import Foundation
import UserNotifications

/// Cloud push payload → structured target. Only routing data; wording is generated locally.
struct AgentPush: Equatable {
    enum Kind: String { case waiting, permission, done, resolved }

    let serverId: String
    let session: Int64
    let agentId: String
    let kind: Kind
    let toolName: String?
    let expiresAt: Int64?

    /// 同一会话里同一个 agent 只占一条通知;带上 agent_id,两个 agent 互不误伤。
    var tag: String { Self.tag(serverId: serverId, session: session, agentId: agentId) }

    static func tag(serverId: String, session: Int64, agentId: String) -> String { "\(serverId):\(session):\(agentId)" }

    static func parse(_ data: [AnyHashable: Any]) -> AgentPush? {
        func string(_ key: String) -> String? {
            if let s = data[key] as? String { return s }
            if let n = data[key] as? NSNumber { return n.stringValue }
            return nil
        }
        guard let serverId = string("server_id"), !serverId.isEmpty,
              let session = string("session").flatMap(Int64.init),
              let agentId = string("agent_id"), !agentId.isEmpty,
              let kind = string("kind").flatMap(Kind.init(rawValue:)) else { return nil }
        return AgentPush(serverId: serverId, session: session, agentId: agentId, kind: kind,
                         toolName: string("tool_name"), expiresAt: string("expires_at").flatMap(Int64.init))
    }
}

func agentDisplayName(_ agentId: String) -> String {
    switch agentId {
    case "claude-code": "Claude Code"
    case "codex": "Codex"
    case "grok": "Grok"
    case "pi": "Pi"
    case "antigravity": "Antigravity"
    default: agentId
    }
}

enum AgentNotifications {
    /// Removes delivered alerts for one agent once its session has been opened.
    static func remove(tag: String) {
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { delivered in
            let ids = delivered.filter { note in
                note.request.identifier == tag || AgentPush.parse(note.request.content.userInfo)?.tag == tag
            }.map(\.request.identifier)
            center.removeDeliveredNotifications(withIdentifiers: ids)
        }
    }
}
