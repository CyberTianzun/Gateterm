package ai.eclosion.octoterm.android.term

import ai.eclosion.octoterm.android.appearance.AppearanceCodec
import ai.eclosion.octoterm.android.appearance.TermAppearance
import org.junit.Assert.assertEquals
import org.junit.Test

class TerminalThemeTest {
    private fun foreground(theme: TermAppearance, cell: Cell) = theme.colorFor(cell.fgIndex, cell.fg, true)
    private fun background(theme: TermAppearance, cell: Cell) = theme.colorFor(cell.bgIndex, cell.bg, false)

    @Test
    fun colorResetRestoresDefaultColorIdentityForEveryColorMode() {
        val colors = listOf(
            "ansi" to "\u001b[1;31;44m",
            "indexed" to "\u001b[1;38;5;196;48;5;17m",
            "rgb" to "\u001b[1;38;2;1;2;3;48;2;4;5;6m",
        )
        val resets = listOf("empty" to "\u001b[m", "zero" to "\u001b[0m", "repeated-zero" to "\u001b[0;0m")
        for ((colorName, color) in colors) for ((resetName, reset) in resets) {
            val emulator = VtEmulator(20, 3)
            emulator.write("${color}X${reset}Y".toByteArray())
            val cell = emulator.cell(1, 0)
            assertEquals("$colorName/$resetName foreground", Color.INDEX_DEFAULT, cell.fgIndex)
            assertEquals("$colorName/$resetName background", Color.INDEX_DEFAULT, cell.bgIndex)
            assertEquals(0, cell.attrs)
        }
    }

    @Test
    fun pageRedrawAfterTrueColorStillUsesTheSelectedTheme() {
        val dark = AppearanceCodec.builtin("2026 Dark")!!
        val emulator = VtEmulator(20, 3)
        emulator.write("First page".toByteArray())
        assertEquals(dark.foreground, foreground(dark, emulator.cell(0, 0)))
        emulator.write("\u001b[38;2;1;2;3;48;2;4;5;6mX\u001b[m\u001b[2J\u001b[HNext page".toByteArray())
        val frame = TermFrame.capture(emulator)
        assertEquals('N', frame.cells[0][0].ch)
        assertEquals(dark.foreground, foreground(dark, frame.cells[0][0]))
        assertEquals(dark.background, background(dark, frame.cells[0][0]))
    }

    @Test
    fun scrollbackPreservesThemedDefaultsAndExplicitRgbAcrossThemeChanges() {
        val dark = AppearanceCodec.builtin("2026 Dark")!!
        val light = AppearanceCodec.builtin("2026 Light")!!
        val emulator = VtEmulator(12, 2, maxScrollback = 10)
        emulator.write("\u001b[38;2;1;2;3;48;2;4;5;6mRGB\u001b[0m\r\nDefault\r\nLatest".toByteArray())
        emulator.scrollBy(1)
        val frame = TermFrame.capture(emulator)
        val rgb = frame.cells[0][0]
        val normal = frame.cells[1][0]
        assertEquals('R', rgb.ch)
        assertEquals('D', normal.ch)
        for (theme in listOf(dark, light)) {
            assertEquals(theme.foreground, foreground(theme, normal))
            assertEquals(theme.background, background(theme, normal))
            assertEquals(Color.rgb(1, 2, 3), foreground(theme, rgb))
            assertEquals(Color.rgb(4, 5, 6), background(theme, rgb))
        }
        emulator.scrollToBottom()
        assertEquals(dark.foreground, foreground(dark, TermFrame.capture(emulator).cells[0][0]))
    }
}
