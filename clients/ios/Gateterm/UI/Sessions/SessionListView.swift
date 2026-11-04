import SwiftUI

/// Content column: the selected server's sessions. "+" opens the launcher picker, the clock opens
/// agent history; rows support swipe-to-end and a context menu (rename / end).
struct SessionListView: View {
    @Environment(AppModel.self) private var model
    let connection: ServerConnection
    @State private var launcherOpen = false
    @State private var historyOpen = false
    @State private var renaming: SessionInfo?
    @State private var renameDraft = ""
    @State private var pendingKill: SessionInfo?

    var body: some View {
        let _ = model.localeRevision
        let status = model.status(connection.id)
        List(selection: Binding(get: { model.attachedId }, set: { id in
            if let id { model.openSession(id) } else { model.leaveTerminal() }
        })) {
            if model.reconnecting {
                HStack(spacing: 10) {
                    ProgressView()
                    Text(L.t(status == .connecting ? "ios_connect_hint" : "connection_reconnecting")).foregroundStyle(.secondary)
                }
            }
            ForEach(model.sessions) { session in
                SessionRow(session: session, agent: AgentSessions.forSession(model.agents, session.id),
                           open: model.openSessionIds.contains(session.id))
                    .tag(session.id)
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) { pendingKill = session } label: {
                            Label(L.t("session_kill"), systemImage: "xmark.octagon")
                        }
                        Button { startRename(session) } label: { Label(L.t("ios_rename"), systemImage: "pencil") }
                            .tint(.gray)
                    }
                    .contextMenu {
                        Button { model.openSession(session.id) } label: { Label(L.t("connection_open"), systemImage: "terminal") }
                        Button { startRename(session) } label: { Label(L.t("ios_rename"), systemImage: "pencil") }
                        if model.openSessionIds.contains(session.id) {
                            Button { model.closeOpenSession(session.id) } label: { Label(L.t("ios_close_tab"), systemImage: "xmark.square") }
                        }
                        Divider()
                        Button(role: .destructive) { pendingKill = session } label: {
                            Label(L.t("session_kill"), systemImage: "xmark.octagon")
                        }
                    }
            }
        }
        .listStyle(.insetGrouped)
        .overlay {
            if model.sessions.isEmpty && !model.reconnecting {
                ContentUnavailableView {
                    Label(L.t("session_empty_title"), systemImage: "terminal")
                } description: {
                    Text(L.t("session_empty_body"))
                } actions: {
                    Button(L.t("session_new")) { openLaunchers() }.buttonStyle(.borderedProminent)
                }
            }
        }
        .refreshable { model.refreshSessions() }
        .navigationTitle(connection.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .topBarTrailing) {
                Button { historyOpen = true } label: { Label(L.t("history_title"), systemImage: "clock.arrow.circlepath") }
                    .disabled(model.reconnecting)
                Button { openLaunchers() } label: { Label(L.t("session_new"), systemImage: "plus") }
                    .disabled(model.reconnecting)
            }
        }
        .sheet(isPresented: $launcherOpen) { LauncherSheet() }
        .sheet(isPresented: $historyOpen) { HistoryView() }
        .alert(L.t("session_rename_title"), isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField(L.t("session_rename_label"), text: $renameDraft)
            Button(L.t("action_cancel"), role: .cancel) {}
            Button(L.t("action_save")) { if let renaming { model.rename(renaming.id, to: renameDraft) } }
        }
        .confirmationDialog(L.t("session_kill"), isPresented: Binding(get: { pendingKill != nil }, set: { if !$0 { pendingKill = nil } }),
                            titleVisibility: .visible, presenting: pendingKill) { session in
            Button(L.t("session_kill"), role: .destructive) { model.killSession(session.id) }
            Button(L.t("action_cancel"), role: .cancel) {}
        } message: { session in
            Text(L.t("ios_kill_confirm", sessionTitle(session)))
        }
    }

    private func openLaunchers() {
        model.loadLaunchers()
        launcherOpen = true
    }

    private func startRename(_ session: SessionInfo) {
        renameDraft = session.name
        renaming = session
    }
}

