import Foundation

final class AppearanceStore {
    private let defaults: UserDefaults
    private enum Key {
        static let json = "gateterm.appearance.json"
        static let followSystem = "gateterm.appearance.follow-system"
        static let extraKeyRows = "gateterm.appearance.extra-key-rows"
        static let agentLayouts = "gateterm.appearance.agent-key-layouts"
    }

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    var followsSystem: Bool {
        defaults.object(forKey: Key.followSystem) as? Bool ?? (defaults.string(forKey: Key.json) == nil)
    }

    var extraKeyRows: Int {
        get { let v = defaults.integer(forKey: Key.extraKeyRows); return (1...2).contains(v) ? v : 1 }
        set { defaults.set(min(2, max(1, newValue)), forKey: Key.extraKeyRows) }
    }

    var agentKeyLayouts: [String: AgentKeyLayout] {
        get { AgentKeyboardLayouts.decode(defaults.string(forKey: Key.agentLayouts)) }
        set { defaults.set(AgentKeyboardLayouts.encode(newValue), forKey: Key.agentLayouts) }
    }

    func load(prefersDark: Bool) -> TermAppearance {
        let base = AppearanceCodec.default(prefersDark: prefersDark)
        let saved = defaults.string(forKey: Key.json).flatMap { AppearanceCodec.importJSON($0, base: base) } ?? base
        return followsSystem ? saved.withTheme(base) : saved
    }

    func save(_ appearance: TermAppearance, followsSystem: Bool) {
        defaults.set(AppearanceCodec.exportJSON(appearance), forKey: Key.json)
        defaults.set(followsSystem, forKey: Key.followSystem)
    }
}
