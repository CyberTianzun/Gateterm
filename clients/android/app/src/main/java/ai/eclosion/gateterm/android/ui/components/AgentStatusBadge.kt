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
import androidx.compose.ui.draw.clipToBounds
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
    Claude, Codex, Gemini, OpenCode, Copilot, Pi, Grok, Agy, Generic
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
        "copilot" in lower -> AgentBrand(AgentKind.Copilot, "Copilot", Color(0xFFA371F7))
        lower == "pi" || lower.startsWith("pi-") -> AgentBrand(AgentKind.Pi, "pi", Color(0xFFF472B6))
        "grok" in lower -> AgentBrand(AgentKind.Grok, "Grok", Color(0xFFF3F4F6))
        "agy" in lower || "antigravity" in lower -> AgentBrand(AgentKind.Agy, "AGY", Color(0xFF3186FF))
        else -> AgentBrand(AgentKind.Generic, agentId.ifBlank { "Agent" }, Color(0xFF4EA1FF))
    }
}

// Pure space-separated M / L / C / Z cubic-Bezier paths (viewBox 0 0 24 24).
// All elliptical arcs (A/a) and H/V shorthands are pre-converted to explicit cubic Beziers
// so Android's PathParser never encounters minified arc-flag tokens (e.g. `0 013.046` or `0 000`).
internal const val CLAUDE_SVG_PATH =
    "M 4.709 15.955 L 9.429 13.308 L 9.509 13.078 L 9.429 12.95 L 9.2 12.95 L 8.41 12.902 L 5.712 12.829 L 3.373 12.732 L 1.107 12.61 L 0.536 12.489 L 0 11.784 L 0.055 11.432 L 0.535 11.111 L 1.221 11.171 L 2.741 11.274 L 5.019 11.432 L 6.671 11.529 L 9.12 11.784 L 9.509 11.784 L 9.564 11.627 L 9.43 11.529 L 9.327 11.432 L 6.969 9.836 L 4.417 8.148 L 3.081 7.176 L 2.357 6.685 L 1.993 6.223 L 1.835 5.215 L 2.491 4.493 L 3.372 4.553 L 3.597 4.614 L 4.49 5.3 L 6.398 6.776 L 8.889 8.609 L 9.254 8.913 L 9.399 8.81 L 9.418 8.737 L 9.254 8.463 L 7.899 6.017 L 6.453 3.527 L 5.809 2.495 L 5.639 1.876 C 5.574 1.638 5.539 1.393 5.535 1.147 L 6.283 0.134 L 6.696 0 L 7.692 0.134 L 8.112 0.498 L 8.732 1.912 L 9.734 4.141 L 11.289 7.171 L 11.745 8.069 L 11.988 8.901 L 12.079 9.156 L 12.237 9.156 L 12.237 9.01 L 12.365 7.304 L 12.602 5.209 L 12.832 2.514 L 12.912 1.754 L 13.288 0.844 L 14.035 0.352 L 14.619 0.632 L 15.099 1.317 L 15.032 1.761 L 14.746 3.612 L 14.187 6.515 L 13.823 8.457 L 14.035 8.457 L 14.278 8.215 L 15.263 6.909 L 16.915 4.845 L 17.645 4.025 L 18.495 3.121 L 19.042 2.69 L 20.075 2.69 L 20.835 3.819 L 20.495 4.985 L 19.431 6.332 L 18.55 7.474 L 17.286 9.174 L 16.496 10.534 L 16.569 10.644 L 16.757 10.624 L 19.613 10.018 L 21.156 9.738 L 22.997 9.423 L 23.83 9.811 L 23.921 10.206 L 23.593 11.013 L 21.624 11.499 L 19.315 11.961 L 15.876 12.774 L 15.834 12.804 L 15.883 12.865 L 17.432 13.011 L 18.094 13.047 L 19.716 13.047 L 22.736 13.272 L 23.526 13.794 L 24 14.432 L 23.921 14.917 L 22.706 15.537 L 21.066 15.148 L 17.237 14.238 L 15.925 13.909 L 15.743 13.909 L 15.743 14.019 L 16.836 15.087 L 18.842 16.897 L 21.351 19.227 L 21.478 19.805 L 21.156 20.26 L 20.816 20.211 L 18.611 18.554 L 17.76 17.807 L 15.834 16.187 L 15.706 16.187 L 15.706 16.357 L 16.15 17.006 L 18.495 20.527 L 18.617 21.607 L 18.447 21.96 L 17.839 22.173 L 17.171 22.051 L 15.797 20.126 L 14.382 17.959 L 13.239 16.016 L 13.099 16.096 L 12.425 23.35 L 12.109 23.72 L 11.38 24 L 10.773 23.539 L 10.451 22.792 L 10.773 21.316 L 11.162 19.392 L 11.477 17.862 L 11.763 15.962 L 11.933 15.33 L 11.921 15.288 L 11.781 15.306 L 10.347 17.273 L 8.167 20.218 L 6.441 22.063 L 6.027 22.227 L 5.31 21.857 L 5.377 21.195 L 5.778 20.606 L 8.166 17.57 L 9.606 15.688 L 10.536 14.602 L 10.53 14.444 L 10.475 14.444 L 4.132 18.56 L 3.002 18.706 L 2.515 18.25 L 2.576 17.504 L 2.807 17.261 L 4.715 15.949 L 4.709 15.955 Z"

