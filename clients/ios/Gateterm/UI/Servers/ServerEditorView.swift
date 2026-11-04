import SwiftUI

/// Add/edit a server as a modal form: Cancel on the left, Save on the right (iOS convention),
/// instead of Android's full-screen editor with a bottom Save button.
struct ServerEditorView: View {
    @Environment(AppModel.self) private var model
    @State private var cachedState = EditorState()
    @State private var tokenVisible = false
    @State private var scanning = false
    @FocusState private var focused: Field?

    private enum Field { case url, token, name }

    var body: some View {
        let _ = model.localeRevision
        let state = model.editor ?? cachedState
        let binding = { (keyPath: WritableKeyPath<EditorState, String>) in
            Binding<String>(get: { state[keyPath: keyPath] }, set: { value in
                guard var next = model.editor, next[keyPath: keyPath] != value else { return }
                next[keyPath: keyPath] = value
                model.editorChanged(next)
            })
        }
        NavigationStack {
            Form {
                Section {
                    Button { scanning = true } label: { Label(L.t("servers_add_scan"), systemImage: "qrcode.viewfinder") }
                    if state.clipboardURL != nil {
                        Button { model.pasteClipboardIntoEditor() } label: {
                            Label(L.t("editor_paste_clipboard"), systemImage: "doc.on.clipboard")
                        }
                    }
                }
                Section {
                    TextField(L.t("editor_url"), text: binding(\.url), prompt: Text(verbatim: "http://192.168.1.10:7683/#token=…"))
                        .keyboardType(.URL)
                        .textContentType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($focused, equals: .url)
                        .submitLabel(.next)
                        .onSubmit { focused = .token }
                } header: {
                    Text(L.t("editor_url"))
                } footer: {
                    Text(L.t(state.urlError == .emptyURL ? "editor_url_empty" : state.urlError == .badURL ? "editor_url_bad" : "editor_url_hint"))
                        .foregroundStyle(state.urlError == nil ? Color.secondary : Color.red)
                }
                Section {
                    HStack {
                        Group {
                            if tokenVisible {
                                TextField(L.t("editor_token"), text: binding(\.token))
                            } else {
                                SecureField(L.t("editor_token"), text: binding(\.token))
                            }
                        }
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .font(.body.monospaced())
                        .focused($focused, equals: .token)
                        .submitLabel(.next)
                        .onSubmit { focused = .name }
                        Button { tokenVisible.toggle() } label: { Image(systemName: tokenVisible ? "eye.slash" : "eye") }
                            .buttonStyle(.borderless)
                            .accessibilityLabel(L.t(tokenVisible ? "editor_token_hide" : "editor_token_show"))
                    }
                } header: {
                    Text(L.t("editor_token"))
                } footer: {
                    Text(L.t(state.tokenError == .emptyToken ? "editor_token_empty" : "editor_token_hint"))
                        .foregroundStyle(state.tokenError == nil ? Color.secondary : Color.red)
                }
                Section {
                    TextField(L.t("editor_name"), text: binding(\.name))
                        .focused($focused, equals: .name)
                        .submitLabel(.done)
                        .onSubmit(save)
                } header: {
                    Text(L.t("editor_name"))
                } footer: {
                    Text(L.t("editor_name_hint"))
                }
                if state.loopbackWarning || state.wildcardWarning {
                    Section {
                        if state.loopbackWarning { Label(L.t("editor_warning_loopback"), systemImage: "exclamationmark.triangle") }
                        if state.wildcardWarning { Label(L.t("editor_warning_wildcard"), systemImage: "exclamationmark.triangle") }
                    }
                    .foregroundStyle(.orange)
                }
            }
            .navigationTitle(L.t(state.connectionId == nil ? "editor_title_add" : "editor_title_edit"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(L.t("action_cancel")) {
                        focused = nil
                        model.editor = nil
                    }
                }
                ToolbarItem(placement: .confirmationAction) { Button(L.t("action_save"), action: save).bold() }
            }
            .onChange(of: model.editor, initial: true) { _, new in
                if let new { cachedState = new }
            }
            .onAppear {
                if state.scanOnOpen {
                    model.editor?.scanOnOpen = false
                    scanning = true
                } else if state.url.isEmpty {
                    focused = .url
                }
            }
            .sheet(isPresented: $scanning) {
                QRScannerSheet { scanned in
                    scanning = false
                    model.applyScanned(scanned)
                } onManual: {
                    scanning = false
                    focused = .url
                }
            }
        }
        .interactiveDismissDisabled(state.url != "" || state.token != "")
    }

    private func save() {
        if model.saveEditor() {
            focused = nil
        } else {
            focused = model.editor?.tokenError != nil ? .token : .url
        }
    }
}
