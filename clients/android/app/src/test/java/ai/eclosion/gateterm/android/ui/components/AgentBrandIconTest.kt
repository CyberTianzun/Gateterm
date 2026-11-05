package ai.eclosion.gateterm.android.ui.components

import androidx.compose.ui.graphics.vector.PathNode
import androidx.compose.ui.graphics.vector.PathParser
import org.junit.Assert.assertTrue
import org.junit.Test

class AgentBrandIconTest {
    @Test
    fun agentBrandIconPathsParseCleanlyInside24x24ViewBox() {
        val icons = mapOf(
            "claude" to CLAUDE_SVG_PATH,
            "codex" to CODEX_SVG_PATH,
            "gemini" to GEMINI_SVG_PATH,
            "opencode" to OPENCODE_SVG_PATH,
            "copilot" to COPILOT_SVG_PATH,
            "grok" to GROK_SVG_PATH,
            "agy" to AGY_SVG_PATH,
            "pi" to PI_SVG_PATH,
        )
        for ((name, pathStr) in icons) {
            val nodes = PathParser().parsePathString(pathStr).toNodes()
            assertTrue("$name should have path nodes", nodes.isNotEmpty())
            for (node in nodes) {
                when (node) {
                    is PathNode.MoveTo -> {
                        assertTrue("$name MoveTo x=${node.x}", node.x in -0.5f..24.5f)
                        assertTrue("$name MoveTo y=${node.y}", node.y in -0.5f..24.5f)
                    }
                    is PathNode.LineTo -> {
                        assertTrue("$name LineTo x=${node.x}", node.x in -0.5f..24.5f)
                        assertTrue("$name LineTo y=${node.y}", node.y in -0.5f..24.5f)
                    }
                    is PathNode.CurveTo -> {
                        assertTrue("$name CurveTo x3=${node.x3}", node.x3 in -0.5f..24.5f)
                        assertTrue("$name CurveTo y3=${node.y3}", node.y3 in -0.5f..24.5f)
                    }
                    is PathNode.Close -> Unit
                    else -> throw AssertionError("$name contained unexpected PathNode: $node")
                }
            }
        }
    }
}
