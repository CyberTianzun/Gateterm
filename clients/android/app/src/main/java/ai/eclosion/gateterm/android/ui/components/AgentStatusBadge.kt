package ai.eclosion.gateterm.android.ui.components

import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.agent.AgentSession
import ai.eclosion.gateterm.android.agent.AgentState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

private enum class AgentKind {
    Claude, Codex, Gemini, OpenCode, Pi, Grok, Agy, Generic
}

private data class AgentBrand(
    val kind: AgentKind,
    val label: String,
    val accent: Color,
)

private fun agentBrand(agentId: String): AgentBrand {
    val lower = agentId.lowercase()
    return when {
        "claude" in lower -> AgentBrand(AgentKind.Claude, "Claude", Color(0xFFD97757))
        "codex" in lower -> AgentBrand(AgentKind.Codex, "Codex", Color(0xFF10A37F))
        "gemini" in lower -> AgentBrand(AgentKind.Gemini, "Gemini", Color(0xFF818CF8))
        "opencode" in lower -> AgentBrand(AgentKind.OpenCode, "OpenCode", Color(0xFF38BDF8))
        lower == "pi" || lower.startsWith("pi-") -> AgentBrand(AgentKind.Pi, "pi", Color(0xFFF472B6))
        "grok" in lower -> AgentBrand(AgentKind.Grok, "Grok", Color(0xFFE879F9))
        "agy" in lower || "antigravity" in lower -> AgentBrand(AgentKind.Agy, "AGY", Color(0xFF60A5FA))
        else -> AgentBrand(AgentKind.Generic, agentId, Color(0xFF4EA1FF))
    }
}

/**
 * Compact 20×20dp agent brand + status icon badge placed to the left of session titles,
 * aligned 1:1 with `clients/web` and `clients/desktop-native`.
 */
@Composable
fun AgentStatusBadge(agent: AgentSession, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val brand = agentBrand(agent.agentId)
    val (background, borderColor, iconColor) = when (agent.state) {
        AgentState.Waiting -> Triple(Color(0xFFFFB020), Color(0xFFFFB020), Color(0xFF241A00))
        AgentState.Error -> Triple(Color(0xFFFF6B6B), Color(0xFFFF6B6B), Color(0xFF2A0000))
        AgentState.Working, AgentState.Thinking -> Triple(
            brand.accent.copy(alpha = 0.22f),
            brand.accent.copy(alpha = 0.45f),
            brand.accent,
        )
        AgentState.Idle, AgentState.Done -> Triple(
            brand.accent.copy(alpha = 0.14f),
            brand.accent.copy(alpha = 0.30f),
            brand.accent,
        )
    }
    val dotColor: Color? = when (agent.state) {
        AgentState.Waiting -> Color(0xFFFFB020)
        AgentState.Working -> brand.accent
        AgentState.Thinking -> Color(0xFF9A8CFF)
        AgentState.Error -> Color(0xFFFF6B6B)
        AgentState.Idle, AgentState.Done -> null
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
    val statusCore = if (detail.isNullOrEmpty()) stateText else "$stateText · $detail"
    val label = if (brand.label.isBlank()) statusCore else "${brand.label} · $statusCore"
    val badgeShape = RoundedCornerShape(5.dp)

    Box(
        modifier = modifier
            .size(20.dp)
            .background(background, badgeShape)
            .border(1.dp, borderColor, badgeShape)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(modifier = Modifier.size(13.dp)) {
            drawAgentGlyph(brand.kind, iconColor)
        }
        if (dotColor != null) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .offset(x = 2.dp, y = 2.dp)
                    .size(6.dp)
                    .background(dotColor, CircleShape)
                    .border(1.dp, colors.surface, CircleShape),
            )
        }
    }
}

