import Foundation
import Observation
import UIKit

/// App-wide state and server transports. A port of Android's `ConnectionController`:
/// the protocol rules (paint gate, resize debouncing, agent snapshot refresh, chat cursors)
/// are the same; navigation is expressed as selections so `NavigationSplitView` can show
/// servers, sessions and the terminal side by side on iPad and as a stack on iPhone.
@MainActor
@Observable
final class AppModel {
    static let maxOpen = 8
    private static let proposeDebounce: TimeInterval = 0.08
    private static let publishCoalesce: TimeInterval = 0.032
    private static let resizeQuiet: TimeInterval = 0.8
    private static let desiredKey = "gateterm.connection-runtime.desired"

    // MARK: Observed state

    private(set) var connections: [ServerConnection]
    private(set) var statuses: [String: ServerConnectionStatus] = [:]
    private(set) var agentCounts: [AgentState: Int] = [:]
    /// Sidebar selection (server whose sessions are shown).
    private(set) var selectedServerId: String?
    private(set) var sessions: [SessionInfo] = []
    private(set) var openSessionIds: [Int64] = []
    private(set) var attachedId: Int64?
    private(set) var reconnecting = false
    private(set) var serverOS = ""
    private(set) var win32InputMode = false
    private(set) var agents: [String: AgentSession] = [:]
    private(set) var transcriptEnabled = false
    private(set) var chat = ChatUiState()
    private(set) var launcherMenu = LauncherMenuState()
    private(set) var painted: TermFrame = .empty
    private(set) var termGeneration: UInt64 = 0
    private(set) var scrollOffset = 0
    private(set) var mouseTracking = false
    var selection: TermSelection?
    var editor: EditorState?
    private(set) var appearance: TermAppearance
    private(set) var appearanceFollowsSystem: Bool
    private(set) var extraKeyRows: Int
    private(set) var agentKeyLayouts: [String: AgentKeyLayout]
    private(set) var toast: (id: UUID, message: UserMessage)?
    var localePref: LocalePref = L.pref {
        didSet { L.pref = localePref; localeRevision &+= 1 }
    }
    /// Bumped when the language changes so views re-read `L.t`.
    private(set) var localeRevision = 0

    var chatOpen: Bool { chat.sessionId != nil && chat.sessionId == attachedId }
    var chatAvailable: Bool { AgentSessions.chatAvailable(agents, attachedId, transcriptEnabled: transcriptEnabled) }
    var selectedConnection: ServerConnection? { selectedServerId.flatMap(connection) }
    var attachedSession: SessionInfo? { attachedId.flatMap { id in sessions.first { $0.id == id } } }

    // MARK: Internal state

    @ObservationIgnored private let store = ConnectionStore()
    @ObservationIgnored private let appearanceStore = AppearanceStore()
    @ObservationIgnored private var links: [String: LiveServerConnection] = [:]
    @ObservationIgnored private var activeLinks: Set<String> = []
    @ObservationIgnored private var client: GateClient?
    @ObservationIgnored private var opens: [Int64: OpenSession] = [:]
    @ObservationIgnored private var openOrder: [Int64] = []
    @ObservationIgnored private var uiForeground = true
    @ObservationIgnored private var viewportsSuspended = false
    @ObservationIgnored private var awaitingCreated = false
    @ObservationIgnored private var pendingOpen = PendingOpenTracker()
    @ObservationIgnored private var chatTask: Task<Void, Never>?
    @ObservationIgnored private var olderChatTask: Task<Void, Never>?
    @ObservationIgnored private var chatTimeline: UInt64 = 0
    @ObservationIgnored private var chatAgain = false
    @ObservationIgnored private var chatCursor: String?
    @ObservationIgnored private var chatAgentKey: String?
    @ObservationIgnored private var proposedCols = 80
    @ObservationIgnored private var proposedRows = 24
    @ObservationIgnored private var proposeWork: DispatchWorkItem?
    @ObservationIgnored private var publishWork: DispatchWorkItem?
    @ObservationIgnored private var systemDark: Bool
    @ObservationIgnored private var launcherTask: Task<Void, Never>?

    init() {
        let dark = UITraitCollection.current.userInterfaceStyle != .light
        systemDark = dark
        connections = store.load()
        appearance = appearanceStore.load(prefersDark: dark)
        appearanceFollowsSystem = appearanceStore.followsSystem
        extraKeyRows = appearanceStore.extraKeyRows
        agentKeyLayouts = appearanceStore.agentKeyLayouts
    }

    func connection(_ id: String) -> ServerConnection? { connections.first { $0.id == id } }
    func status(_ id: String) -> ServerConnectionStatus { statuses[id] ?? .disconnected }
    var http: ServerHTTP? { selectedServerId.flatMap { links[$0] }.map(\.http) }

