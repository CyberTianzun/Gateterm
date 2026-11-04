import Foundation

struct EditorState: Equatable {
    /// nil = adding a new server.
    var connectionId: String?
    var name = ""
    var url = ""
    var token = ""
    var urlError: EditorFieldError?
    var tokenError: EditorFieldError?
    var loopbackWarning = false
    var wildcardWarning = false
    var clipboardURL: String?
    /// Open the QR scanner as soon as the editor appears ("Scan to add").
    var scanOnOpen = false
}

enum EditorFieldError { case emptyURL, badURL, emptyToken }

enum UserMessage: Equatable {
    case saved, deleted, authFailed(String?), unreachable, timeout, unexpected, copied, importFailed
    case tooManySessions, connectServerFirst, sessionGone

    var text: String {
        switch self {
        case .saved: L.t("msg_saved")
        case .deleted: L.t("msg_deleted")
        case let .authFailed(detail):
            (detail?.trimmingCharacters(in: .whitespaces)).flatMap { $0.isEmpty ? nil : $0 }
                .map { L.t("msg_auth_failed_detail", $0) } ?? L.t("msg_auth_failed")
        case .unreachable: L.t("msg_unreachable")
        case .timeout: L.t("msg_timeout")
        case .unexpected: L.t("msg_unexpected")
        case .copied: L.t("msg_copied")
        case .importFailed: L.t("msg_import_failed")
        case .tooManySessions: L.t("msg_too_many_sessions")
        case .connectServerFirst: L.t("msg_connect_server_first")
        case .sessionGone: L.t("msg_session_gone")
        }
    }

    var isError: Bool {
        switch self {
        case .saved, .deleted, .copied: false
        default: true
        }
    }
}

struct LauncherMenuState: Equatable {
    var loading = false
    var items: [Launcher] = []
}

/// 聊天视图里没有消息可看时,说明为什么。
enum ChatNotice: Equatable {
    /// 这个会话里没认出 agent
    case noAgent
    /// 服务端说读不了,带上是哪一家,好让文案说清楚
    case server(ChatFallback, agentId: String)
}

/// `sessionId` 记的是为哪个会话打开的 —— 只有它等于当前会话时才显示。
struct ChatUiState: Equatable {
    var sessionId: Int64?
    var messages: [ChatMessage] = []
    var notice: ChatNotice?
    var loading = false
    var olderCursor: String?
    var olderLoading = false
    var olderFailed = false
    var historySupported = false
    var historyPage = 0
}

/// 点通知要去的地方:本地连接 id、会话 id,以及打开后要撤回的那条通知的标识。
struct PendingOpen: Equatable {
    let connectionId: String
    let session: Int64
    let notificationTag: String?
}

enum PendingOpenDecision: Equatable {
    case open(Int64, notificationTag: String?)
    case sessionGone
}

/// 点通知跳转的生命周期。跳转只属于为它发起的那次连接:连接失败/断开/改连别的服务器都作废。
struct PendingOpenTracker {
    private(set) var current: PendingOpen?

    mutating func onTap(_ target: PendingOpen) { current = target }
    mutating func onConnectStart(_ connectionId: String) { if current?.connectionId != connectionId { current = nil } }
    mutating func onConnectFailed() { current = nil }
    mutating func onDisconnect() { current = nil }

    /// 收到会话列表:到了目标连接就给出决定并清掉;否则继续等。
    mutating func onSessions(selectedServer: String?, sessions: [SessionInfo]) -> PendingOpenDecision? {
        guard let target = current, selectedServer == target.connectionId else { return nil }
        current = nil
        return sessions.contains { $0.id == target.session } ? .open(target.session, notificationTag: target.notificationTag) : .sessionGone
    }
}

/// Live state for one server transport. Navigation selects a link; only an explicit
/// disconnect/replacement closes it.
@MainActor
final class LiveServerConnection {
    let connection: ServerConnection
    let endpoint: ParsedEndpoint
    let client: GateClient
    var status: ServerConnectionStatus = .connecting
    var sessions: [SessionInfo] = []
    var agents: [String: AgentSession] = [:]
    var transcriptEnabled = false
    var os = ""
    var win32InputMode = false
    var agentTask: Task<Void, Never>?
    var agentAgain = false
    var agentRevision: UInt64 = 0
    var timeoutTask: Task<Void, Never>?

    init(connection: ServerConnection, endpoint: ParsedEndpoint, client: GateClient) {
        self.connection = connection
        self.endpoint = endpoint
        self.client = client
    }

    var http: ServerHTTP { ServerHTTP(origin: endpoint.httpOrigin(), token: endpoint.token ?? "") }

    func close() {
        agentTask?.cancel()
        timeoutTask?.cancel()
        client.close()
        status = .disconnected
    }
}

/// A terminal viewport the client keeps attached locally (a "tab").
@MainActor
final class OpenSession {
    let id: Int64
    let channel: UInt32
    let term: AlacrittyTerm
    var paintGate = TermPaintGate()
    var painted: TermFrame = .empty
    var pendingAttach = true
    var lastSentCols = 0
    var lastSentRows = 0
    var resizeQuietUntil: Date = .distantPast

    init(id: Int64, channel: UInt32, scrollback: Int) {
        self.id = id
        self.channel = channel
        term = AlacrittyTerm(maxScrollback: scrollback)
    }
}
