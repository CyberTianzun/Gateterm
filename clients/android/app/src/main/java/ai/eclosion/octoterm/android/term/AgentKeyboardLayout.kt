package ai.eclosion.octoterm.android.term

import ai.eclosion.octoterm.android.agent.AgentSession
import ai.eclosion.octoterm.android.agent.AgentSessions
import ai.eclosion.octoterm.android.agent.AgentState
import org.json.JSONArray
import org.json.JSONObject

enum class KeyboardShortcut(
    val wire: String,
    val label: String,
    val description: String,
    private val key: TerminalKey? = null,
    private val modifiers: KeyModifiers = KeyModifiers(),
) {
    Keyboard("keyboard", "⌨", "Keyboard"),
    Escape("escape", "Esc", "Escape", TerminalKey.Escape),
    Tab("tab", "⇥", "Tab", TerminalKey.Tab),
    Ctrl("ctrl", "⌃", "Ctrl"), Alt("alt", "⌥", "Alt"), Shift("shift", "⇧", "Shift"),
    Up("up", "↑", "Up", TerminalKey.Up), Down("down", "↓", "Down", TerminalKey.Down),
    Left("left", "←", "Left", TerminalKey.Left), Right("right", "→", "Right", TerminalKey.Right),
    Enter("enter", "↵", "Enter", TerminalKey.Enter),
    CtrlC("ctrl-c", "⌃C", "Ctrl+C", TerminalKey.C, KeyModifiers(ctrl = true)),
    Home("home", "Hm", "Home", TerminalKey.Home), End("end", "End", "End", TerminalKey.End),
    PageUp("page-up", "P↑", "Page Up", TerminalKey.PageUp), PageDown("page-down", "P↓", "Page Down", TerminalKey.PageDown),
    Backspace("backspace", "⌫", "Backspace", TerminalKey.Backspace),
    Delete("delete", "Del", "Delete", TerminalKey.Delete), Insert("insert", "Ins", "Insert", TerminalKey.Insert),
    ShiftEnter("shift-enter", "⇧↵", "Shift+Enter", TerminalKey.Enter, KeyModifiers(shift = true)),
    CtrlJ("ctrl-j", "⌃J", "Ctrl+J", TerminalKey.J, KeyModifiers(ctrl = true)),
    AltUp("alt-up", "⌥↑", "Alt+Up", TerminalKey.Up, KeyModifiers(alt = true)),
    Paste("paste", "", "Paste"), Copy("copy", "", "Copy"), JumpBottom("jump-bottom", "⤓", "Jump to bottom"),
    F1("f1", "F1", "F1", TerminalKey.F1), F2("f2", "F2", "F2", TerminalKey.F2),
    F3("f3", "F3", "F3", TerminalKey.F3), F4("f4", "F4", "F4", TerminalKey.F4),
    F5("f5", "F5", "F5", TerminalKey.F5), F6("f6", "F6", "F6", TerminalKey.F6),
    F7("f7", "F7", "F7", TerminalKey.F7), F8("f8", "F8", "F8", TerminalKey.F8),
    F9("f9", "F9", "F9", TerminalKey.F9), F10("f10", "F10", "F10", TerminalKey.F10),
    F11("f11", "F11", "F11", TerminalKey.F11), F12("f12", "F12", "F12", TerminalKey.F12);

    val stroke: KeyStroke? get() = key?.let { KeyStroke(it, modifiers = modifiers) }
}

enum class AgentMultilineKey(val wire: String, val shortcuts: List<KeyboardShortcut>) {
    CtrlJ("ctrl-j", listOf(KeyboardShortcut.CtrlJ)),
    ShiftEnter("shift-enter", listOf(KeyboardShortcut.ShiftEnter)),
    Both("both", listOf(KeyboardShortcut.CtrlJ, KeyboardShortcut.ShiftEnter)),
}

data class AgentKeyLayout(
    val enabled: Boolean = true,
    val multiline: AgentMultilineKey = AgentMultilineKey.Both,
    val priority: List<KeyboardShortcut> = AgentKeyboardLayouts.defaultPriority,
)

object AgentKeyboardLayouts {
    val agents = linkedMapOf(
        "claude-code" to "Claude Code", "codex" to "Codex", "pi" to "pi",
        "grok" to "Grok", "antigravity" to "Antigravity",
    )
    val defaultPriority = listOf(
        KeyboardShortcut.Keyboard, KeyboardShortcut.CtrlC, KeyboardShortcut.Escape, KeyboardShortcut.Tab,
        KeyboardShortcut.Up, KeyboardShortcut.Down, KeyboardShortcut.Left, KeyboardShortcut.Right,
    )
    val priorityOptions = defaultPriority + listOf(
        KeyboardShortcut.Enter, KeyboardShortcut.Ctrl, KeyboardShortcut.Alt, KeyboardShortcut.Shift,
        KeyboardShortcut.Paste, KeyboardShortcut.Home, KeyboardShortcut.End, KeyboardShortcut.AltUp,
        KeyboardShortcut.Backspace, KeyboardShortcut.Delete, KeyboardShortcut.Insert,
    )

    fun agentForTerminal(agents: Map<String, AgentSession>, terminalId: Long): String? =
        AgentSessions.forSession(agents, terminalId)?.takeIf { it.state != AgentState.Done }?.agentId

    fun order(agentId: String?, profiles: Map<String, AgentKeyLayout>): List<KeyboardShortcut> {
        val terminal = KeyboardShortcut.entries.toList()
        if (agentId == null) return terminal
        val profile = profiles[agentId] ?: AgentKeyLayout()
        if (!profile.enabled) return terminal
        return (listOf(KeyboardShortcut.PageUp, KeyboardShortcut.PageDown) + profile.multiline.shortcuts +
            profile.priority.filter { it in priorityOptions } + terminal).distinct()
    }

    fun decode(json: String?): Map<String, AgentKeyLayout> {
        if (json == null || json.length > 128 * 1024) return emptyMap()
        return try {
            val root = JSONObject(json)
            buildMap {
                root.keys().asSequence().take(64).forEach { id ->
                    if (id.isBlank() || id.length > 128) return@forEach
                    val row = root.optJSONObject(id) ?: return@forEach
                    val choices = row.optJSONArray("priority")
                    val priority = if (choices == null) defaultPriority else buildList {
                        for (index in 0 until choices.length().coerceAtMost(64)) {
                            KeyboardShortcut.entries.find { it.wire == choices.optString(index) && it in priorityOptions }?.let(::add)
                        }
                    }.distinct()
                    put(id, AgentKeyLayout(
                        enabled = row.optBoolean("enabled", true),
                        multiline = AgentMultilineKey.entries.find { it.wire == row.optString("multiline") } ?: AgentMultilineKey.Both,
                        priority = priority,
                    ))
                }
            }
        } catch (_: Exception) { emptyMap() }
    }

    fun encode(profiles: Map<String, AgentKeyLayout>): String = JSONObject().apply {
        profiles.forEach { (id, profile) ->
            put(id, JSONObject().put("enabled", profile.enabled).put("multiline", profile.multiline.wire)
                .put("priority", JSONArray(profile.priority.map { it.wire })))
        }
    }.toString()
}
