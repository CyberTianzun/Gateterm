package ai.eclosion.octoterm.android.ui.term

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.term.AgentKeyLayout
import ai.eclosion.octoterm.android.term.AgentKeyboardLayouts
import ai.eclosion.octoterm.android.term.KeyboardShortcut
import ai.eclosion.octoterm.android.term.KeyStroke
import kotlinx.coroutines.launch

@Composable
fun ExtraKeysBar(
    rows: Int,
    sessionId: Long,
    agentId: String?,
    agentLayouts: Map<String, AgentKeyLayout>,
    ctrl: Boolean,
    alt: Boolean,
    shift: Boolean,
    enabled: Boolean,
    copyEnabled: Boolean,
    scrollOffset: Int,
    onCtrl: (Boolean) -> Unit,
    onAlt: (Boolean) -> Unit,
    onShift: (Boolean) -> Unit,
    onKey: (KeyStroke) -> Unit,
    onShowKeyboard: () -> Unit,
    onCopy: () -> Unit,
    onPaste: () -> Unit,
    onJumpToBottom: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val profile = agentId?.let { agentLayouts[it] ?: AgentKeyLayout() }
    val keys = AgentKeyboardLayouts.order(agentId, agentLayouts).mapNotNull { shortcut ->
        if (shortcut == KeyboardShortcut.Copy && !copyEnabled || shortcut == KeyboardShortcut.JumpBottom && scrollOffset <= 0) return@mapNotNull null
        val selected = when (shortcut) {
            KeyboardShortcut.Ctrl -> ctrl
            KeyboardShortcut.Alt -> alt
            KeyboardShortcut.Shift -> shift
            else -> null
        }
        val local = shortcut in listOf(KeyboardShortcut.Keyboard, KeyboardShortcut.Copy, KeyboardShortcut.JumpBottom)
        ExtraKey(shortcut, shortcutLabel(shortcut), local || enabled, shortcutDescription(shortcut), selected) {
            when (shortcut) {
                KeyboardShortcut.Keyboard -> onShowKeyboard()
                KeyboardShortcut.Ctrl -> onCtrl(!ctrl)
                KeyboardShortcut.Alt -> onAlt(!alt)
                KeyboardShortcut.Shift -> onShift(!shift)
                KeyboardShortcut.Paste -> onPaste()
                KeyboardShortcut.Copy -> onCopy()
                KeyboardShortcut.JumpBottom -> onJumpToBottom()
                else -> shortcut.stroke?.let(onKey)
            }
        }
    }
    val scroll = rememberScrollState()
    val scope = rememberCoroutineScope()
    LaunchedEffect(sessionId, agentId, profile, rows) { scroll.scrollTo(0) }
    var pageWidth by remember { mutableIntStateOf(0) }
    Row(modifier.padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        KeyButton(
            "‹", scroll.canScrollBackward, stringResource(R.string.term_keys_previous), width = 20.dp,
        ) { scope.launch { scroll.animateScrollBy(-pageWidth * 0.8f) } }
        Row(
            Modifier.weight(1f)
                .onSizeChanged { pageWidth = it.width }
                .horizontalScroll(scroll)
                .padding(horizontal = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            keys.chunked(rows.coerceIn(1, 2)).forEach { column ->
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    column.forEach { button ->
                        key(button.shortcut) {
                            KeyButton(button.label, button.enabled, button.description, button.selected, onClick = button.onClick)
                        }
                    }
                }
            }
        }
        KeyButton(
            "›", scroll.canScrollForward, stringResource(R.string.term_keys_next), width = 20.dp,
        ) { scope.launch { scroll.animateScrollBy(pageWidth * 0.8f) } }
    }
}

private data class ExtraKey(
    val shortcut: KeyboardShortcut,
    val label: String,
    val enabled: Boolean,
    val description: String,
    val selected: Boolean?,
    val onClick: () -> Unit,
)

/** Fixed dimensions avoid Material button padding crowding out the terminal keys. */
@Composable
private fun KeyButton(
    label: String,
    enabled: Boolean = true,
    description: String = label,
    selected: Boolean? = null,
    width: Dp = 35.dp,
    onClick: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(4.dp)
    Box(
        modifier = Modifier
            .width(width)
            .height(35.dp)
            .alpha(if (enabled) 1f else 0.38f)
            .clip(shape)
            .background(if (selected == true) colors.primaryContainer else colors.surfaceVariant, shape)
            .border(0.5.dp, if (selected == true) colors.primary else colors.outlineVariant, shape)
            .focusProperties { canFocus = false }
            .semantics {
                contentDescription = description
                if (selected != null) this.selected = selected
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = if (selected == true) colors.onPrimaryContainer else colors.onSurface,
            style = TextStyle(
                fontSize = 11.sp,
                fontWeight = if (selected == true) FontWeight.Bold else FontWeight.Medium,
                platformStyle = PlatformTextStyle(includeFontPadding = false),
            ),
            maxLines = 1,
            softWrap = false,
        )
    }
}

@Composable
internal fun shortcutLabel(shortcut: KeyboardShortcut): String = when (shortcut) {
    KeyboardShortcut.Paste -> stringResource(R.string.term_paste)
    KeyboardShortcut.Copy -> stringResource(R.string.term_copy)
    else -> shortcut.label
}

@Composable
internal fun shortcutDescription(shortcut: KeyboardShortcut): String = when (shortcut) {
    KeyboardShortcut.Keyboard -> stringResource(R.string.term_keyboard)
    KeyboardShortcut.Paste -> stringResource(R.string.term_paste)
    KeyboardShortcut.Copy -> stringResource(R.string.term_copy)
    KeyboardShortcut.JumpBottom -> stringResource(R.string.term_jump_bottom)
    KeyboardShortcut.Up -> stringResource(R.string.term_key_up)
    KeyboardShortcut.Down -> stringResource(R.string.term_key_down)
    KeyboardShortcut.Left -> stringResource(R.string.term_key_left)
    KeyboardShortcut.Right -> stringResource(R.string.term_key_right)
    else -> shortcut.description
}