    func post(_ message: UserMessage) { toast = (UUID(), message) }
    func dismissToast() { toast = nil }

    // MARK: Editor

    func startAdd(prefill: String? = nil, scan: Bool = false) {
        var state = EditorState(clipboardURL: clipboardURL(), scanOnOpen: scan)
        if let prefill { state = applyPrefill(state, prefill) }
        editor = refreshWarnings(state)
    }

    func startEdit(_ id: String) {
        guard let existing = connection(id) else { return }
        editor = refreshWarnings(EditorState(connectionId: id, name: existing.name, url: existing.url, token: existing.token,
                                             clipboardURL: clipboardURL()))
    }

    func applyScanned(_ scanned: ScannedConnection) {
        guard var state = editor else { return }
        state.url = scanned.url
        state.token = scanned.token
        if state.name.trimmingCharacters(in: .whitespaces).isEmpty { state.name = scanned.name }
        state.urlError = nil
        state.tokenError = nil
        editor = refreshWarnings(state)
    }

    func editorChanged(_ state: EditorState) {
        guard editor != nil else { return }
        var next = state
        if next.url != editor?.url { next.urlError = nil }
        if next.token != editor?.token { next.tokenError = nil }
        editor = refreshWarnings(next)
    }

    func pasteClipboardIntoEditor() {
        guard let clip = clipboardURL(), let current = editor else { return }
        editor = refreshWarnings(applyPrefill(current, clip))
    }

    func clipboardURL() -> String? {
        guard UIPasteboard.general.hasURLs || UIPasteboard.general.hasStrings,
              let text = (UIPasteboard.general.url?.absoluteString ?? UIPasteboard.general.string)?
                .trimmingCharacters(in: .whitespacesAndNewlines),
              ConnectionUrl.looksLikeURL(text) else { return nil }
        return text
    }

    private func applyPrefill(_ base: EditorState, _ raw: String) -> EditorState {
        var next = base
        next.url = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if let parsed = try? ConnectionUrl.parse(raw) {
            if let token = parsed.token { next.token = token }
            if next.name.trimmingCharacters(in: .whitespaces).isEmpty { next.name = parsed.displayHost }
        }
        return next
    }

    private func refreshWarnings(_ state: EditorState) -> EditorState {
        var next = state
        let parsed = try? ConnectionUrl.parse(state.url, tokenOverride: state.token)
        next.loopbackWarning = parsed?.isLoopback ?? false
        next.wildcardWarning = parsed?.isWildcardBind ?? false
        return next
    }

    /// Returns true when saved (the sheet can close).
    @discardableResult
    func saveEditor() -> Bool {
        guard var state = editor else { return false }
        let parsed: ParsedEndpoint
        do { parsed = try ConnectionUrl.parse(state.url, tokenOverride: state.token) } catch {
            state.urlError = state.url.trimmingCharacters(in: .whitespaces).isEmpty ? .emptyURL : .badURL
            editor = state
            return false
        }
        guard let token = parsed.token, !token.isEmpty else {
            state.tokenError = .emptyToken
            editor = state
            return false
        }
        let name = state.name.trimmingCharacters(in: .whitespacesAndNewlines)
        let url = state.url.trimmingCharacters(in: .whitespacesAndNewlines)
        var updated = connections
        if let id = state.connectionId, let index = updated.firstIndex(where: { $0.id == id }) {
            let previous = updated[index]
            updated[index].name = name
            updated[index].url = url
            updated[index].token = token
            if previous.url != url || previous.token != token { disconnectServer(id) }
        } else {
            updated.append(ServerConnection(id: UUID().uuidString, name: name, url: url, token: token,
                                            createdAt: Int64(Date().timeIntervalSince1970 * 1000)))
        }
        persist(updated)
        editor = nil
        post(.saved)
        return true
    }

    func delete(_ id: String) {
        disconnectServer(id)
        links.removeValue(forKey: id)
        persist(connections.filter { $0.id != id })
        publishConnections()
        post(.deleted)
    }

    func move(from source: IndexSet, to destination: Int) {
        var next = connections
        next.move(fromOffsets: source, toOffset: destination)
        persist(next)
    }

    private func persist(_ items: [ServerConnection]) {
        connections = items
        store.save(items)
    }

    // MARK: Server transports

    /// Selecting a server in the sidebar: open (or reuse) its transport and show its sessions.
    func select(server id: String?) {
        guard let id else { leaveServer(); return }
        guard id != selectedServerId || links[id]?.status.active != true else { return }
        connect(id)
    }

