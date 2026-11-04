import Foundation

/// One server transport. Same reconnect/reattach contract as Android's `GateClient`:
/// exponential backoff, re-`attach` with `last_seq` after every `hello-ok`, and a fatal
/// stop when the server rejects the token before the handshake completes.
/// All public calls and callbacks run on the main actor.
@MainActor
final class GateClient {
    private struct AttachState {
        let sessionId: Int64
        var cols: Int
        var rows: Int
        var lastSeq: Int64?
        var colors: TermColors
    }

    var onOpen: () -> Void = {}
    var onControl: (ServerMsg) -> Void = { _ in }
    var onChannelData: (UInt32, Data) -> Void = { _, _ in }
    var onReconnecting: () -> Void = {}
    var onFatal: (String) -> Void = { _ in }

    private let session: URLSession
    private var url: URL?
    private var token = ""
    private var task: URLSessionWebSocketTask?
    private var attachments: [UInt32: AttachState] = [:]
    private var closedByUser = false
    private var authOk = false
    private var preHelloError: String?
    private var generation: UInt64 = 0
    private var backoff: TimeInterval = 0.25
    private var pingTimer: Timer?

    init() {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 8
        config.waitsForConnectivity = false
        session = URLSession(configuration: config)
    }

    func connect(webSocketURL: String, token: String) {
        close()
        url = URL(string: webSocketURL)
        self.token = token
        closedByUser = false
        backoff = 0.25
        dial()
    }

    func close() {
        closedByUser = true
        authOk = false
        generation &+= 1
        pingTimer?.invalidate()
        pingTimer = nil
        task?.cancel(with: .normalClosure, reason: nil)
        task = nil
        attachments.removeAll()
    }

    /// Coming back from the background: skip the remaining backoff and redial now.
    func kick() {
        guard !closedByUser, !authOk else { return }
        generation &+= 1
        task?.cancel()
        task = nil
        backoff = 0.25
        dial()
    }

    func send(_ msg: ClientMsg) {
        guard authOk else { return }
        task?.send(.data(ControlJson.encode(msg))) { _ in }
    }

    func sendInput(channel: UInt32, bytes: Data) {
        guard !bytes.isEmpty, authOk, attachments[channel] != nil else { return }
        task?.send(.data(Frame.encode(channel: channel, payload: bytes))) { _ in }
    }

    func attach(sessionId: Int64, channel: UInt32, cols: Int, rows: Int, colors: TermColors) {
        attachments[channel] = AttachState(sessionId: sessionId, cols: cols, rows: rows, lastSeq: nil, colors: colors)
        send(.attach(id: sessionId, channel: channel, lastSeq: nil, cols: cols, rows: rows, colors: colors))
    }

    func detach(channel: UInt32) {
        guard attachments.removeValue(forKey: channel) != nil else { return }
        send(.detach(channel: channel))
    }

    func resize(channel: UInt32, cols: Int, rows: Int) {
        guard var state = attachments[channel], state.cols != cols || state.rows != rows else { return }
        state.cols = cols
        state.rows = rows
        attachments[channel] = state
        send(.resize(channel: channel, cols: cols, rows: rows, colors: nil))
    }

    /// 换了主题。协议没有单独的改颜色消息,按 X4 搭在 resize 上,尺寸照旧。
    func setColors(channel: UInt32, colors: TermColors) {
        guard var state = attachments[channel], state.colors != colors else { return }
        state.colors = colors
        attachments[channel] = state
        send(.resize(channel: channel, cols: state.cols, rows: state.rows, colors: colors))
    }

    func noteData(channel: UInt32, byteCount: Int) {
        guard var state = attachments[channel], let seq = state.lastSeq else { return }
        state.lastSeq = seq + Int64(byteCount)
        attachments[channel] = state
    }

    private func dial() {
        generation &+= 1
        let attempt = generation
        authOk = false
        preHelloError = nil
        guard let url else { onSocketGone(attempt: attempt); return }
        var request = URLRequest(url: url)
        request.timeoutInterval = 8
        let task = session.webSocketTask(with: request)
        task.maximumMessageSize = 64 * 1024 * 1024
        self.task = task
        task.resume()
        // URLSession has no explicit "open" callback; the first send completes after the upgrade.
        task.send(.data(ControlJson.encode(.hello(token: token)))) { [weak self] error in
            Task { @MainActor in
                guard let self, self.isCurrent(attempt) else { return }
                if error != nil { self.socketFailed(task: task, attempt: attempt) }
            }
        }
        receive(task: task, attempt: attempt)
    }

    private func isCurrent(_ attempt: UInt64) -> Bool { !closedByUser && generation == attempt }

    private func receive(task: URLSessionWebSocketTask, attempt: UInt64) {
        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self, self.isCurrent(attempt) else { return }
                switch result {
                case let .success(.data(data)):
                    self.handle(data)
                    if self.isCurrent(attempt) { self.receive(task: task, attempt: attempt) }
                case .success:
                    // T4: text frames are ignored after the handshake.
                    self.receive(task: task, attempt: attempt)
                case .failure:
                    self.socketFailed(task: task, attempt: attempt)
                }
            }
        }
    }

    private func socketFailed(task: URLSessionWebSocketTask, attempt: UInt64) {
        guard isCurrent(attempt) else { return }
        if let http = task.response as? HTTPURLResponse, http.statusCode == 401 || http.statusCode == 403 {
            preHelloError = "HTTP \(http.statusCode)"
        }
        onSocketGone(attempt: attempt)
    }

    private func handle(_ data: Data) {
        guard let frame = Frame.decode(data) else { return }
        if frame.channel != Frame.controlChannel {
            onChannelData(frame.channel, frame.payload)
            return
        }
        guard let msg = ControlJson.parse(frame.payload) else { return }
        switch msg {
        case let .error(message, _):
            if !authOk { preHelloError = message }
        case .helloOk:
            authOk = true
            backoff = 0.25
            startPing()
            reattachAll()
            onOpen()
        case let .resyncEnd(channel, seq):
            attachments[channel]?.lastSeq = seq
        default:
            break
        }
        onControl(msg)
    }

    private func startPing() {
        pingTimer?.invalidate()
        let attempt = generation
        pingTimer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.isCurrent(attempt), let task = self.task else { return }
                task.sendPing { error in
                    guard error != nil else { return }
                    Task { @MainActor in self.socketFailed(task: task, attempt: attempt) }
                }
            }
        }
    }

    private func reattachAll() {
        for (channel, state) in attachments.sorted(by: { $0.key < $1.key }) {
            send(.attach(id: state.sessionId, channel: channel, lastSeq: state.lastSeq,
                         cols: state.cols, rows: state.rows, colors: state.colors))
        }
    }

    private func onSocketGone(attempt: UInt64) {
        guard isCurrent(attempt) else { return }
        // Retire this socket before callbacks can close/reconnect it.
        generation &+= 1
        let retired = generation
        pingTimer?.invalidate()
        pingTimer = nil
        task?.cancel()
        task = nil
        if !authOk, let error = preHelloError {
            closedByUser = true
            onFatal(error)
            return
        }
        authOk = false
        onReconnecting()
        let delay = backoff
        backoff = min(backoff * 2, 10)
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self, !self.closedByUser, self.generation == retired else { return }
            self.dial()
        }
    }
}