private fun DrawScope.drawAgentGlyph(kind: AgentKind, color: Color) {
    val sx = size.width / 16f
    val sy = size.height / 16f
    fun pt(x: Float, y: Float) = Offset(x * sx, y * sy)
    val stroke = Stroke(width = 1.5f * sx, cap = StrokeCap.Round, join = StrokeJoin.Round)

    when (kind) {
        AgentKind.Claude -> {
            drawLine(color, pt(8f, 2f), pt(8f, 14f), strokeWidth = stroke.width, cap = StrokeCap.Round)
            drawLine(color, pt(2f, 8f), pt(14f, 8f), strokeWidth = stroke.width, cap = StrokeCap.Round)
            drawLine(color, pt(3.76f, 3.76f), pt(12.24f, 12.24f), strokeWidth = stroke.width, cap = StrokeCap.Round)
            drawLine(color, pt(12.24f, 3.76f), pt(3.76f, 12.24f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
        AgentKind.Codex -> {
            val hex = Path().apply {
                moveTo(8f * sx, 1.75f * sy)
                lineTo(13.5f * sx, 4.9f * sy)
                lineTo(13.5f * sx, 11.1f * sy)
                lineTo(8f * sx, 14.25f * sy)
                lineTo(2.5f * sx, 11.1f * sy)
                lineTo(2.5f * sx, 4.9f * sy)
                close()
            }
            drawPath(hex, color, style = stroke)
            val prompt = Path().apply {
                moveTo(5.75f * sx, 6.25f * sy)
                lineTo(7.75f * sx, 8f * sy)
                lineTo(5.75f * sx, 9.75f * sy)
            }
            drawPath(prompt, color, style = stroke)
            drawLine(color, pt(8.5f, 9.75f), pt(10.5f, 9.75f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
        AgentKind.Gemini -> {
            val star = Path().apply {
                moveTo(8f * sx, 1.5f * sy)
                cubicTo(8f * sx, 5.1f * sy, 10.9f * sx, 8f * sy, 14.5f * sx, 8f * sy)
                cubicTo(10.9f * sx, 8f * sy, 8f * sx, 10.9f * sy, 8f * sx, 14.5f * sy)
                cubicTo(8f * sx, 10.9f * sy, 5.1f * sx, 8f * sy, 1.5f * sx, 8f * sy)
                cubicTo(5.1f * sx, 8f * sy, 8f * sx, 5.1f * sy, 8f * sx, 1.5f * sy)
                close()
            }
            drawPath(star, color, style = stroke)
        }
        AgentKind.OpenCode -> {
            val left = Path().apply {
                moveTo(5.25f * sx, 4.75f * sy)
                lineTo(2f * sx, 8f * sy)
                lineTo(5.25f * sx, 11.25f * sy)
            }
            val right = Path().apply {
                moveTo(10.75f * sx, 4.75f * sy)
                lineTo(14f * sx, 8f * sy)
                lineTo(10.75f * sx, 11.25f * sy)
            }
            drawPath(left, color, style = stroke)
            drawPath(right, color, style = stroke)
            drawLine(color, pt(9.25f, 3.5f), pt(6.75f, 12.5f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
        AgentKind.Pi -> {
            drawLine(color, pt(3f, 4.5f), pt(13f, 4.5f), strokeWidth = stroke.width, cap = StrokeCap.Round)
            drawLine(color, pt(6.25f, 4.5f), pt(6.25f, 11.75f), strokeWidth = stroke.width, cap = StrokeCap.Round)
            drawLine(color, pt(10.25f, 4.5f), pt(10.25f, 11.75f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
        AgentKind.Grok -> {
            drawCircle(color, radius = 4.75f * sx, center = pt(8f, 8f), style = stroke)
            drawLine(color, pt(3f, 13f), pt(13f, 3f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
        AgentKind.Agy -> {
            val delta = Path().apply {
                moveTo(8f * sx, 2.25f * sy)
                lineTo(13.5f * sx, 13f * sy)
                lineTo(2.5f * sx, 13f * sy)
                close()
            }
            drawPath(delta, color, style = stroke)
            drawLine(color, pt(5.2f, 10.2f), pt(10.8f, 10.2f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
        AgentKind.Generic -> {
            drawRoundRect(
                color = color,
                topLeft = pt(3f, 4.5f),
                size = Size(10f * sx, 8f * sy),
                cornerRadius = CornerRadius(2f * sx, 2f * sy),
                style = stroke,
            )
            drawLine(color, pt(8f, 2f), pt(8f, 4.5f), strokeWidth = stroke.width, cap = StrokeCap.Round)
            drawLine(color, pt(6f, 10.5f), pt(10f, 10.5f), strokeWidth = stroke.width, cap = StrokeCap.Round)
        }
    }
}
