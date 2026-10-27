package ai.eclosion.gateterm.android.ui.appearance

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.term.AgentKeyLayout
import ai.eclosion.gateterm.android.term.AgentKeyboardLayouts
import ai.eclosion.gateterm.android.term.AgentMultilineKey
import ai.eclosion.gateterm.android.term.KeyboardShortcut
import ai.eclosion.gateterm.android.ui.term.shortcutDescription
import ai.eclosion.gateterm.android.ui.term.shortcutLabel

@Composable
internal fun AgentKeyboardSettings(
    layouts: Map<String, AgentKeyLayout>,
    onLayout: (String, AgentKeyLayout) -> Unit,
) {
    var selectedAgent by rememberSaveable { mutableStateOf("claude-code") }
    var menuOpen by remember { mutableStateOf(false) }
    var editing by rememberSaveable { mutableStateOf(false) }
    val agents = AgentKeyboardLayouts.agents + layouts.keys.associateWith { AgentKeyboardLayouts.agents[it] ?: it }
    val profile = layouts[selectedAgent] ?: AgentKeyLayout()
    val priority = profile.priority.filter { it in AgentKeyboardLayouts.priorityOptions }.distinct()
    Row(Modifier.fillMaxWidth().padding(top = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(stringResource(R.string.settings_agent_keyboard), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
        TextButton(onClick = { editing = !editing }) {
            Text(stringResource(if (editing) R.string.chat_show_less else R.string.settings_agent_configure))
        }
    }
    Text(stringResource(R.string.settings_agent_keyboard_hint), style = MaterialTheme.typography.bodySmall)
    if (!editing) return
    Box {
        TextButton(onClick = { menuOpen = true }) { Text("${agents[selectedAgent] ?: selectedAgent} ▾") }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            agents.forEach { (id, name) ->
                DropdownMenuItem(text = { Text(name) }, onClick = { selectedAgent = id; menuOpen = false })
            }
        }
    }
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilterChip(selected = profile.enabled, onClick = { onLayout(selectedAgent, profile.copy(enabled = true)) },
            label = { Text(stringResource(R.string.settings_agent_layout)) })
        FilterChip(selected = !profile.enabled, onClick = { onLayout(selectedAgent, profile.copy(enabled = false)) },
            label = { Text(stringResource(R.string.settings_terminal_layout)) })
    }
    if (profile.enabled) {
        Text(stringResource(R.string.settings_agent_multiline), style = MaterialTheme.typography.titleSmall)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            AgentMultilineKey.entries.forEach { choice ->
                FilterChip(
                    selected = profile.multiline == choice,
                    onClick = { onLayout(selectedAgent, profile.copy(multiline = choice)) },
                    label = { Text(when (choice) {
                        AgentMultilineKey.CtrlJ -> "⌃J (Ctrl+J)"
                        AgentMultilineKey.ShiftEnter -> "⇧↵ (Shift+Enter)"
                        AgentMultilineKey.Both -> stringResource(R.string.settings_agent_both)
                    }) },
                )
            }
        }
        val pinned = (listOf(KeyboardShortcut.PageUp, KeyboardShortcut.PageDown) + profile.multiline.shortcuts).joinToString(" / ") { it.label }
        Text(stringResource(R.string.settings_agent_pinned, pinned), style = MaterialTheme.typography.bodySmall)
        Text(stringResource(R.string.settings_agent_priority), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 12.dp))
        priority.forEachIndexed { index, shortcut ->
            key(shortcut) {
                val description = shortcutDescription(shortcut)
                val up = stringResource(R.string.settings_agent_move_up, description)
                val down = stringResource(R.string.settings_agent_move_down, description)
                val remove = stringResource(R.string.settings_agent_remove, description)
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text("${shortcutLabel(shortcut)} · $description", modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                    IconButton(
                        enabled = index > 0,
                        onClick = {
                            val next = priority.toMutableList()
                            java.util.Collections.swap(next, index, index - 1)
                            onLayout(selectedAgent, profile.copy(priority = next))
                        }, modifier = Modifier.semantics { contentDescription = up },
                    ) { Text("↑") }
                    IconButton(
                        enabled = index < priority.lastIndex,
                        onClick = {
                            val next = priority.toMutableList()
                            java.util.Collections.swap(next, index, index + 1)
                            onLayout(selectedAgent, profile.copy(priority = next))
                        }, modifier = Modifier.semantics { contentDescription = down },
                    ) { Text("↓") }
                    IconButton(
                        onClick = { onLayout(selectedAgent, profile.copy(priority = priority - shortcut)) },
                        modifier = Modifier.semantics { contentDescription = remove },
                    ) { Text("×") }
                }
            }
        }
        Text(stringResource(R.string.settings_agent_add), style = MaterialTheme.typography.labelLarge)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            AgentKeyboardLayouts.priorityOptions.filterNot { it in priority }.forEach { shortcut ->
                val description = shortcutDescription(shortcut)
                FilterChip(
                    selected = false,
                    onClick = { onLayout(selectedAgent, profile.copy(priority = priority + shortcut)) },
                    label = { Text("+ ${shortcutLabel(shortcut)}") },
                    modifier = Modifier.semantics { contentDescription = description },
                )
            }
        }
    }
    TextButton(onClick = { onLayout(selectedAgent, AgentKeyLayout()) }) {
        Text(stringResource(R.string.settings_agent_reset, agents[selectedAgent] ?: selectedAgent))
    }
}
