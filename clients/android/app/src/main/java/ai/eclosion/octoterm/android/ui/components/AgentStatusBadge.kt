package ai.eclosion.octoterm.android.ui.components

import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.agent.AgentSession
import ai.eclosion.octoterm.android.agent.AgentState
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** Same state labels, notice details and emphasis as Web's session-list badges. */
@Composable
fun AgentStatusBadge(agent: AgentSession, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val (background, foreground) = when (agent.state) {
        AgentState.Waiting -> Color(0xFFFFB020) to Color(0xFF241A00)
        AgentState.Error -> Color(0xFFFF6B6B) to Color(0xFF2A0000)
        AgentState.Working -> Color(0xFF4EA1FF).copy(alpha = 0.28f) to colors.onSurface
        AgentState.Thinking -> Color(0xFF9A8CFF).copy(alpha = 0.28f) to colors.onSurface
        AgentState.Idle, AgentState.Done -> colors.surfaceVariant to colors.onSurfaceVariant
    }
    val stateText = stringResource(when (agent.state) {
        AgentState.Idle -> R.string.agent_state_idle
        AgentState.Thinking -> R.string.agent_state_thinking
        AgentState.Working -> R.string.agent_state_working
        AgentState.Waiting -> if (agent.pending.isNullOrEmpty()) R.string.agent_state_waiting_terminal else R.string.agent_state_waiting
        AgentState.Error -> R.string.agent_state_error
        AgentState.Done -> R.string.agent_state_done
    })
    val detail = when (agent.detail) {
        "permission_prompt" -> stringResource(R.string.agent_notice_permission)
        "idle_prompt" -> stringResource(R.string.agent_notice_idle)
        "agent_needs_input" -> stringResource(R.string.agent_notice_input)
        "elicitation_dialog" -> stringResource(R.string.agent_notice_choice)
        else -> agent.detail
    }
    val label = if (detail.isNullOrEmpty()) stateText else "$stateText · $detail"
    Surface(modifier = modifier, shape = CircleShape, color = background, contentColor = foreground) {
        Text(
            text = label,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 3.dp),
            style = MaterialTheme.typography.labelMedium,
            fontWeight = when (agent.state) {
                AgentState.Waiting -> FontWeight.Bold
                AgentState.Error -> FontWeight.SemiBold
                else -> FontWeight.Medium
            },
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
