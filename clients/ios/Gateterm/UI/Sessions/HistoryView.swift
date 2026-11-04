import SwiftUI

/// Agent session history. iOS idioms over Android's separate filter screen: a search field,
/// a filter menu in the toolbar, pull to refresh, and a pushed conversation preview.
struct HistoryView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var agent = ""
    @State private var workspace = ""
    @State private var page: HistoryPage?
    @State private var loading = false
    @State private var error: String?
    @State private var working = false
    @State private var refreshToken = 0

    private var filterCount: Int { [agent, workspace].filter { !$0.isEmpty }.count }

    var body: some View {
        NavigationStack {
            List {
                if let error {
                    Label(historyErrorText(error), systemImage: "exclamationmark.triangle").foregroundStyle(.red)
                }
                if let page {
                    Section {
                        ForEach(page.sessions) { entry in row(entry) }
                        if let offset = page.nextOffset {
                            Button(L.t("history_more")) { loadMore(offset) }.disabled(loading || working)
                        } else if !page.indexing, let maximum = page.maxSessions {
                            Text(L.t("history_limit", maximum)).font(.footnote).foregroundStyle(.secondary)
                        }
                    } header: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(page.indexing ? L.t("history_indexing")
                                 : page.total == 0 ? L.t("history_empty") : L.t("history_count", page.total))
                            if page.truncated { Text(L.t("history_truncated")) }
                            if page.skipped > 0 { Text(L.t("history_skipped", page.skipped)) }
                        }
                        .textCase(nil)
                    }
                }
            }
            .overlay { if page == nil && loading { ProgressView() } }
            .searchable(text: $query, prompt: L.t("history_search"))
            .refreshable { refreshToken += 1 }
            .navigationTitle(L.t("history_title"))
            .navigationBarTitleDisplayMode(.inline)
            .navigationDestination(for: HistoryEntry.self) { HistoryConversationView(entry: $0) }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(L.t("ios_done")) { dismiss() } }
                ToolbarItem(placement: .primaryAction) { filterMenu }
            }
            .task(id: "\(query)|\(agent)|\(workspace)|\(refreshToken)") { await reload(force: refreshToken > 0) }
        }
    }

    private var filterMenu: some View {
        Menu {
            Picker(L.t("history_filter_agent"), selection: $agent) {
                Text(L.t("history_all_agents")).tag("")
                ForEach(page?.agents ?? [], id: \.id) { Text($0.name).tag($0.id) }
            }
            .pickerStyle(.menu)
            Picker(L.t("history_filter_workspace"), selection: $workspace) {
                Text(L.t("history_all_workspaces")).tag("")
                ForEach(page?.workspaces ?? [], id: \.self) { Text($0).tag($0) }
            }
            .pickerStyle(.menu)
            if filterCount > 0 {
                Button(L.t("history_filter_reset"), role: .destructive) { agent = ""; workspace = "" }
            }
        } label: {
            Label(filterCount == 0 ? L.t("history_filter") : L.t("history_filter_count", filterCount),
                  systemImage: filterCount == 0 ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
        }
    }

    private func row(_ entry: HistoryEntry) -> some View {
        NavigationLink(value: entry) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    AgentBrandIcon(agentId: entry.agentId)
                    Text(historyTitle(entry)).font(.headline).lineLimit(2)
                }
                Text(entry.description ?? L.t("history_no_preview")).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                HistoryMetadata(entry: entry, compact: true)
                if entry.terminalSession == nil, let reason = entry.unavailableReason {
                    Text(historyErrorText(reason)).font(.caption).foregroundStyle(.orange)
                }
            }
            .padding(.vertical, 2)
        }
        .swipeActions(edge: .trailing) {
            if entry.terminalSession != nil || entry.canResume {
                Button { resume(entry) } label: {
                    Label(L.t(entry.terminalSession != nil ? "history_open_terminal" : "history_resume"), systemImage: "play.fill")
                }
                .tint(.accentColor)
            }
        }
        .contextMenu {
            Button { resume(entry) } label: {
                Label(L.t(entry.terminalSession != nil ? "history_open_terminal" : "history_resume"), systemImage: "play.fill")
            }
            .disabled(!(entry.terminalSession != nil || entry.canResume) || working || model.reconnecting)
            Button { UIPasteboard.general.string = entry.sessionId } label: { Label("ID", systemImage: "doc.on.doc") }
        }
    }

    private func reload(force: Bool) async {
        // Debounce typing in the search field.
        try? await Task.sleep(for: .milliseconds(250))
        guard !Task.isCancelled else { return }
        loading = true
        error = nil
        defer { if !Task.isCancelled { loading = false } }
        var refresh = force
        do {
            while !Task.isCancelled {
                let next = try await model.loadHistory(query: query, agent: agent, offset: 0, refresh: refresh, workspace: workspace)
                refresh = false
                guard !Task.isCancelled else { return }
                page = next
                if !next.indexing { break }
                try await Task.sleep(for: .milliseconds(500))
            }
        } catch is CancellationError {
        } catch {
            if !Task.isCancelled { self.error = (error as? HistoryFailure)?.reason ?? "unreadable" }
        }
    }

    private func loadMore(_ offset: Int) {
        loading = true
        Task {
            defer { loading = false }
            do {
                let next = try await model.loadHistory(query: query, agent: agent, offset: offset, refresh: false, workspace: workspace)
                var merged = next
                var seen = Set<String>()
                merged.sessions = ((page?.sessions ?? []) + next.sessions).filter { seen.insert($0.id).inserted }
                page = merged
            } catch {
                self.error = (error as? HistoryFailure)?.reason ?? "unreadable"
            }
        }
    }

    private func resume(_ entry: HistoryEntry) {
        guard !working else { return }
        working = true
        error = nil
        Task {
            defer { working = false }
            do {
                try await model.resumeHistory(entry)
                dismiss()
            } catch {
                self.error = (error as? HistoryFailure)?.reason ?? "unreadable"
            }
        }
    }
}

