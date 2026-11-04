import SwiftUI

enum AppSheet: Identifiable {
    case settings, cloud
    var id: Self { self }
}

/// Servers → sessions → terminal. On iPad (regular width) this is a three-column split view
/// with the server list as the sidebar; on iPhone it collapses into a navigation stack with the
/// usual back-swipe, matching the Android flow.
struct RootView: View {
    @Environment(AppModel.self) private var model
    @Environment(CloudAccountModel.self) private var cloud
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.locale) private var systemLocale
    /// Start with the server list and sessions visible (portrait iPad would otherwise open on an
    /// empty detail placeholder); hide the server list once a terminal is open.
    @State private var columns: NavigationSplitViewVisibility = .all
    @State private var sheet: AppSheet?

    var body: some View {
        @Bindable var model = model
        let _ = model.localeRevision
        NavigationSplitView(columnVisibility: $columns) {
            ServerListView(sheet: $sheet)
                .navigationSplitViewColumnWidth(min: 260, ideal: 300, max: 360)
        } content: {
            Group {
                if let connection = model.selectedConnection {
                    SessionListView(connection: connection)
                } else {
                    ContentUnavailableView(L.t("ios_select_server"), systemImage: "server.rack",
                                           description: Text(L.t("ios_select_server_body")))
                }
            }
            .navigationSplitViewColumnWidth(min: 280, ideal: 340, max: 420)
        } detail: {
            NavigationStack {
                if let id = model.attachedId {
                    TerminalScreen(sessionId: id)
                } else {
                    ContentUnavailableView(L.t("ios_select_session"), systemImage: "terminal",
                                           description: Text(L.t("ios_select_session_body")))
                }
            }
        }
        .tint(Color(rgb: model.appearance.palette.accent))
        .onChange(of: model.attachedId) { old, new in
            // Give the terminal room on iPad: hide the server sidebar once a session opens.
            guard sizeClass == .regular else { return }
            if old == nil && new != nil { withAnimation { columns = .doubleColumn } }
            if old != nil && new == nil { withAnimation { columns = .all } }
        }
        .onChange(of: colorScheme, initial: true) { _, scheme in model.systemThemeChanged(prefersDark: scheme == .dark) }
        .onChange(of: systemLocale, initial: true) { _, _ in model.systemLocaleChanged() }
        .onReceive(NotificationCenter.default.publisher(for: NSLocale.currentLocaleDidChangeNotification)) { _ in
            model.systemLocaleChanged()
        }
        .sheet(item: $sheet) { sheet in
            switch sheet {
            case .settings: SettingsView()
            case .cloud: CloudAccountView()
            }
        }
        .sheet(isPresented: Binding(get: { model.editor != nil }, set: { if !$0 { model.editor = nil } })) {
            ServerEditorView()
        }
        .overlay(alignment: .top) { ToastOverlay() }
        .environment(\.locale, Locale(identifier: L.locale.rawValue))
    }
}

struct ToastOverlay: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let toast = model.toast {
            HStack(spacing: 8) {
                Image(systemName: toast.message.isError ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .foregroundStyle(toast.message.isError ? .orange : .green)
                Text(toast.message.text).font(.subheadline).multilineTextAlignment(.leading)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(.regularMaterial, in: Capsule())
            .shadow(color: .black.opacity(0.15), radius: 10, y: 4)
            .padding(.top, 8)
            .padding(.horizontal, 24)
            .transition(.move(edge: .top).combined(with: .opacity))
            .onTapGesture { model.dismissToast() }
            .task(id: toast.id) {
                UINotificationFeedbackGenerator().notificationOccurred(toast.message.isError ? .error : .success)
                try? await Task.sleep(for: .seconds(toast.message.isError ? 4 : 2))
                withAnimation { model.dismissToast() }
            }
            .accessibilityAddTraits(.isStaticText)
        }
    }
}
