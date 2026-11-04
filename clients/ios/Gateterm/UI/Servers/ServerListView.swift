import SwiftUI

/// Sidebar: saved servers. iOS idioms instead of Android's FAB/overflow menus: an "Add" menu in
/// the navigation bar, swipe actions and context menus on rows, settings and account in the toolbar.
struct ServerListView: View {
    @Environment(AppModel.self) private var model
    @Environment(CloudAccountModel.self) private var cloud
    @Binding var sheet: AppSheet?
    @State private var pendingDelete: ServerConnection?

    var body: some View {
        let _ = model.localeRevision
        List(selection: Binding(get: { model.selectedServerId }, set: { model.select(server: $0) })) {
            if !model.connections.isEmpty {
                Section {
                    ForEach(model.connections) { connection in
                        ServerRow(connection: connection, status: model.status(connection.id))
                            .tag(connection.id)
                            .swipeActions(edge: .trailing) {
                                Button(role: .destructive) { pendingDelete = connection } label: {
                                    Label(L.t("action_delete"), systemImage: "trash")
                                }
                                Button { model.startEdit(connection.id) } label: {
                                    Label(L.t("action_edit"), systemImage: "pencil")
                                }
                                .tint(.gray)
                            }
                            .swipeActions(edge: .leading) {
                                if model.status(connection.id).active {
                                    Button { model.disconnectServer(connection.id) } label: {
                                        Label(L.t("connection_disconnect"), systemImage: "bolt.slash")
                                    }
                                    .tint(.orange)
                                }
                            }
                            .contextMenu { menu(for: connection) }
                    }
                } footer: {
                    if let summary = agentSummary { Text(summary) }
                }
            }
        }
        .listStyle(.sidebar)
        .overlay {
            if model.connections.isEmpty {
                ContentUnavailableView {
                    Label(L.t("servers_empty_title"), systemImage: "server.rack")
                } description: {
                    Text(L.t("servers_empty_body"))
                } actions: {
                    addMenu { Text(L.t("servers_add")) }.buttonStyle(.borderedProminent)
                }
            }
        }
        .navigationTitle(L.t("servers_title"))
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                addMenu { Label(L.t("servers_add"), systemImage: "plus") }
            }
            // Secondary actions live in the sidebar's bottom bar, keeping the title readable.
            ToolbarItemGroup(placement: .bottomBar) {
                Button { sheet = .settings } label: { Label(L.t("settings_title"), systemImage: "gearshape") }
                Spacer()
                Button { sheet = .cloud } label: {
                    Label(cloud.status?.email ?? L.t("cloud_title"),
                          systemImage: cloud.status == nil ? "person.crop.circle.badge.xmark" : "person.crop.circle.badge.checkmark")
                        .labelStyle(.titleAndIcon)
                        .symbolRenderingMode(.hierarchical)
                        .lineLimit(1)
                }
                .accessibilityLabel(cloud.status.map { L.t("cloud_connected_as", $0.email) } ?? L.t("cloud_not_connected"))
            }
        }
        .confirmationDialog(L.t("servers_delete_title"), isPresented: Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } }),
                            titleVisibility: .visible, presenting: pendingDelete) { target in
            Button(L.t("action_delete"), role: .destructive) { model.delete(target.id) }
            Button(L.t("action_cancel"), role: .cancel) {}
        } message: { target in
            Text(L.t("servers_delete_body", target.title))
        }
    }

    private func addMenu<Content: View>(@ViewBuilder label: () -> Content) -> some View {
        let content = label()
        return Menu {
            Button { model.startAdd(scan: true) } label: { Label(L.t("servers_add_scan"), systemImage: "qrcode.viewfinder") }
            if let clip = model.clipboardURL() {
                Button { model.startAdd(prefill: clip) } label: {
                    Label(L.t("servers_add_clipboard"), systemImage: "doc.on.clipboard")
                }
            }
            Button { model.startAdd() } label: { Label(L.t("servers_add_manual"), systemImage: "square.and.pencil") }
        } label: { content }
    }

    @ViewBuilder
    private func menu(for connection: ServerConnection) -> some View {
        let status = model.status(connection.id)
        Button { model.connect(connection.id) } label: {
            Label(L.t(status.active ? "connection_open" : "connection_connect"), systemImage: "arrow.right.circle")
        }
        Button { model.reconnect(connection.id) } label: { Label(L.t("connection_reconnect"), systemImage: "arrow.clockwise") }
        if status.active {
            Button { model.disconnectServer(connection.id) } label: { Label(L.t("connection_disconnect"), systemImage: "bolt.slash") }
        }
        Divider()
        Button { model.startEdit(connection.id) } label: { Label(L.t("action_edit"), systemImage: "pencil") }
        Button(role: .destructive) { pendingDelete = connection } label: { Label(L.t("action_delete"), systemImage: "trash") }
    }

    private var agentSummary: String? {
        let parts: [String] = [AgentState.waiting, .error, .working, .thinking, .idle].compactMap { state in
            guard let count = model.agentCounts[state], count > 0 else { return nil }
            return L.t("connections_notification_agent_count", agentStateText(state, pending: true), count)
        }
        return parts.isEmpty ? nil : L.t("connections_notification_agents", parts.joined(separator: " · "))
    }
}

private struct ServerRow: View {
    let connection: ServerConnection
    let status: ServerConnectionStatus

    var body: some View {
        HStack(spacing: 12) {
            Text(String(connection.title.prefix(1)).uppercased())
                .font(.headline)
                .frame(width: 36, height: 36)
                .background(Color.accentColor.opacity(0.18), in: Circle())
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading, spacing: 2) {
                Text(connection.title).font(.body).lineLimit(1)
                Text(connection.subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                HStack(spacing: 5) {
                    Circle().fill(color).frame(width: 6, height: 6)
                    Text(statusText).font(.caption2).foregroundStyle(color)
                }
            }
            Spacer(minLength: 0)
            if status == .connecting || status == .reconnecting { ProgressView().controlSize(.small) }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }

    private var statusText: String {
        switch status {
        case .disconnected: L.t("connection_disconnected")
        case .connecting: L.t("connection_connecting")
        case .connected: L.t("connection_connected")
        case .reconnecting: L.t("connection_reconnecting")
        case .failed: L.t("connection_failed")
        }
    }

    private var color: Color {
        switch status {
        case .connected: .green
        case .connecting, .reconnecting: .orange
        case .failed: .red
        case .disconnected: .secondary
        }
    }
}

func agentStateText(_ state: AgentState, pending: Bool) -> String {
    switch state {
    case .idle: L.t("agent_state_idle")
    case .thinking: L.t("agent_state_thinking")
    case .working: L.t("agent_state_working")
    case .waiting: L.t(pending ? "agent_state_waiting" : "agent_state_waiting_terminal")
    case .error: L.t("agent_state_error")
    case .done: L.t("agent_state_done")
    }
}