private struct HistoryConversationView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let entry: HistoryEntry
    @State private var messages: [ChatMessage] = []
    @State private var loading = true
    @State private var error: String?
    @State private var detailsOpen = false

    var body: some View {
        VStack(spacing: 0) {
            if detailsOpen {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        AgentBrandIcon(agentId: entry.agentId)
                        Text(historyTitle(entry)).font(.subheadline.weight(.semibold))
                    }
                    HistoryMetadata(entry: entry, compact: false)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
                .background(.bar)
            }
            if let error {
                ContentUnavailableView(historyErrorText(error), systemImage: "exclamationmark.triangle")
            } else {
                ChatView(chat: ChatUiState(messages: messages, loading: loading), onOpenTerminal: { dismiss() })
            }
        }
        .navigationTitle(historyTitle(entry))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                HStack(spacing: 8) {
                    AgentBrandIcon(agentId: entry.agentId)
                    Text(historyTitle(entry)).font(.headline).lineLimit(1)
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Button { withAnimation { detailsOpen.toggle() } } label: {
                    Label(L.t("history_details"), systemImage: detailsOpen ? "info.circle.fill" : "info.circle")
                }
            }
        }
        .task(id: entry.id) {
            do { messages = try await model.previewHistory(entry) } catch {
                self.error = (error as? HistoryFailure)?.reason ?? "unreadable"
            }
            loading = false
        }
    }
}

private struct HistoryMetadata: View {
    let entry: HistoryEntry
    let compact: Bool

    var body: some View {
        let date = Date(timeIntervalSince1970: TimeInterval(entry.updatedAt))
        Text("\(entry.agentName) · \(date.formatted(date: .abbreviated, time: .shortened)) · \(entry.model ?? L.t("history_unknown_model"))")
            .font(.caption).foregroundStyle(.secondary)
        Text(entry.cwd ?? L.t("history_unknown_directory")).font(.caption).foregroundStyle(.secondary).lineLimit(compact ? 1 : nil)
        if !compact { Text(entry.sessionId).font(.caption2.monospaced()).foregroundStyle(.secondary).textSelection(.enabled) }
    }
}

private func historyTitle(_ entry: HistoryEntry) -> String {
    !entry.title.trimmingCharacters(in: .whitespaces).isEmpty && entry.title != entry.sessionId
        ? entry.title : L.t("history_untitled", entry.agentName)
}

private func historyErrorText(_ reason: String) -> String {
    switch reason {
    case "disabled": L.t("history_disabled")
    case "missing-directory": L.t("history_missing_directory")
    case "missing-cli": L.t("history_missing_cli")
    case "not-found": L.t("history_not_found")
    case "unsupported-agent": L.t("history_unsupported")
    case "spawn-failed": L.t("history_spawn_failed")
    default: L.t("history_unreadable")
    }
}