internal const val CODEX_SVG_PATH =
    "M 8.086 0.457 C 9.049 0.061 10.098 -0.082 11.132 0.042 C 12.465 0.195 13.653 0.762 14.696 1.742 C 14.725 1.769 14.765 1.78 14.803 1.771 C 16.211 1.425 17.565 1.547 18.864 2.137 L 18.927 2.167 L 19.081 2.243 C 20.438 2.946 21.411 4.013 21.999 5.441 C 22.277 6.12 22.417 6.829 22.42 7.567 C 22.44 8.117 22.379 8.666 22.24 9.198 C 22.226 9.253 22.241 9.312 22.28 9.353 C 23.066 10.15 23.613 11.152 23.858 12.244 C 24.243 14.145 23.848 15.859 22.675 17.384 L 22.493 17.604 C 21.716 18.494 20.696 19.137 19.559 19.455 C 19.509 19.47 19.468 19.508 19.451 19.557 C 19.196 20.293 18.94 20.921 18.464 21.549 C 17.265 23.131 15.502 24.011 13.516 24 C 11.933 23.992 10.53 23.413 9.306 22.264 C 9.268 22.229 9.215 22.217 9.166 22.232 C 8.648 22.399 8.126 22.423 7.562 22.417 C 6.661 22.41 5.773 22.197 4.967 21.795 C 4.123 21.376 3.388 20.766 2.821 20.014 C 2.618 19.745 2.417 19.492 2.27 19.193 C 2.067 18.781 1.902 18.352 1.775 17.91 C 1.509 16.907 1.503 15.852 1.758 14.846 C 1.766 14.822 1.769 14.797 1.766 14.772 C 1.761 14.747 1.748 14.725 1.729 14.708 C 1.113 14.084 0.641 13.333 0.349 12.506 C 0.155 11.997 0.043 11.461 0.016 10.917 C -0.032 10.201 0.031 9.482 0.204 8.785 C 0.654 7.301 1.513 6.137 2.781 5.292 C 3.063 5.104 3.331 4.958 3.583 4.854 C 3.869 4.734 4.156 4.634 4.444 4.55 C 4.486 4.538 4.519 4.505 4.531 4.463 C 4.749 3.678 5.125 2.945 5.635 2.31 C 6.315 1.464 7.132 0.846 8.086 0.457 Z M 7.282 8.307 C 7.049 7.9 6.531 7.759 6.125 7.992 C 5.718 8.224 5.576 8.742 5.809 9.149 L 7.503 12.114 L 5.815 14.962 C 5.6 15.363 5.739 15.862 6.131 16.094 C 6.522 16.326 7.027 16.207 7.275 15.826 L 9.215 12.554 C 9.371 12.291 9.373 11.965 9.222 11.7 L 7.282 8.307 Z M 12.728 14.547 C 12.279 14.574 11.929 14.945 11.929 15.395 C 11.929 15.844 12.279 16.215 12.728 16.242 L 17.576 16.242 C 18.028 16.22 18.384 15.847 18.384 15.394 C 18.384 14.941 18.028 14.568 17.576 14.546 L 12.728 14.546 Z"

