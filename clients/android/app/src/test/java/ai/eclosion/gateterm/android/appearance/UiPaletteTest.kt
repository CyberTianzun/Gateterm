package ai.eclosion.gateterm.android.appearance

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UiPaletteTest {
    @Test
    fun dark2026MatchesWebUiColors() {
        val palette = AppearanceCodec.default(true).uiPalette()
        assertFalse(palette.light)
        assertEquals(0x191A1B, palette.background)
        assertEquals(0xCCCCCC, palette.foreground)
        assertEquals(0x262627, palette.panel)
        assertEquals(0x323334, palette.panel2)
        assertEquals(0x404142, palette.line)
        assertEquals(0x7B7C7C, palette.dim)
        assertEquals(0x2472C8, palette.accent)
        assertEquals(0xCD3131, palette.danger)
    }

    @Test
    fun light2026MatchesWebUiColors() {
        val palette = AppearanceCodec.default(false).uiPalette()
        assertTrue(palette.light)
        assertEquals(0xFAFAFD, palette.background)
        assertEquals(0x3B3B3B, palette.foreground)
        assertEquals(0xF0F0F3, palette.panel)
        assertEquals(0xE5E5E8, palette.panel2)
        assertEquals(0xD4D4D6, palette.line)
        assertEquals(0x919192, palette.dim)
        assertEquals(0x0451A5, palette.accent)
        assertEquals(0xCD3131, palette.danger)
    }

    @Test
    fun lowContrastAccentUsesBrighterVariant() {
        val theme = AppearanceCodec.default(true)
        val ansi = theme.ansi.toMutableList().apply {
            this[4] = theme.background
            this[12] = 0xFFFFFF
        }
        assertEquals(0xFFFFFF, theme.copy(ansi = ansi).uiPalette().accent)
    }

    @Test
    fun markdownHeadingAndCodeColorsMeetContrastForAllBuiltins() {
        for (theme in AppearanceCodec.builtins()) {
            val palette = theme.uiPalette()
            val headingContrast = contrast(palette.mdHeading, palette.background)
            val codeBgContrast = contrast(palette.mdCode, palette.background)
            val codePanel2Contrast = contrast(palette.mdCode, palette.panel2)
            assertTrue(
                "${theme.name} mdHeading contrast $headingContrast < 3.0",
                headingContrast >= 3.0,
            )
            assertTrue(
                "${theme.name} mdCode vs bg contrast $codeBgContrast < 3.0",
                codeBgContrast >= 3.0,
            )
            assertTrue(
                "${theme.name} mdCode vs panel2 contrast $codePanel2Contrast < 3.0",
                codePanel2Contrast >= 3.0,
            )
        }
    }

    @Test
    fun oneHalfLightInlineCodeMeetsContrastAgainstBackgroundAndPanel2() {
        val oneHalfLight = AppearanceCodec.default(false).copy(
            name = "One Half Light",
            background = 0xFAFAFA,
            foreground = 0x383A42,
            ansi = listOf(
                0x383A42, 0xE45649, 0x50A14F, 0xC18401,
                0x0184BC, 0xA626A4, 0x0997B3, 0xBABABA,
                0x4F525E, 0xE06C75, 0x98C379, 0xD8B36E,
                0x61AFEF, 0xC678DD, 0x56B6C2, 0xFFFFFF,
            ),
        )
        val palette = oneHalfLight.uiPalette()
        assertTrue(contrast(palette.mdCode, palette.background) >= 3.0)
        assertTrue(contrast(palette.mdCode, palette.panel2) >= 3.0)

        // When accent meets 3.0 vs background but not vs panel2, mdCode falls back to foreground
        val bgOnlyAccentAnsi = oneHalfLight.ansi.toMutableList().apply {
            this[2] = 0xD8B36E
            this[3] = 0xD8B36E
            this[6] = 0xD8B36E
            this[10] = 0xD8B36E
            this[11] = 0xD8B36E
            this[14] = 0xD8B36E
            this[4] = 0xC18401
            this[12] = 0xC18401
        }
        val bgOnlyPalette = oneHalfLight.copy(ansi = bgOnlyAccentAnsi).uiPalette()
        assertEquals(bgOnlyPalette.accent, bgOnlyPalette.mdHeading)
        assertEquals(bgOnlyPalette.foreground, bgOnlyPalette.mdCode)
        assertTrue(contrast(bgOnlyPalette.mdCode, bgOnlyPalette.panel2) >= 3.0)
    }

    @Test
    fun syntheticPaleLightThemeFallsBackToAccentAndForeground() {
        // Solarized-Light-like theme where cyan/yellow/green are too low contrast against #FDF6E3
        val paleSolarized = AppearanceCodec.default(false).copy(
            name = "Pale Solarized",
            background = 0xFDF6E3,
            foreground = 0x586E75,
            ansi = listOf(
                0x073642, 0xDC322F, 0x93A1A1, 0xEEE8D5,
                0x1D6FA5, 0xD33682, 0x93A1A1, 0xEEE8D5,
                0x002B36, 0xCB4B16, 0xEEE8D5, 0xFDF6E3,
                0x839496, 0x6C71C4, 0xEEE8D5, 0xFDF6E3,
            ),
        )
        val palette = paleSolarized.uiPalette()
        assertEquals(palette.accent, palette.mdHeading)
        assertEquals(palette.accent, palette.mdCode)
        assertTrue(contrast(palette.mdHeading, palette.background) >= 3.0)
        assertTrue(contrast(palette.mdCode, palette.background) >= 3.0)
        assertTrue(contrast(palette.mdCode, palette.panel2) >= 3.0)

        // When even accent (blue/brightBlue) is < 3.0 contrast, fall back to foreground
        val allPaleAnsi = paleSolarized.ansi.toMutableList().apply {
            this[4] = 0xEEE8D5
            this[12] = 0xFDF6E3
        }
        val extremePalette = paleSolarized.copy(ansi = allPaleAnsi).uiPalette()
        assertEquals(extremePalette.foreground, extremePalette.mdHeading)
        assertEquals(extremePalette.foreground, extremePalette.mdCode)
        assertTrue(contrast(extremePalette.mdHeading, extremePalette.background) >= 3.0)
        assertTrue(contrast(extremePalette.mdCode, extremePalette.background) >= 3.0)
        assertTrue(contrast(extremePalette.mdCode, extremePalette.panel2) >= 3.0)
    }
}
