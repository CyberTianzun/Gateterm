import Foundation

enum KeyboardShortcut: String, CaseIterable, Identifiable {
    case keyboard, escape, tab, ctrl, alt, shift, up, down, left, right, enter
    case ctrlC = "ctrl-c", home, end, pageUp = "page-up", pageDown = "page-down", backspace, delete, insert
    case shiftEnter = "shift-enter", ctrlJ = "ctrl-j", altUp = "alt-up", paste, copy, jumpBottom = "jump-bottom"
    case f1, f2, f3, f4, f5, f6, f7, f8, f9, f10, f11, f12

    var id: String { rawValue }
    var wire: String { rawValue }

    var label: String {
        switch self {
        case .keyboard: "⌨"
        case .escape: "Esc"
        case .tab: "⇥"
        case .ctrl: "⌃"
        case .alt: "⌥"
        case .shift: "⇧"
        case .up: "↑"
        case .down: "↓"
        case .left: "←"
        case .right: "→"
        case .enter: "↵"
        case .ctrlC: "⌃C"
        case .home: "Hm"
        case .end: "End"
        case .pageUp: "P↑"
        case .pageDown: "P↓"
        case .backspace: "⌫"
        case .delete: "Del"
        case .insert: "Ins"
        case .shiftEnter: "⇧↵"
        case .ctrlJ: "⌃J"
        case .altUp: "⌥↑"
        case .paste, .copy: ""
        case .jumpBottom: "⤓"
        default: rawValue.uppercased()
        }
    }

    var englishDescription: String {
        switch self {
        case .keyboard: "Keyboard"
        case .escape: "Escape"
        case .ctrlC: "Ctrl+C"
        case .pageUp: "Page Up"
        case .pageDown: "Page Down"
        case .shiftEnter: "Shift+Enter"
        case .ctrlJ: "Ctrl+J"
        case .altUp: "Alt+Up"
        case .jumpBottom: "Jump to bottom"
        default: rawValue.prefix(1).uppercased() + rawValue.dropFirst()
        }
    }

    var stroke: KeyStroke? {
        let ctrl = KeyModifiers(ctrl: true)
        switch self {
        case .escape: return KeyStroke(.escape)
        case .tab: return KeyStroke(.tab)
        case .up: return KeyStroke(.up)
        case .down: return KeyStroke(.down)
        case .left: return KeyStroke(.left)
        case .right: return KeyStroke(.right)
        case .enter: return KeyStroke(.enter)
        case .ctrlC: return KeyStroke(.c, modifiers: ctrl)
        case .home: return KeyStroke(.home)
        case .end: return KeyStroke(.end)
        case .pageUp: return KeyStroke(.pageUp)
        case .pageDown: return KeyStroke(.pageDown)
        case .backspace: return KeyStroke(.backspace)
        case .delete: return KeyStroke(.delete)
        case .insert: return KeyStroke(.insert)
        case .shiftEnter: return KeyStroke(.enter, modifiers: KeyModifiers(shift: true))
        case .ctrlJ: return KeyStroke(.j, modifiers: ctrl)
        case .altUp: return KeyStroke(.up, modifiers: KeyModifiers(alt: true))
        case .f1, .f2, .f3, .f4, .f5, .f6, .f7, .f8, .f9, .f10, .f11, .f12:
            return KeyStroke(TerminalKey.functionKeys[Int(rawValue.dropFirst())! - 1])
        case .keyboard, .ctrl, .alt, .shift, .paste, .copy, .jumpBottom: return nil
        }
    }
}

enum AgentMultilineKey: String, CaseIterable, Identifiable {
    case ctrlJ = "ctrl-j", shiftEnter = "shift-enter", both
    var id: String { rawValue }
    var shortcuts: [KeyboardShortcut] {
        switch self {
        case .ctrlJ: [.ctrlJ]
        case .shiftEnter: [.shiftEnter]
        case .both: [.ctrlJ, .shiftEnter]
        }
    }
}

struct AgentKeyLayout: Equatable {
    var enabled = true
    var multiline: AgentMultilineKey = .both
    var priority: [KeyboardShortcut] = AgentKeyboardLayouts.defaultPriority
}

/// Same JSON shape and ordering rules as Android `AgentKeyboardLayouts`.
enum AgentKeyboardLayouts {
    static let agents: [(id: String, name: String)] = [
        ("claude-code", "Claude Code"), ("codex", "Codex"), ("pi", "pi"), ("grok", "Grok"), ("antigravity", "Antigravity"),
    ]
    static let defaultPriority: [KeyboardShortcut] = [.keyboard, .ctrlC, .escape, .tab, .up, .down, .left, .right]
    static let priorityOptions: [KeyboardShortcut] = defaultPriority + [
        .enter, .ctrl, .alt, .shift, .paste, .home, .end, .altUp, .backspace, .delete, .insert,
    ]

    static func agentForTerminal(_ agents: [String: AgentSession], _ terminalId: Int64) -> String? {
        guard let agent = AgentSessions.forSession(agents, terminalId), agent.state != .done else { return nil }
        return agent.agentId
    }

    static func order(agentId: String?, profiles: [String: AgentKeyLayout]) -> [KeyboardShortcut] {
        let terminal = KeyboardShortcut.allCases
        guard let agentId else { return terminal }
        let profile = profiles[agentId] ?? AgentKeyLayout()
        guard profile.enabled else { return terminal }
        var seen = Set<KeyboardShortcut>()
        return ([.pageUp, .pageDown] + profile.multiline.shortcuts + profile.priority.filter(priorityOptions.contains) + terminal)
            .filter { seen.insert($0).inserted }
    }

    static func decode(_ json: String?) -> [String: AgentKeyLayout] {
        guard let json, json.count <= 128 * 1024, let root = JSON(string: json) else { return [:] }
        var out: [String: AgentKeyLayout] = [:]
        for id in root.raw.keys.sorted().prefix(64) where !id.trimmingCharacters(in: .whitespaces).isEmpty && id.count <= 128 {
            guard let row = root.object(id) else { continue }
            var priority = defaultPriority
            if let choices = row.array("priority") {
                var seen = Set<KeyboardShortcut>()
                priority = choices.prefix(64).compactMap { ($0 as? String).flatMap(KeyboardShortcut.init(rawValue:)) }
                    .filter { priorityOptions.contains($0) && seen.insert($0).inserted }
            }
            out[id] = AgentKeyLayout(enabled: row.bool("enabled", default: true),
                                     multiline: AgentMultilineKey(rawValue: row.string("multiline")) ?? .both, priority: priority)
        }
        return out
    }

    static func encode(_ profiles: [String: AgentKeyLayout]) -> String {
        let obj = profiles.mapValues { ["enabled": $0.enabled, "multiline": $0.multiline.rawValue, "priority": $0.priority.map(\.wire)] as [String: Any] }
        let data = (try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])) ?? Data("{}".utf8)
        return String(decoding: data, as: UTF8.self)
    }
}