    func connect(_ id: String) {
        guard let connection = connection(id) else { pendingOpen.onConnectFailed(); return }
        if selectedServerId != nil && selectedServerId != id { leaveServer() }
        pendingOpen.onConnectStart(id)
        selectedServerId = id
        var link = links[id]
        if link == nil || link?.status.active != true || link?.connection.url != connection.url || link?.connection.token != connection.token {
            link = createLink(connection)
        }
        guard let link else { leaveServer(); return }
        activate(link)
        if link.status == .connected {
            link.client.send(.listSessions)
            refreshAgents(link)
        }
    }

    private func activate(_ link: LiveServerConnection) {
        selectedServerId = link.connection.id
        client = link.client
        let connected = link.status == .connected
        sessions = link.sessions
        agents = link.agents
        transcriptEnabled = link.transcriptEnabled
        serverOS = link.os
        win32InputMode = link.win32InputMode
        reconnecting = !connected
    }

    private func isCurrent(_ id: String, _ link: LiveServerConnection) -> Bool {
        activeLinks.contains(id) && links[id] === link
    }

    private func createLink(_ connection: ServerConnection) -> LiveServerConnection? {
        guard let endpoint = try? ConnectionUrl.parse(connection.url, tokenOverride: connection.token) else {
            pendingOpen.onConnectFailed(); post(.unexpected); return nil
        }
        guard let token = endpoint.token, !token.isEmpty else {
            pendingOpen.onConnectFailed(); post(.authFailed(nil)); return nil
        }
        let next = GateClient()
        let link = LiveServerConnection(connection: connection, endpoint: endpoint, client: next)
        if let previous = links[connection.id], previous !== link, activeLinks.contains(connection.id) { previous.close() }
        links[connection.id] = link
        activeLinks.insert(connection.id)
        if selectedServerId == connection.id { client = next }
        publishConnections()
        let id = connection.id
        next.onOpen = { [weak self, weak link] in
            guard let self, let link, self.isCurrent(id, link) else { return }
            link.status = .connected
            link.timeoutTask?.cancel()
            let now = Int64(Date().timeIntervalSince1970 * 1000)
            self.persist(self.connections.map { var c = $0; if c.id == id { c.lastUsedAt = now }; return c })
            self.publishConnections()
            next.send(.listSessions)
            link.agentRevision &+= 1
            self.refreshAgents(link)
            if self.client === next && self.selectedServerId == id {
                self.reconnecting = false
                if self.viewportsSuspended && self.uiForeground { self.resumeViewports() }
            }
        }
        next.onReconnecting = { [weak self, weak link] in
            guard let self, let link, self.isCurrent(id, link) else { return }
            link.status = .reconnecting
            self.publishConnections()
            if self.client === next { self.reconnecting = true }
        }
        next.onFatal = { [weak self, weak link] message in
            guard let self, let link, self.isCurrent(id, link) else { return }
            self.activeLinks.remove(id)
            link.close()
            link.status = .failed
            if self.client === next {
                self.pendingOpen.onConnectFailed()
                self.leaveServer()
                self.post(.authFailed(message))
            }
            self.publishConnections()
        }
        next.onControl = { [weak self, weak link] msg in
            guard let self, let link, self.isCurrent(id, link) else { return }
            switch msg {
            case let .helloOk(_, os, cloudServerId, win32):
                link.os = os
                link.win32InputMode = win32
                if let updated = CloudLink.record(self.connections, connectionId: id, cloudServerId: cloudServerId) { self.persist(updated) }
            case let .sessions(list):
                link.sessions = list
            case .sessionEvent:
                next.send(.listSessions)
            case let .agentEvent(session):
                link.agentRevision &+= 1
                link.agents = AgentSessions.applyEvent(link.agents, session)
                self.publishAgentActivity()
                if session.state != .done && link.agents[session.key]?.supportsTranscript == nil { self.refreshAgents(link) }
            default:
                break
            }
            if self.client === next && self.selectedServerId == id {
                self.serverOS = link.os
                self.win32InputMode = link.win32InputMode
                self.agents = link.agents
                self.transcriptEnabled = link.transcriptEnabled
                self.handleServer(msg)
            }
        }
        next.onChannelData = { [weak self] channel, payload in
            guard let self, self.client === next, self.selectedServerId == id, !self.viewportsSuspended,
                  let open = self.opens.values.first(where: { $0.channel == channel }) else { return }
            open.term.write(payload)
            TerminalHostView.drainClipboardToSystem(open.term)
            next.noteData(channel: channel, byteCount: payload.count)
            if open.id == self.attachedId && open.paintGate.shouldPublishWrite { self.schedulePublish() }
        }
        next.connect(webSocketURL: endpoint.webSocketURL, token: token)
        link.timeoutTask = Task { [weak self, weak link] in
            try? await Task.sleep(for: .seconds(12))
            guard let self, let link, !Task.isCancelled, self.isCurrent(id, link), link.status == .connecting else { return }
            self.activeLinks.remove(id)
            link.close()
            link.status = .failed
            if self.client === next { self.pendingOpen.onConnectFailed(); self.leaveServer(); self.post(.timeout) }
            self.publishConnections()
        }
        return link
    }

