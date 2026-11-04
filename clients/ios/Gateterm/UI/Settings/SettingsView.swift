import SwiftUI

/// Settings as a grouped form presented modally with a Done button (iOS convention).
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var scrollbackDraft = ""

    var body: some View {
        @Bindable var model = model
        let _ = model.localeRevision
        let a = model.appearance
        NavigationStack {
            Form {
                Section(L.t("ios_settings_general")) {
                    Picker(L.t("locale_action"), selection: $model.localePref) {
                        Text(L.t("locale_auto")).tag(LocalePref.auto)
                        Text(verbatim: "简体中文").tag(LocalePref.zhCN)
                        Text(verbatim: "English").tag(LocalePref.en)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text(L.t("connections_background_title"))
                        Text(L.t("connections_background_hint")).font(.footnote).foregroundStyle(.secondary)
                    }
                }

                Section(L.t("appearance_theme")) {
                    themeRow(L.t("appearance_theme_system", model.systemThemeName), selected: model.appearanceFollowsSystem) {
                        model.followSystemTheme()
                    }
                    ForEach(AppearanceCodec.builtins, id: \.name) { theme in
                        themeRow(theme.name, swatch: theme, selected: !model.appearanceFollowsSystem && theme.name == a.name) {
                            model.selectTheme(theme.name)
                        }
                    }
                }

                Section {
                    Picker(L.t("appearance_font"), selection: Binding(get: { a.font }, set: { model.setFont($0) })) {
                        ForEach(TermFontKind.allCases) { Text($0.displayName).tag($0) }
                    }
                    Stepper(value: Binding(get: { a.fontSize }, set: { model.setFontSize($0) }), in: 6...48, step: 1) {
                        LabeledContent(L.t("ios_font_size_step"), value: "\(Int(a.fontSize)) pt")
                    }
                    VStack(alignment: .leading) {
                        LabeledContent(L.t("appearance_line_height", a.lineHeight), value: "")
                        Slider(value: Binding(get: { a.lineHeight }, set: { model.setLineHeight(($0 * 20).rounded() / 20) }), in: 1...3)
                    }
                    VStack(alignment: .leading) {
                        LabeledContent(L.t("appearance_letter_spacing", a.letterSpacing), value: "")
                        Slider(value: Binding(get: { a.letterSpacing }, set: { model.setLetterSpacing(($0 * 2).rounded() / 2) }), in: -5...10)
                    }
                    LabeledContent(L.t("appearance_scrollback")) {
                        TextField(L.t("appearance_scrollback_hint"), text: $scrollbackDraft)
                            .keyboardType(.numberPad)
                            .multilineTextAlignment(.trailing)
                            .onChange(of: scrollbackDraft) { _, raw in
                                let digits = String(raw.filter(\.isNumber).prefix(6))
                                if digits != raw { scrollbackDraft = digits }
                                if let lines = Int(digits) { model.setScrollback(lines) }
                            }
                    }
                    TerminalPreview(appearance: a).listRowInsets(EdgeInsets())
                } header: {
                    Text(L.t("ios_settings_terminal"))
                } footer: {
                    Text(L.t("appearance_font_size_hint"))
                }

                Section {
                    Picker(L.t("settings_keyboard_rows"), selection: Binding(get: { model.extraKeyRows }, set: { model.setExtraKeyRows($0) })) {
                        Text(L.t("settings_keyboard_one_row")).tag(1)
                        Text(L.t("settings_keyboard_two_rows")).tag(2)
                    }
                    NavigationLink(L.t("settings_agent_keyboard")) { AgentKeyboardSettingsView() }
                } footer: {
                    Text(L.t("settings_agent_keyboard_hint"))
                }

                Section {
                    Button { model.copyAppearance() } label: { Label(L.t("appearance_export"), systemImage: "doc.on.doc") }
                    Button { model.importAppearanceFromClipboard() } label: { Label(L.t("appearance_import"), systemImage: "square.and.arrow.down") }
                } header: {
                    Text(L.t("ios_settings_config"))
                } footer: {
                    Text(L.t("appearance_import_hint"))
                }

                Section {
                    LabeledContent("Gateterm", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
                }
            }
            .navigationTitle(L.t("settings_title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button(L.t("ios_done")) { dismiss() } } }
            .onAppear { scrollbackDraft = String(a.scrollback) }
        }
    }

    private func themeRow(_ title: String, swatch: TermAppearance? = nil, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                if let swatch {
                    HStack(spacing: 0) {
                        ForEach([swatch.background, swatch.ansi[1], swatch.ansi[2], swatch.ansi[4], swatch.foreground], id: \.self) {
                            Color(rgb: $0)
                        }
                    }
                    .frame(width: 44, height: 18)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .overlay(RoundedRectangle(cornerRadius: 4).stroke(.separator))
                } else {
                    Image(systemName: "circle.lefthalf.filled").frame(width: 44)
                }
                Text(title).foregroundStyle(.primary)
                Spacer()
                if selected { Image(systemName: "checkmark").foregroundStyle(Color.accentColor).fontWeight(.semibold) }
            }
        }
    }
}

