package ai.eclosion.gateterm.android.term

import ai.eclosion.gateterm.android.agent.AgentSession
import ai.eclosion.gateterm.android.agent.AgentState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class AgentKeyboardLayoutTest {
    @Test
    fun agentLayoutPinsPagingAndBothMultilineKeysBeforeOtherActions() {
        for (agent in AgentKeyboardLayouts.agents.keys + "future-agent") {
            val order = AgentKeyboardLayouts.order(agent, emptyMap())
            assertEquals(listOf(KeyboardShortcut.PageUp, KeyboardShortcut.PageDown, KeyboardShortcut.CtrlJ, KeyboardShortcut.ShiftEnter), order.take(4))
            assertEquals(KeyboardShortcut.entries.toSet(), order.toSet())
            assertEquals(order.size, order.distinct().size)
        }
    }

    @Test
    fun perAgentMultilineAndPrioritySettingsAreIndependent() {
        val profiles = mapOf(
            "claude-code" to AgentKeyLayout(multiline = AgentMultilineKey.CtrlJ, priority = listOf(KeyboardShortcut.Tab, KeyboardShortcut.CtrlC)),
            "codex" to AgentKeyLayout(multiline = AgentMultilineKey.ShiftEnter, priority = listOf(KeyboardShortcut.Escape)),
        )
        val claude = AgentKeyboardLayouts.order("claude-code", profiles)
        val codex = AgentKeyboardLayouts.order("codex", profiles)
        assertEquals(listOf(KeyboardShortcut.PageUp, KeyboardShortcut.PageDown, KeyboardShortcut.CtrlJ, KeyboardShortcut.Tab, KeyboardShortcut.CtrlC), claude.take(5))
        assertEquals(listOf(KeyboardShortcut.PageUp, KeyboardShortcut.PageDown, KeyboardShortcut.ShiftEnter, KeyboardShortcut.Escape), codex.take(4))
        assertEquals(KeyboardShortcut.entries.toSet(), claude.toSet())
        assertEquals(KeyboardShortcut.entries.toSet(), codex.toSet())
    }

    @Test
    fun normalTerminalsAndDisabledProfilesKeepTheTerminalLayout() {
        val profiles = mapOf("codex" to AgentKeyLayout(enabled = false))
        assertEquals(KeyboardShortcut.entries.toList(), AgentKeyboardLayouts.order(null, profiles))
        assertEquals(KeyboardShortcut.entries.toList(), AgentKeyboardLayouts.order("codex", profiles))
    }

    @Test
    fun recognitionUsesOnlyTheCurrentTerminalAndExcludesCompletedAgents() {
        fun agent(id: String, terminal: Long, state: AgentState) = AgentSession(id, "session-$id", terminal, state, null, null, true)
        val codex = agent("codex", 1, AgentState.Working)
        val claude = agent("claude-code", 2, AgentState.Waiting)
        val sessions = mapOf(codex.key to codex, claude.key to claude)
        assertEquals("codex", AgentKeyboardLayouts.agentForTerminal(sessions, 1))
        assertEquals("claude-code", AgentKeyboardLayouts.agentForTerminal(sessions, 2))
        assertNull(AgentKeyboardLayouts.agentForTerminal(sessions, 3))
        assertNull(AgentKeyboardLayouts.agentForTerminal(mapOf(codex.key to codex.copy(state = AgentState.Done)), 1))
        assertNull(AgentKeyboardLayouts.agentForTerminal(emptyMap(), 1))
    }

    @Test
    fun customLayoutsRoundTripAndInvalidPreferenceEntriesAreIgnored() {
        val profiles = mapOf(
            "codex" to AgentKeyLayout(multiline = AgentMultilineKey.CtrlJ, priority = listOf(KeyboardShortcut.Paste, KeyboardShortcut.Tab)),
            "pi" to AgentKeyLayout(enabled = false),
        )
        assertEquals(profiles, AgentKeyboardLayouts.decode(AgentKeyboardLayouts.encode(profiles)))
        assertEquals(emptyMap<String, AgentKeyLayout>(), AgentKeyboardLayouts.decode("broken"))
        val parsed = AgentKeyboardLayouts.decode("""{"codex":{"multiline":"unknown","priority":["tab","unknown","tab","page-up","ctrl-j"]},"pi":{"enabled":false}}""")
        assertEquals(AgentMultilineKey.Both, parsed.getValue("codex").multiline)
        assertEquals(listOf(KeyboardShortcut.Tab), parsed.getValue("codex").priority)
        assertFalse(parsed.getValue("pi").enabled)
    }

    @Test
    fun displayedShortcutsStillEncodeTheOriginalKeyCombinations() {
        val keyboard = TerminalKeyboard("windows", win32InputMode = true)
        assertEquals("\u001b[74;36;10;1;8;1_\u001b[74;36;10;0;8;1_", keyboard.tap(KeyboardShortcut.CtrlJ.stroke!!).toString(Charsets.UTF_8))
        assertEquals("\u001b[13;28;13;1;16;1_\u001b[13;28;13;0;16;1_", keyboard.tap(KeyboardShortcut.ShiftEnter.stroke!!).toString(Charsets.UTF_8))
        assertEquals(TerminalKey.PageUp, KeyboardShortcut.PageUp.stroke!!.key)
        assertEquals(TerminalKey.PageDown, KeyboardShortcut.PageDown.stroke!!.key)
        assertNull(KeyboardShortcut.Paste.stroke)
    }
}
