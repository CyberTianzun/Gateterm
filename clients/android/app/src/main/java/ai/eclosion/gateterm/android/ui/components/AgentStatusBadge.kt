package ai.eclosion.gateterm.android.ui.components

import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.agent.AgentSession
import ai.eclosion.gateterm.android.agent.AgentState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

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
        "codex" in lower -> AgentBrand(AgentKind.Codex, "Codex", Color(0xFF7A9DFF))
        "gemini" in lower -> AgentBrand(AgentKind.Gemini, "Gemini", Color(0xFF3186FF))
        "opencode" in lower -> AgentBrand(AgentKind.OpenCode, "OpenCode", Color(0xFF38BDF8))
        lower == "pi" || lower.startsWith("pi-") -> AgentBrand(AgentKind.Pi, "pi", Color(0xFFF472B6))
        "grok" in lower -> AgentBrand(AgentKind.Grok, "Grok", Color(0xFFF3F4F6))
        "agy" in lower || "antigravity" in lower -> AgentBrand(AgentKind.Agy, "AGY", Color(0xFF3186FF))
        else -> AgentBrand(AgentKind.Generic, agentId.ifBlank { "Agent" }, Color(0xFF4EA1FF))
    }
}

private const val CLAUDE_SVG_PATH =
    "M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"

private const val CODEX_SVG_PATH =
    "M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"

private const val GEMINI_SVG_PATH =
    "M20.616 10.835a14.147 14.147 0 01-4.45-3.001 14.111 14.111 0 01-3.678-6.452.503.503 0 00-.975 0 14.134 14.134 0 01-3.679 6.452 14.155 14.155 0 01-4.45 3.001c-.65.28-1.318.505-2.002.678a.502.502 0 000 .975c.684.172 1.35.397 2.002.677a14.147 14.147 0 014.45 3.001 14.112 14.112 0 013.679 6.453.502.502 0 00.975 0c.172-.685.397-1.351.677-2.003a14.145 14.145 0 013.001-4.45 14.113 14.113 0 016.453-3.678.503.503 0 000-.975 13.245 13.245 0 01-2.003-.678z"

private const val OPENCODE_SVG_PATH =
    "M16 6H8v12h8V6zm4 16H4V2h16v20z"

private const val GROK_SVG_PATH =
    "M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815"

private const val AGY_SVG_PATH =
    "M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z"

private const val PI_SVG_PATH =
    "M4.5 6.75h15M9.375 6.75v10.875M15.375 6.75v8.625a2.25 2.25 0 0 0 2.25 2.25"

private val claudePath: Path by lazy {
    PathParser().parsePathString(CLAUDE_SVG_PATH).toPath()
}
private val codexPath: Path by lazy {
    PathParser().parsePathString(CODEX_SVG_PATH).toPath().apply {
        fillType = PathFillType.EvenOdd
    }
}
private val geminiPath: Path by lazy {
    PathParser().parsePathString(GEMINI_SVG_PATH).toPath()
}
private val openCodePath: Path by lazy {
    PathParser().parsePathString(OPENCODE_SVG_PATH).toPath().apply {
        fillType = PathFillType.EvenOdd
    }
}
private val grokPath: Path by lazy {
    PathParser().parsePathString(GROK_SVG_PATH).toPath().apply {
        fillType = PathFillType.EvenOdd
    }
}
private val agyPath: Path by lazy {
    PathParser().parsePathString(AGY_SVG_PATH).toPath().apply {
        fillType = PathFillType.EvenOdd
    }
}
private val piPath: Path by lazy {
    PathParser().parsePathString(PI_SVG_PATH).toPath()
}

/**
 * Unified, reusable Agent brand icon component used across the Sidebar session list
 * and Agent Session History screens.
 */
@Composable
fun AgentBrandIcon(
    agentId: String,
    modifier: Modifier = Modifier,
    boxSize: Dp = 20.dp,
    glyphSize: Dp = 18.dp,
) {
    val brand = agentBrand(agentId)
    val grokColor = MaterialTheme.colorScheme.onSurface

    Box(
        modifier = modifier
            .size(boxSize)
            .semantics { contentDescription = brand.label },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(modifier = Modifier.size(glyphSize)) {
            drawAgentGlyph(brand.kind, brand.accent, grokColor)
        }
    }
}

/**
 * Pure 20×20dp agent brand icon placed to the left of session titles.
 * Always renders in the agent's fixed brand color without a bounding box or status overlays.
 */