/// A few lines rendered with the current font, size and colours.
private struct TerminalPreview: View {
    let appearance: TermAppearance

    var body: some View {
        let font = Font(TermFonts.font(appearance.font, size: CGFloat(appearance.fontSize)))
        VStack(alignment: .leading, spacing: CGFloat(appearance.lineHeight - 1) * CGFloat(appearance.fontSize)) {
            Text(verbatim: "~/gateterm $ ls").foregroundStyle(Color(rgb: appearance.foreground))
            HStack(spacing: 0) {
                Text(verbatim: "crates  ").foregroundStyle(Color(rgb: appearance.ansi[4]))
                Text(verbatim: "clients  ").foregroundStyle(Color(rgb: appearance.ansi[4]))
                Text(verbatim: "build.sh").foregroundStyle(Color(rgb: appearance.ansi[2]))
            }
            Text(verbatim: "error: 1 warning").foregroundStyle(Color(rgb: appearance.ansi[1]))
        }
        .font(font)
        .kerning(CGFloat(appearance.letterSpacing))
        .lineLimit(1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Color(rgb: appearance.background))
    }
}

/// Per-agent key bar layout: an editable list (drag to reorder, swipe to remove) plus add buttons.
struct AgentKeyboardSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var agentId = "claude-code"

    var body: some View {
        let agents = AgentKeyboardLayouts.agents + model.agentKeyLayouts.keys.sorted()
            .filter { id in !AgentKeyboardLayouts.agents.contains { $0.id == id } }.map { ($0, $0) }
        let profile = model.agentKeyLayouts[agentId] ?? AgentKeyLayout()
        let priority = profile.priority.filter(AgentKeyboardLayouts.priorityOptions.contains)
        Form {
            Section {
                Picker("Agent", selection: $agentId) {
                    ForEach(agents, id: \.0) { Text($0.1).tag($0.0) }
                }
                Picker(L.t("settings_agent_layout"), selection: Binding(get: { profile.enabled }, set: { enabled in
                    var next = profile; next.enabled = enabled; model.setAgentKeyLayout(agentId, next)
                })) {
                    Text(L.t("settings_agent_layout")).tag(true)
                    Text(L.t("settings_terminal_layout")).tag(false)
                }
                .pickerStyle(.segmented)
            } footer: {
                Text(L.t("settings_agent_keyboard_hint"))
            }
            if profile.enabled {
                Section {
                    Picker(L.t("settings_agent_multiline"), selection: Binding(get: { profile.multiline }, set: { value in
                        var next = profile; next.multiline = value; model.setAgentKeyLayout(agentId, next)
                    })) {
                        Text(verbatim: "⌃J (Ctrl+J)").tag(AgentMultilineKey.ctrlJ)
                        Text(verbatim: "⇧↵ (Shift+Enter)").tag(AgentMultilineKey.shiftEnter)
                        Text(L.t("settings_agent_both")).tag(AgentMultilineKey.both)
                    }
                } footer: {
                    Text(L.t("settings_agent_pinned", ([KeyboardShortcut.pageUp, .pageDown] + profile.multiline.shortcuts).map(\.label).joined(separator: " / ")))
                }
                Section(L.t("settings_agent_priority")) {
                    ForEach(priority) { key in
                        LabeledContent(shortcutDescription(key), value: shortcutLabel(key))
                    }
                    .onMove { from, to in
                        var next = profile; var list = priority
                        list.move(fromOffsets: from, toOffset: to)
                        next.priority = list
                        model.setAgentKeyLayout(agentId, next)
                    }
                    .onDelete { offsets in
                        var next = profile; var list = priority
                        list.remove(atOffsets: offsets)
                        next.priority = list
                        model.setAgentKeyLayout(agentId, next)
                    }
                }
                let remaining = AgentKeyboardLayouts.priorityOptions.filter { !priority.contains($0) }
                if !remaining.isEmpty {
                    Section(L.t("settings_agent_add")) {
                        ForEach(remaining) { key in
                            Button {
                                var next = profile; next.priority = priority + [key]; model.setAgentKeyLayout(agentId, next)
                            } label: {
                                Label("\(shortcutLabel(key)) · \(shortcutDescription(key))", systemImage: "plus.circle.fill")
                            }
                        }
                    }
                }
            }
            Section {
                Button(L.t("settings_agent_reset", agents.first { $0.0 == agentId }?.1 ?? agentId), role: .destructive) {
                    model.setAgentKeyLayout(agentId, AgentKeyLayout())
                }
            }
        }
        .navigationTitle(L.t("settings_agent_keyboard"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { EditButton() }
    }
}