    /// Navigation releases terminal viewports but leaves the server transport running.
    func leaveServer() {
        pendingOpen.onDisconnect()
        leaveTerminal()
        client = nil
        selectedServerId = nil
        sessions = []
        agents = [:]
        reconnecting = false
        serverOS = ""
        win32InputMode = false
        transcriptEnabled = false
        launcherMenu = LauncherMenuState()
    }

    func disconnectServer(_ id: String) {
        if selectedServerId == id { leaveServer() }
        if activeLinks.remove(id) != nil { links[id]?.close() }
        publishConnections()
    }

    func reconnect(_ id: String) {
        let wasSelected = selectedServerId == id
        disconnectServer(id)
        guard let connection = connection(id) else { return }
        if wasSelected { connect(id) } else { _ = createLink(connection) }
    }

    func disconnectAll() {
        leaveServer()
        for id in activeLinks { links[id]?.close() }
        activeLinks.removeAll()
        publishConnections()
    }

    private func publishConnections(persistDesired: Bool = true) {
        statuses = links.mapValues(\.status)
        publishAgentActivity()
        if persistDesired { UserDefaults.standard.set(Array(activeLinks), forKey: Self.desiredKey) }
    }

    private func publishAgentActivity() {
        agentCounts = connectedAgentCounts(links.values.map { ($0.status, Array($0.agents.values)) })
    }

    /// Reconnects the servers that were connected when the app last ran or went to the background.
    func restoreDesiredConnections() {
        let desired = Set(UserDefaults.standard.stringArray(forKey: Self.desiredKey) ?? [])
        for connection in connections where desired.contains(connection.id) && links[connection.id]?.status.active != true {
            guard let link = createLink(connection) else { continue }
            if selectedServerId == connection.id { activate(link) }
        }
    }

    func systemLocaleChanged() {
        let previous = L.locale
        if L.refresh() != previous { localeRevision &+= 1 }
    }

    /// iOS suspends sockets in the background. Detach viewports on the way out and redial
    /// immediately on return instead of waiting out the backoff.
    func setForeground(_ foreground: Bool) {
        guard foreground != uiForeground else { return }
        uiForeground = foreground
        if foreground {
            systemLocaleChanged()
            restoreDesiredConnections()
            for id in activeLinks { links[id]?.client.kick() }
            if viewportsSuspended && client != nil && !reconnecting { resumeViewports() }
            if chatOpen { refreshChat(incremental: true) }
        } else {
            suspendViewports()
            chatTask?.cancel(); chatTask = nil
            olderChatTask?.cancel(); olderChatTask = nil
            chat.olderLoading = false
        }
    }

    private func suspendViewports() {
        guard !viewportsSuspended, !opens.isEmpty else { return }
        proposeWork?.cancel()
        publishWork?.cancel()
        for open in opens.values {
            client?.detach(channel: open.channel)
            open.paintGate.onDetached()
            open.pendingAttach = false
        }
        viewportsSuspended = true
    }

    private func resumeViewports() {
        guard uiForeground, attachedId != nil, !chatOpen else { return }
        viewportsSuspended = false
        for open in opens.values { open.pendingAttach = true; open.paintGate.onSessionOpen() }
        scheduleProposeFlush(after: 0)
    }

    // MARK: Notifications

    func openFromNotification(cloudServerId: String, session: Int64, agentId: String?) {
        guard let connection = CloudLink.find(connections, cloudServerId: cloudServerId) else { post(.connectServerFirst); return }
        let tag = agentId.map { AgentPush.tag(serverId: cloudServerId, session: session, agentId: $0) }
        if selectedServerId != connection.id { leaveServer() }
        pendingOpen.onTap(PendingOpen(connectionId: connection.id, session: session, notificationTag: tag))
        connect(connection.id)
        client?.send(.listSessions)
    }

    private func consumePendingOpen() {
        switch pendingOpen.onSessions(selectedServer: selectedServerId, sessions: sessions) {
        case nil: break
        case let .open(session, tag):
            openSession(session)
            // 真打开了才撤回通知;打不开(会话太多等)就留着,用户还能再点。
            if opens[session] != nil, let tag { AgentNotifications.remove(tag: tag) }
        case .sessionGone:
            post(.sessionGone)
        }
    }

    // MARK: Launchers and sessions

    func loadLaunchers() {
        guard let http else { return }
        launcherMenu = LauncherMenuState(loading: true, items: [.fallback])
        launcherTask?.cancel()
        launcherTask = Task {
            let items = await Launchers.fetch(http)
            guard !Task.isCancelled else { return }
            launcherMenu = LauncherMenuState(loading: false, items: items)
        }
    }

