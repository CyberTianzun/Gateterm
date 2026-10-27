package ai.eclosion.gateterm.android.term

import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalCellMetricsTest {
    @Test
    fun matchesCapturedBrowserGridAcrossFontsSizesDensitiesAndSpacing() {
        val text = javaClass.getResourceAsStream("/terminal-webgl-metrics.json")!!.bufferedReader().use { it.readText() }
        val cases = JSONObject(text).getJSONArray("cases")
        assertTrue(cases.length() >= 80)
        for (i in 0 until cases.length()) {
            val c = cases.getJSONObject(i)
            val metrics = TerminalCellMetrics(c.getDouble("logicalAdvance"), c.getDouble("logicalAscent"),
                c.getDouble("logicalDescent"), c.getDouble("dpr"), c.getDouble("lineHeight"), c.getDouble("letterSpacing"))
            val label = "${c.getString("font")} ${c.getInt("size")} DPR ${c.getDouble("dpr")} spacing ${c.getDouble("letterSpacing")} height ${c.getDouble("lineHeight")}"
            assertEquals(label, c.getInt("cellWidthDevicePx"), metrics.width)
            assertEquals(label, c.getInt("cellHeightDevicePx"), metrics.height)
            assertEquals(label, c.getInt("charLeftDevicePx"), metrics.charLeft)
            assertEquals(label, c.getInt("charTopDevicePx"), metrics.charTop)
        }
    }

    @Test
    fun readsAdvanceFromTheActualBundledFontFiles() {
        val assets = File(requireNotNull(System.getProperty("gateterm.fontAssets")))
        for (font in listOf("JetBrainsMono", "GoogleSansCode", "ZedMono")) {
            val advance = File(assets, "$font-Regular.ttf").inputStream().use(FontAdvance::readEm)
            assertEquals(font, 0.6, advance, 0.00000001)
            // A 20px font at DPR 1 must not shrink to 11px due to native float noise.
            assertEquals(12, TerminalCellMetrics(advance * 20, 20.4, 6.0, 1.0, 1.0, 0.0).width)
        }
    }

    @Test
    fun extremeNegativeSpacingNeverCreatesZeroSizedCells() {
        val metrics = TerminalCellMetrics(3.6, 6.12, 1.8, 1.0, 0.8, -5.0)
        assertEquals(1, metrics.width)
        assertEquals(8, metrics.height)
        assertEquals(0, metrics.charTop)
    }
}