internal const val GEMINI_SVG_PATH =
    "M 20.616 10.835 C 18.955 10.128 17.444 9.109 16.166 7.834 C 14.386 6.05 13.116 3.822 12.488 1.382 C 12.431 1.159 12.231 1.003 12 1.003 C 11.77 1.003 11.57 1.159 11.513 1.382 C 10.884 3.822 9.614 6.05 7.834 7.834 C 6.556 9.109 5.045 10.128 3.384 10.835 C 2.734 11.115 2.066 11.34 1.382 11.513 C 1.158 11.568 1 11.769 1 12.001 C 1 12.232 1.158 12.433 1.382 12.488 C 2.066 12.66 2.732 12.885 3.384 13.165 C 5.045 13.872 6.556 14.891 7.834 16.166 C 9.615 17.95 10.885 20.178 11.513 22.619 C 11.568 22.843 11.769 23.001 12 23.001 C 12.232 23.001 12.433 22.843 12.488 22.619 C 12.66 21.934 12.885 21.268 13.165 20.616 C 13.872 18.955 14.891 17.444 16.166 16.166 C 17.95 14.386 20.178 13.116 22.619 12.488 C 22.842 12.431 22.998 12.231 22.998 12 C 22.998 11.77 22.842 11.57 22.619 11.513 C 21.934 11.341 21.265 11.114 20.616 10.835 Z"

internal const val OPENCODE_SVG_PATH =
    "M 16 6 L 8 6 L 8 18 L 16 18 L 16 6 Z M 20 22 L 4 22 L 4 2 L 20 2 L 20 22 Z"

internal const val COPILOT_SVG_PATH =
    "M 23.922 16.992 C 23.061 18.495 18.067 22.012 12 22.012 C 5.933 22.012 0.939 18.495 0.078 16.992 C 0.027 16.895 0 16.785 0 16.673 L 0 14.226 C 0 13.662 0.262 13.131 0.708 12.789 C 1.2 12.406 1.797 11.995 2.525 11.584 C 2.458 9.211 2.838 7.229 3.694 5.799 C 4.539 4.386 5.899 3.558 7.757 3.249 C 9.199 3.012 10.606 2.987 11.999 2.988 C 13.393 2.987 14.8 3.012 16.242 3.249 C 18.1 3.558 19.46 4.386 20.305 5.799 C 21.161 7.229 21.542 9.211 21.475 11.584 C 22.202 11.995 22.8 12.406 23.291 12.789 C 23.738 13.131 24 13.662 24 14.226 L 24 16.673 C 24 16.785 23.973 16.895 23.922 16.992 Z M 4.868 12.966 C 6.238 13.524 8.226 13.861 11.225 13.987 L 11.225 10.228 C 9.488 10.172 8.248 9.914 7.367 9.497 C 6.283 8.983 5.736 8.211 5.367 7.121 C 4.94 8.352 4.764 10.258 4.868 12.966 Z M 12.775 13.987 C 15.774 13.861 17.762 13.524 19.132 12.966 C 19.236 10.258 19.06 8.352 18.633 7.121 C 18.264 8.211 17.717 8.983 16.633 9.497 C 15.752 9.914 14.512 10.172 12.775 10.228 L 12.775 13.987 Z M 9.25 16.25 C 8.56 16.25 8 16.81 8 17.5 C 8 18.19 8.56 18.75 9.25 18.75 C 9.94 18.75 10.5 18.19 10.5 17.5 C 10.5 16.81 9.94 16.25 9.25 16.25 Z M 14.75 16.25 C 14.06 16.25 13.5 16.81 13.5 17.5 C 13.5 18.19 14.06 18.75 14.75 18.75 C 15.44 18.75 16 18.19 16 17.5 C 16 16.81 15.44 16.25 14.75 16.25 Z"