    func newSession(_ launcher: Launcher) {
        awaitingCreated = true
        client?.send(.newSession(name: launcher.name.isEmpty ? nil : launcher.name,
                                 command: launcher.command.isEmpty ? nil : launcher.command, cwd: launcher.cwd))
    }

    func rename(_ id: Int64, to name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        client?.send(.renameSession(id: id, name: trimmed))
    }

    func killSession(_ id: Int64) {
        if opens[id] != nil { closeOpenSession(id) }
        client?.send(.killSession(id: id))
    }

    func refreshSessions() {
        client?.send(.listSessions)
        if let id = selectedServerId, let link = links[id] { refreshAgents(link) }
    }

    func loadHistory(query: String, agent: String, offset: Int, refresh: Bool, workspace: String) async throws -> HistoryPage {
        guard let http else { throw HistoryFailure(reason: "unreadable") }
        return try await AgentHistory.fetch(http, query: query, agent: agent, offset: offset, refresh: refresh, workspace: workspace)
    }

    func previewHistory(_ entry: HistoryEntry) async throws -> [ChatMessage] {
        guard let http else { throw HistoryFailure(reason: "unreadable") }
        return try await AgentHistory.messages(http, entry)
    }

    func resumeHistory(_ entry: HistoryEntry) async throws {
        guard let connection = client, !reconnecting, let http else { throw HistoryFailure(reason: "unreadable") }
        let session = try await AgentHistory.open(http, entry)
        guard client === connection, selectedServerId != nil else { return }
        if !sessions.contains(where: { $0.id == session.id }) { sessions.append(session) }
        if opens[session.id] == nil && opens.count >= Self.maxOpen {
            // Make room in the local render cache; the detached remote terminal keeps running.
            if let victim = openOrder.first(where: { $0 != attachedId }) { closeOpenSession(victim) }
        }
        openSession(session.id)
        connection.send(.listSessions)
        if let id = selectedServerId, let link = links[id] { link.agentRevision &+= 1; refreshAgents(link) }
    }

    func openSession(_ sessionId: Int64) {
        if chat.sessionId != sessionId { resetChat() }
        if opens[sessionId] != nil {
            attachedId = sessionId
            selection = nil
            if viewportsSuspended && uiForeground { resumeViewports() }
            publishTerm()
            return
        }
        guard opens.count < Self.maxOpen else { post(.tooManySessions); return }
        let open = OpenSession(id: sessionId, channel: nextChannel(), scrollback: appearance.scrollback)
        open.paintGate.onSessionOpen()
        opens[sessionId] = open
        openOrder.append(sessionId)
        if !uiForeground || chatOpen { viewportsSuspended = true }
        painted = .empty
        attachedId = sessionId
        openSessionIds = openOrder
        selection = nil
        scrollOffset = 0
        mouseTracking = false
        termGeneration = open.term.generation
        scheduleProposeFlush(after: 0)
    }

    /// Returning to the session list (iPhone) releases every local terminal viewport.
    func leaveTerminal() {
        resetChat()
        proposeWork?.cancel()
        publishWork?.cancel()
        for open in opens.values {
            client?.detach(channel: open.channel)
            open.paintGate.onDetached()
        }
        opens.removeAll()
        openOrder.removeAll()
        awaitingCreated = false
        painted = .empty
        viewportsSuspended = false
        attachedId = nil
        openSessionIds = []
        selection = nil
        scrollOffset = 0
        mouseTracking = false
    }

    func closeOpenSession(_ id: Int64) {
        guard let open = opens.removeValue(forKey: id) else { return }
        openOrder.removeAll { $0 == id }
        if chat.sessionId == id { resetChat() }
        client?.detach(channel: open.channel)
        open.paintGate.onDetached()
        openSessionIds = openOrder
        if attachedId == id {
            attachedId = openOrder.last
            if attachedId == nil {
                painted = .empty
                selection = nil
                scrollOffset = 0
                mouseTracking = false
            } else {
                publishTerm()
            }
        }
    }

    var activeTerm: AlacrittyTerm? { attachedId.flatMap { opens[$0]?.term } }

    func sendInput(_ bytes: Data) {
        guard let id = attachedId else { return }
        sendInput(id, bytes)
    }

    func sendInput(_ sessionId: Int64, _ bytes: Data) {
        guard !reconnecting, !bytes.isEmpty, let open = opens[sessionId] else { return }
        open.term.scrollToBottom()
        client?.sendInput(channel: open.channel, bytes: bytes)
        selection = nil
        publishTerm()
    }

    func sendMouse(col: Int, row: Int, pressed: Bool, moving: Bool) {
        guard let open = attachedId.flatMap({ opens[$0] }) else { return }
        if open.term.scrollOffset != 0 { open.term.scrollToBottom() }
        guard let bytes = open.term.encodePointer(col: col, row: row, pressed: pressed, moving: moving), !bytes.isEmpty else { return }
        client?.sendInput(channel: open.channel, bytes: bytes)
    }

