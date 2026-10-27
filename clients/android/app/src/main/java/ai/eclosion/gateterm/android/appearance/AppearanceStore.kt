package ai.eclosion.gateterm.android.appearance

import android.content.Context
import ai.eclosion.gateterm.android.term.AgentKeyLayout
import ai.eclosion.gateterm.android.term.AgentKeyboardLayouts

class AppearanceStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun followsSystem(): Boolean = prefs.getBoolean(KEY_FOLLOW_SYSTEM, !prefs.contains(KEY))

    fun extraKeyRows(): Int = prefs.getInt(KEY_EXTRA_KEY_ROWS, 2).takeIf { it in 1..2 } ?: 2

    fun saveExtraKeyRows(rows: Int) {
        prefs.edit().putInt(KEY_EXTRA_KEY_ROWS, rows.coerceIn(1, 2)).apply()
    }

    fun agentKeyLayouts(): Map<String, AgentKeyLayout> = AgentKeyboardLayouts.decode(prefs.getString(KEY_AGENT_LAYOUTS, null))

    fun saveAgentKeyLayouts(layouts: Map<String, AgentKeyLayout>) {
        prefs.edit().putString(KEY_AGENT_LAYOUTS, AgentKeyboardLayouts.encode(layouts)).apply()
    }

    fun load(prefersDark: Boolean): TermAppearance {
        val base = AppearanceCodec.default(prefersDark)
        val saved = prefs.getString(KEY, null)?.let { AppearanceCodec.importJson(it, base) } ?: base
        return if (followsSystem()) saved.withTheme(base) else saved
    }

    fun save(appearance: TermAppearance, followsSystem: Boolean) {
        prefs.edit()
            .putString(KEY, AppearanceCodec.exportJson(appearance))
            .putBoolean(KEY_FOLLOW_SYSTEM, followsSystem)
            .apply()
    }

    private companion object {
        const val PREFS = "gateterm-appearance"
        const val KEY = "json"
        const val KEY_FOLLOW_SYSTEM = "follow-system"
        const val KEY_EXTRA_KEY_ROWS = "extra-key-rows"
        const val KEY_AGENT_LAYOUTS = "agent-key-layouts"
    }
}
