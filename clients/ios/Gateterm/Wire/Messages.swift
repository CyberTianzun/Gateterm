import Foundation

struct SessionInfo: Identifiable, Equatable, Hashable {
    let id: Int64
    var name: String
    var cols: Int
    var rows: Int
    var createdAt: Int64
    var cwd: String? = nil
}

enum AttachMode { case replay, resync }

enum SessionEventKind { case created, renamed, closed }

/// 本端画会话用的默认前景/背景色(`#rrggbb`),服务端拿它应答程序的 OSC 10/11 颜色查询。
/// 终端查询只由服务端应答,客户端不答:绕一圈网络的应答常常迟到,会落进 shell 成为乱码。
struct TermColors: Equatable {
    let fg: String
    let bg: String

    static func fromRGB(fg: Int, bg: Int) -> TermColors {
        TermColors(fg: String(format: "#%06x", fg & 0xFFFFFF), bg: String(format: "#%06x", bg & 0xFFFFFF))
    }
}

enum ClientMsg: Equatable {
    case hello(token: String, proto: Int = ControlJson.protoVersion)
    case listSessions
    case newSession(name: String?, command: [String]?, cwd: String?)
    case killSession(id: Int64)
    case renameSession(id: Int64, name: String)
    case attach(id: Int64, channel: UInt32, lastSeq: Int64?, cols: Int, rows: Int, colors: TermColors?)
    case detach(channel: UInt32)
    case resize(channel: UInt32, cols: Int, rows: Int, colors: TermColors?)
}

enum ServerMsg {
    case helloOk(proto: Int, os: String, cloudServerId: String?, win32InputMode: Bool)
    case error(message: String, channel: UInt32?)
    case sessions([SessionInfo])
    case sessionEvent(SessionEventKind, SessionInfo)
    case attached(channel: UInt32, seq: Int64, mode: AttachMode)
    case resized(channel: UInt32, cols: Int, rows: Int)
    case resyncBegin(channel: UInt32)
    case resyncEnd(channel: UInt32, seq: Int64)
    case sessionExited(channel: UInt32, id: Int64)
    /// 托管会话里的 coding agent 状态变了。不带 `supports_transcript`,见 `AgentSessions.applyEvent`
    case agentEvent(AgentSession)
    case unknown(String)
}