    func scroll(by delta: Int) {
        guard let open = attachedId.flatMap({ opens[$0] }) else { return }
        open.term.scroll(by: delta)
        selection = nil
        publishTerm(allowEmpty: true)
    }

    func jumpToBottom() {
        guard let open = attachedId.flatMap({ opens[$0] }) else { return }
        open.term.scrollToBottom()
        publishTerm(allowEmpty: true)
    }

    func copySelection() {
        guard let open = attachedId.flatMap({ opens[$0] }), let selection else { return }
        let text = open.term.copyViewport(selection)
        guard !text.isEmpty else { return }
        UIPasteboard.general.string = text
        post(.copied)
    }

    func pasteClipboard() {
        guard let open = attachedId.flatMap({ opens[$0] }), let text = UIPasteboard.general.string, !text.isEmpty else { return }
        open.term.scrollToBottom()
        client?.sendInput(channel: open.channel, bytes: open.term.encodePaste(text))
        selection = nil
        publishTerm()
    }

    func proposeSize(cols: Int, rows: Int) {
        proposedCols = max(20, cols)
        proposedRows = max(5, rows)
        scheduleProposeFlush(after: Self.proposeDebounce)
    }

    private func handleServer(_ msg: ServerMsg) {
        switch msg {
        case let .sessions(list):
            sessions = list
            consumePendingOpen()
        case let .sessionEvent(kind, session):
            if kind == .closed {
                if opens[session.id] != nil { closeOpenSession(session.id) }
            } else if kind == .created && awaitingCreated {
                awaitingCreated = false
                if !sessions.contains(where: { $0.id == session.id }) { sessions.append(session) }
                openSession(session.id)
            }
        case let .attached(channel, _, mode):
            openSession(onChannel: channel)?.paintGate.onAttached(replay: mode == .replay)
        case let .resized(channel, cols, rows):
            // 权威几何只改终端网格。提交画面会把「正确重绘」换成裁切后的空/错帧。
            openSession(onChannel: channel)?.term.resize(cols: cols, rows: rows)
        case let .resyncBegin(channel):
            guard let open = openSession(onChannel: channel) else { return }
            open.paintGate.onResyncBegin()
            open.term.reset()
            open.term.setMaxScrollback(appearance.scrollback)
        case let .resyncEnd(channel, _):
            guard let open = openSession(onChannel: channel) else { return }
            open.paintGate.onResyncEnd()
            if open.id == attachedId {
                publishWork?.cancel()
                publishTerm()
            }
            // 首帧刚落地时 layout 还会再报一两次差 1 列的尺寸,立刻 resize 会让重绘被清屏盖掉。
            open.resizeQuietUntil = Date().addingTimeInterval(Self.resizeQuiet)
        case let .sessionExited(channel, _):
            if let open = openSession(onChannel: channel) { closeOpenSession(open.id) }
            client?.send(.listSessions)
        case let .agentEvent(session):
            closeChatIfUnavailable()
            if uiForeground && chatOpen && session.session == chat.sessionId { refreshChat(incremental: true) }
        case let .error(message, channel):
            post(.authFailed(message))
            if let channel, let open = openSession(onChannel: channel) { closeOpenSession(open.id) }
        default:
            break
        }
    }

    private func openSession(onChannel channel: UInt32) -> OpenSession? { opens.values.first { $0.channel == channel } }

    // MARK: Agents and chat

    private func refreshAgents(_ link: LiveServerConnection) {
        if link.agentTask != nil { link.agentAgain = true; return }
        link.agentTask = Task { [weak self, weak link] in
            defer { link?.agentTask = nil }
            repeat {
                guard let self, let link else { return }
                link.agentAgain = false
                let revision = link.agentRevision
                let snapshot = await AgentSessions.fetch(link.http)
                guard !Task.isCancelled, self.isCurrent(link.connection.id, link) else { return }
                if revision != link.agentRevision { link.agentAgain = true; continue }
                link.agents = AgentSessions.replaceAll(snapshot.sessions)
                link.transcriptEnabled = snapshot.transcriptEnabled
                self.publishAgentActivity()
                if self.client === link.client && self.selectedServerId == link.connection.id {
                    self.agents = link.agents
                    self.transcriptEnabled = snapshot.transcriptEnabled
                    self.closeChatIfUnavailable()
                    if self.uiForeground && self.chatOpen { self.refreshChat(incremental: true) }
                }
            } while link?.agentAgain == true
        }
    }