private struct SessionRow: View {
    let session: SessionInfo
    let agent: AgentSession?
    let open: Bool

    var body: some View {
        HStack(spacing: 10) {
            if let agent { AgentStatusBadge(agent: agent) }
            VStack(alignment: .leading, spacing: 4) {
                Text(sessionTitle(session)).lineLimit(1)
                if !meta.isEmpty {
                    Text(meta).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                if let agent {
                    AgentStateTag(agent: agent)
                }
            }
            Spacer(minLength: 0)
            if open { Image(systemName: "circle.fill").font(.system(size: 7)).foregroundStyle(Color.accentColor) }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }

    private var meta: String {
        var parts: [String] = []
        if let cwd = shortCwd(session.cwd) { parts.append(cwd) }
        if open { parts.append(L.t("session_open")) }
        return parts.joined(separator: " · ")
    }
}

func shortCwd(_ cwd: String?) -> String? {
    guard let trimmed = cwd?.trimmingCharacters(in: .whitespaces), !trimmed.isEmpty else { return nil }
    let parts = trimmed.replacingOccurrences(of: "\\", with: "/").split(separator: "/").map(String.init)
    switch parts.count {
    case 0: return "/"
    case 1: return parts[0]
    default: return "\(parts[parts.count - 2])/\(parts[parts.count - 1])"
    }
}

func agentLine(_ agent: AgentSession) -> String {
    let state = agentStateText(agent.state, pending: !(agent.pending ?? "").isEmpty)
    let detail: String? = switch agent.detail {
    case "permission_prompt": L.t("agent_notice_permission")
    case "idle_prompt": L.t("agent_notice_idle")
    case "agent_needs_input": L.t("agent_notice_input")
    case "elicitation_dialog": L.t("agent_notice_choice")
    default: agent.detail
    }
    return (detail ?? "").isEmpty ? state : "\(state) · \(detail!)"
}

/// New session: launchers from `GET /api/launchers`, grouped by provider and searchable.
struct LauncherSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        let items = model.launcherMenu.items.filter { launcher in
            let q = query.trimmingCharacters(in: .whitespaces)
            guard !q.isEmpty else { return true }
            let name = launcher.name.isEmpty ? "shell" : launcher.name
            return "\(name) \(launcher.detail) \(launcher.provider)".localizedCaseInsensitiveContains(q)
        }
        let groups = Dictionary(grouping: items, by: \.provider)
        let order = items.map(\.provider).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
        NavigationStack {
            List {
                if model.launcherMenu.loading && model.launcherMenu.items.count <= 1 {
                    HStack { ProgressView(); Text(L.t("launcher_loading")).foregroundStyle(.secondary) }
                }
                ForEach(order, id: \.self) { provider in
                    Section(providerLabel(provider)) {
                        ForEach(groups[provider] ?? []) { launcher in
                            Button {
                                model.newSession(launcher)
                                dismiss()
                            } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(launcher.name.isEmpty ? L.t("launcher_default_name") : launcher.name).foregroundStyle(.primary)
                                    Text(detail(launcher)).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                                }
                            }
                        }
                    }
                }
            }
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: L.t("launcher_filter"))
            .navigationTitle(L.t("session_new"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button(L.t("action_cancel")) { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
    }

    private func detail(_ launcher: Launcher) -> String {
        let detail = launcher.detail.isEmpty ? L.t("launcher_default_detail") : launcher.detail
        guard let cwd = launcher.cwd, !cwd.isEmpty else { return detail }
        return "\(detail)  ·  \(cwd)"
    }

    private func providerLabel(_ provider: String) -> String {
        switch provider {
        case "builtin": L.t("launcher_provider_builtin")
        case "config": L.t("launcher_provider_config")
        case "iterm2": "iTerm2"
        case "windows-terminal": "Windows Terminal"
        default: provider
        }
    }
}