@Composable
fun AgentStatusBadge(agent: AgentSession, modifier: Modifier = Modifier) {
    AgentBrandIcon(agentId = agent.agentId, modifier = modifier)
}

/**
 * Separate compact status tag displaying the agent's runtime state (`Waiting`, `Working`,
 * `Thinking`, `Error`, `Idle`) independently of the agent brand icon.
 */
@Composable
fun AgentStateTag(agent: AgentSession, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val (background, borderColor, textColor) = when (agent.state) {
        AgentState.Waiting -> Triple(Color(0xFFFFB020), Color(0xFFFFB020), Color(0xFF241A00))
        AgentState.Error -> Triple(Color(0xFFFF6B6B), Color(0xFFFF6B6B), Color(0xFF2A0000))
        AgentState.Working -> Triple(
            Color(0xFF4EA1FF).copy(alpha = 0.20f),
            Color(0xFF4EA1FF).copy(alpha = 0.45f),
            Color(0xFF6EB5FF),
        )
        AgentState.Thinking -> Triple(
            Color(0xFF9A8CFF).copy(alpha = 0.20f),
            Color(0xFF9A8CFF).copy(alpha = 0.45f),
            Color(0xFFB8AEFF),
        )
        AgentState.Idle, AgentState.Done -> Triple(
            colors.onSurfaceVariant.copy(alpha = 0.14f),
            colors.outlineVariant,
            colors.onSurfaceVariant,
        )
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
    val fullDesc = if (detail.isNullOrEmpty()) stateText else "$stateText · $detail"
    val tagShape = RoundedCornerShape(4.dp)

    Box(
        modifier = modifier
            .background(background, tagShape)
            .border(1.dp, borderColor, tagShape)
            .padding(horizontal = 6.dp, vertical = 2.dp)
            .semantics { contentDescription = fullDesc },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = stateText,
            color = textColor,
            fontSize = 11.sp,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

private fun DrawScope.drawAgentGlyph(kind: AgentKind, color: Color, grokColor: Color) {
    val sx = size.width / 24f
    val sy = size.height / 24f

    withTransform({
        scale(sx, sy, Offset.Zero)
    }) {
        when (kind) {
            AgentKind.Claude -> {
                drawPath(claudePath, color = color)
            }
            AgentKind.Codex -> {
                val codexBrush = Brush.verticalGradient(
                    colors = listOf(Color(0xFFB1A7FF), Color(0xFF7A9DFF), Color(0xFF3941FF)),
                    startY = 0f,
                    endY = 24f,
                )
                drawPath(codexPath, brush = codexBrush)
            }
            AgentKind.Gemini -> {
                val geminiBrush = Brush.linearGradient(
                    colors = listOf(Color(0xFF3186FF), Color(0xFF08B962), Color(0xFFFABC12), Color(0xFFF94543)),
                    start = Offset(4f, 18f),
                    end = Offset(20f, 6f),
                )
                drawPath(geminiPath, brush = geminiBrush)
            }
            AgentKind.OpenCode -> {
                drawPath(openCodePath, color = color)
            }
            AgentKind.Pi -> {
                drawPath(
                    piPath,
                    color = color,
                    style = Stroke(width = 2.1f, cap = StrokeCap.Round, join = StrokeJoin.Round),
                )
            }
            AgentKind.Grok -> {
                drawPath(grokPath, color = grokColor)
            }
            AgentKind.Agy -> {
                val agyBrush = Brush.linearGradient(
                    colors = listOf(Color(0xFF3186FF), Color(0xFF00B95C), Color(0xFFFBBC04), Color(0xFFFC413D)),
                    start = Offset(0f, 0f),
                    end = Offset(24f, 24f),
                )
                drawPath(agyPath, brush = agyBrush)
            }
            AgentKind.Generic -> {
                val stroke = Stroke(width = 2f, cap = StrokeCap.Round, join = StrokeJoin.Round)
                drawRoundRect(
                    color = color,
                    topLeft = Offset(4.5f, 6.75f),
                    size = Size(15f, 12f),
                    cornerRadius = CornerRadius(3f, 3f),
                    style = stroke,
                )
                drawLine(color, Offset(12f, 3f), Offset(12f, 6.75f), strokeWidth = 2f, cap = StrokeCap.Round)
                drawLine(color, Offset(9f, 15.75f), Offset(15f, 15.75f), strokeWidth = 2f, cap = StrokeCap.Round)
            }
        }
    }
}