    /// 聊天 ⇄ 终端。只有本会话有读得了记录的 agent 时才能进聊天;回终端永远可以。
    func toggleChat() {
        if chatOpen {
            resetChat()
            resumeViewports()
            return
        }
        guard let sessionId = attachedId, chatAvailable else { return }
        suspendViewports()
        chat = ChatUiState(sessionId: sessionId, loading: true)
        refreshChat(incremental: false)
    }

    private func closeChatIfUnavailable() {
        if chat.sessionId != nil && !chatAvailable { resetChat(); resumeViewports() }
    }

    private func resetChat() {
        chatTimeline &+= 1
        olderChatTask?.cancel(); olderChatTask = nil
        chatTask?.cancel(); chatTask = nil
        chatAgain = false
        chatCursor = nil
        chatAgentKey = nil
        if chat != ChatUiState() { chat = ChatUiState() }
    }

    /// 拉取串行化,期间再来的请求合并成「再拉一次」。`more` 时接着拉,但只有游标真的动了才继续。
    private func refreshChat(incremental: Bool) {
        if chatTask != nil { chatAgain = true; return }
        guard let http else { return }
        chatTask = Task { [weak self] in
            defer { self?.chatTask = nil }
            var inc = incremental
            repeat {
                guard let self, !Task.isCancelled else { return }
                self.chatAgain = false
                guard let sessionId = self.chat.sessionId, self.chatOpen else { return }
                guard let agent = AgentSessions.chatAgentFor(self.agents, sessionId) else {
                    self.chat.notice = .noAgent
                    self.chat.loading = false
                    return
                }
                if agent.key != self.chatAgentKey { inc = false }
                let win = await Chat.fetch(http, agentId: agent.agentId, agentSessionId: agent.agentSessionId,
                                           after: inc ? self.chatCursor : nil)
                guard !Task.isCancelled, self.chat.sessionId == sessionId, self.chatOpen else { return }
                if AgentSessions.chatAgentFor(self.agents, sessionId)?.key != agent.key {
                    self.chatAgain = true
                    continue
                }
                switch win {
                case let .terminal(reason):
                    self.chat.notice = .server(reason, agentId: agent.agentId)
                    self.chat.loading = false
                case let .transcript(_, cursor, reset, more, olderCursor, historySupported):
                    let advanced = !inc || cursor != self.chatCursor
                    let fresh = reset || !inc
                    if fresh {
                        self.chatTimeline &+= 1
                        self.olderChatTask?.cancel(); self.olderChatTask = nil
                    }
                    self.chatCursor = cursor
                    self.chatAgentKey = agent.key
                    var next = self.chat
                    next.messages = Chat.merge(inc ? next.messages : [], win)
                    next.notice = nil
                    next.loading = false
                    if fresh {
                        next.olderCursor = olderCursor
                        next.historySupported = historySupported
                        next.olderLoading = false
                        next.olderFailed = false
                    }
                    self.chat = next
                    if more && advanced { self.chatAgain = true }
                }
                inc = true
            } while self?.chatAgain == true
        }
    }

    func loadOlderChat() {
        guard let sessionId = chat.sessionId, let before = chat.olderCursor,
              let agent = AgentSessions.chatAgentFor(agents, sessionId), let http,
              chatOpen, !chat.olderLoading, olderChatTask == nil else { return }
        let timeline = chatTimeline
        chat.olderLoading = true
        chat.olderFailed = false
        olderChatTask = Task { [weak self] in
            defer { self?.olderChatTask = nil }
            let win = await Chat.fetch(http, agentId: agent.agentId, agentSessionId: agent.agentSessionId, after: nil, before: before)
            guard let self, !Task.isCancelled, timeline == self.chatTimeline, self.chatOpen, self.chat.sessionId == sessionId,
                  self.chat.olderCursor == before, AgentSessions.chatAgentFor(self.agents, sessionId)?.key == agent.key else { return }
            guard case let .transcript(_, cursor, reset, _, olderCursor, historySupported) = win, reset || olderCursor != before else {
                self.chat.olderLoading = false
                self.chat.olderFailed = true
                return
            }
            if reset {
                self.chatTask?.cancel(); self.chatTask = nil; self.chatAgain = false
                self.chatTimeline &+= 1
                self.chatCursor = cursor
                self.chatAgentKey = agent.key
            }
            var next = self.chat
            next.messages = Chat.merge(next.messages, win, prepend: true)
            next.olderCursor = olderCursor
            next.historySupported = historySupported
            next.olderLoading = false
            next.olderFailed = false
            next.historyPage += 1
            self.chat = next
        }
    }

    // MARK: Painting and sizing

