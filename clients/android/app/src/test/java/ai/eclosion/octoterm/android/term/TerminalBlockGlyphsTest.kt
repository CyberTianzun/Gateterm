package ai.eclosion.octoterm.android.term

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalBlockGlyphsTest {
    private fun occupied(codepoint: Int, x: Int, y: Int): Boolean =
        TerminalBlockGlyphs.rectangles(codepoint)!!.toList().chunked(4).any { r ->
            x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
        }

    @Test
    fun claudeCrabQuadrantsHaveTheirUnicodeCoverage() {
        val masks = mapOf('▛' to 7, '▜' to 11, '▝' to 2, '▘' to 1, '▐' to 10, '▌' to 5, '█' to 15)
        for ((glyph, mask) in masks) {
            for (y in 0..7) for (x in 0..7) {
                val quadrant = (y / 4) * 2 + x / 4
                assertEquals("$glyph at $x,$y", mask and (1 shl quadrant) != 0, occupied(glyph.code, x, y))
            }
        }
    }

    @Test
    fun allSolidBlocksStayInsideTheCellAndOrdinaryTextFallsBack() {
        var count = 0
        for (code in 0x2580..0x259f) {
            val rectangles = TerminalBlockGlyphs.rectangles(code) ?: continue
            count++
            for (r in rectangles.toList().chunked(4)) {
                assertTrue(r[0] in 0..7 && r[1] in 0..7)
                assertTrue(r[2] in 1..8 && r[3] in 1..8)
                assertTrue(r[0] < r[2] && r[1] < r[3])
            }
        }
        assertEquals(29, count)
        for (code in listOf('A'.code, '中'.code, 0x2591, 0x2592, 0x2593, 0x1f980)) {
            assertNull(TerminalBlockGlyphs.rectangles(code))
        }
    }
}