internal const val GROK_SVG_PATH =
    "M 9.27 15.29 L 17.248 9.393 C 17.639 9.103 18.198 9.216 18.385 9.665 C 19.365 12.034 18.927 14.88 16.975 16.834 C 15.024 18.788 12.308 19.216 9.826 18.24 L 7.115 19.497 C 11.004 22.158 15.726 21.5 18.677 18.544 C 21.018 16.2 21.743 13.005 21.065 10.124 L 21.071 10.131 C 20.088 5.899 21.313 4.207 23.821 0.748 C 23.881 0.666 23.941 0.584 24 0.5 L 20.699 3.805 L 20.699 3.795 L 9.267 15.292 Z M 7.623 16.723 C 4.831 14.053 5.313 9.922 7.694 7.539 C 9.455 5.776 12.341 5.056 14.86 6.114 L 17.565 4.864 C 17.003 4.45 16.388 4.114 15.736 3.864 C 12.386 2.493 8.541 3.268 5.984 5.83 C 3.451 8.366 2.654 12.266 4.022 15.594 C 5.044 18.081 3.369 19.84 1.682 21.616 C 1.083 22.246 0.483 22.875 0 23.541 L 7.62 16.726 Z"

internal const val AGY_SVG_PATH =
    "M 21.751 22.607 C 23.091 23.612 25.101 22.942 23.259 21.099 C 17.73 15.74 18.904 1 12.037 1 C 5.17 1 6.342 15.74 0.815 21.1 C -1.195 23.109 0.982 23.611 2.322 22.606 C 7.514 19.089 7.179 12.892 12.037 12.892 C 16.894 12.892 16.559 19.089 21.751 22.607 Z"

internal const val PI_SVG_PATH =
    "M 4.5 6.75 L 19.5 6.75 M 9.375 6.75 L 9.375 17.625 M 15.375 6.75 L 15.375 15.375 C 15.375 16.618 16.382 17.625 17.625 17.625"

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
private val copilotPath: Path by lazy {
    PathParser().parsePathString(COPILOT_SVG_PATH).toPath().apply {
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
        Canvas(
            modifier = Modifier
                .size(glyphSize)
                .clipToBounds(),
        ) {
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
            AgentKind.Copilot -> {
                drawPath(copilotPath, color = color)
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
                drawPath(agyPath, color = Color(0xFF3186FF))
                drawPath(
                    agyPath,
                    brush = Brush.radialGradient(
                        colors = listOf(Color(0xFFFC413D), Color(0x00FC413D)),
                        center = Offset(20.5f, 3.5f),
                        radius = 14f,
                    ),
                )
                drawPath(
                    agyPath,
                    brush = Brush.radialGradient(
                        colors = listOf(Color(0xFF00B95C), Color(0x0000B95C)),
                        center = Offset(2.5f, 6.0f),
                        radius = 14f,
                    ),
                )
                drawPath(
                    agyPath,
                    brush = Brush.radialGradient(
                        colors = listOf(Color(0xFFFBBC04), Color(0x00FBBC04)),
                        center = Offset(10.5f, -1.5f),
                        radius = 11.5f,
                    ),
                )
                drawPath(
                    agyPath,
                    brush = Brush.radialGradient(
                        colors = listOf(Color(0xFF3186FF), Color(0x003186FF)),
                        center = Offset(12.0f, 18.0f),
                        radius = 13.5f,
                    ),
                )
                drawPath(
                    agyPath,
                    brush = Brush.radialGradient(
                        colors = listOf(Color(0x99749BFF), Color(0x00749BFF)),
                        center = Offset(21.5f, 13.5f),
                        radius = 7.5f,
                    ),
                )
                drawPath(
                    agyPath,
                    brush = Brush.radialGradient(
                        colors = listOf(Color(0xCCFFEE48), Color(0x00FFEE48)),
                        center = Offset(4.0f, 2.0f),
                        radius = 7.5f,
                    ),
                )
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