    private func scheduleProposeFlush(after delay: TimeInterval) {
        proposeWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.applyProposedSize() }
        proposeWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    private func applyProposedSize() {
        guard !viewportsSuspended, uiForeground, !chatOpen else { return }
        var retryIn = TimeInterval.infinity
        for open in opens.values {
            if open.pendingAttach {
                open.pendingAttach = false
                open.lastSentCols = proposedCols
                open.lastSentRows = proposedRows
                client?.attach(sessionId: open.id, channel: open.channel, cols: proposedCols, rows: proposedRows, colors: termColors)
                continue
            }
            guard open.paintGate.shouldSendResize else { continue }
            let wait = open.resizeQuietUntil.timeIntervalSinceNow
            if wait > 0 { retryIn = min(retryIn, wait); continue }
            guard significantResize(proposedCols, proposedRows, open.lastSentCols, open.lastSentRows) else { continue }
            open.lastSentCols = proposedCols
            open.lastSentRows = proposedRows
            client?.resize(channel: open.channel, cols: proposedCols, rows: proposedRows)
        }
        if retryIn.isFinite { scheduleProposeFlush(after: max(0.001, retryIn)) }
    }

    private func schedulePublish() {
        publishWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.publishTerm() }
        publishWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.publishCoalesce, execute: work)
    }

    private func publishTerm(allowEmpty: Bool = false) {
        guard let id = attachedId, let open = opens[id] else { return }
        let next = open.term.capture()
        // 服务端重绘以 2J 清屏开头。若清屏被单独提交,正确画面会被空网格盖住。
        if !allowEmpty && !next.hasContent() && open.painted.hasContent() && next.scrollOffset == open.painted.scrollOffset { return }
        open.painted = next
        painted = next
        termGeneration = open.term.generation
        scrollOffset = open.term.scrollOffset
        mouseTracking = open.term.mouseTracking
        if allowEmpty { selection = nil }
    }

    private func nextChannel() -> UInt32 {
        let used = Set(opens.values.map(\.channel))
        var channel: UInt32 = 1
        while used.contains(channel) { channel += 1 }
        return channel
    }

    // MARK: Appearance

    func setFontSize(_ size: Double) { updateAppearance(appearance.withFontSize(size)) }
    func setLineHeight(_ value: Double) { updateAppearance(appearance.withLineHeight(value)) }
    func setLetterSpacing(_ value: Double) { updateAppearance(appearance.withLetterSpacing(value)) }
    func setFont(_ kind: TermFontKind) { updateAppearance(appearance.withFont(kind)) }
    func setScrollback(_ lines: Int) { updateAppearance(appearance.withScrollback(lines)) }

    func setExtraKeyRows(_ rows: Int) {
        appearanceStore.extraKeyRows = rows
        extraKeyRows = appearanceStore.extraKeyRows
    }

    func setAgentKeyLayout(_ agentId: String, _ layout: AgentKeyLayout) {
        var next = agentKeyLayouts
        if layout == AgentKeyLayout() { next.removeValue(forKey: agentId) } else { next[agentId] = layout }
        appearanceStore.agentKeyLayouts = next
        agentKeyLayouts = next
    }

    func systemThemeChanged(prefersDark: Bool) {
        systemDark = prefersDark
        if appearanceFollowsSystem { appearance = appearance.withTheme(AppearanceCodec.default(prefersDark: prefersDark)) }
        reportColors()
    }

    var systemThemeName: String { AppearanceCodec.default(prefersDark: systemDark).name }

    func followSystemTheme() { updateAppearance(appearance.withTheme(AppearanceCodec.default(prefersDark: systemDark)), followsSystem: true) }

    func selectTheme(_ name: String) {
        guard let theme = AppearanceCodec.builtin(name) else { return }
        updateAppearance(appearance.withTheme(theme), followsSystem: false)
    }

    func copyAppearance() {
        UIPasteboard.general.string = AppearanceCodec.exportJSON(appearance)
        post(.copied)
    }

    func importAppearanceFromClipboard() {
        guard let text = UIPasteboard.general.string, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let next = AppearanceCodec.importJSON(text, base: appearance) else { post(.importFailed); return }
        updateAppearance(next, followsSystem: false)
        post(.saved)
    }

    private func updateAppearance(_ next: TermAppearance, followsSystem: Bool? = nil) {
        let follows = followsSystem ?? appearanceFollowsSystem
        appearanceStore.save(next, followsSystem: follows)
        let scrollbackChanged = next.scrollback != appearance.scrollback
        appearance = next
        appearanceFollowsSystem = follows
        if scrollbackChanged { for open in opens.values { open.term.setMaxScrollback(next.scrollback) } }
        if attachedId != nil { publishTerm(allowEmpty: true) }
        reportColors()
    }

    private var termColors: TermColors { TermColors.fromRGB(fg: appearance.foreground, bg: appearance.background) }

    /// 服务端拿这个应答 OSC 10/11 颜色查询,主题一变就得重报。
    private func reportColors() {
        let colors = termColors
        for open in opens.values { client?.setColors(channel: open.channel, colors: colors) }
    }
}
