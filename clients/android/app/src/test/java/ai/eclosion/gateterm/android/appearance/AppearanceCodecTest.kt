package ai.eclosion.gateterm.android.appearance

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AppearanceCodecTest {
    @Test
    fun defaultsSelect2026ThemeForSystemMode() {
        assertEquals(AppearanceCodec.builtin("2026 Light"), AppearanceCodec.default(prefersDark = false))
        assertEquals(AppearanceCodec.builtin("2026 Dark"), AppearanceCodec.default(prefersDark = true))
        assertEquals(TermFontKind.JetBrainsMono, AppearanceCodec.default().font)
        assertEquals(14f, AppearanceCodec.default().fontSize)
        assertEquals(1.0, AppearanceCodec.default().lineHeight, 0.0)
        assertEquals(0.0, AppearanceCodec.default().letterSpacing, 0.0)
    }

    @Test
    fun systemThemeChangesKeepFontAndScrollback() {
        val customized = AppearanceCodec.default(false)
            .withFontSize(18f).withFont(TermFontKind.GoogleSansCode).withScrollback(5000)
            .withLineHeight(1.2).withLetterSpacing(1.5)
        val dark = customized.withTheme(AppearanceCodec.default(true))
        assertEquals("2026 Dark", dark.name)
        assertEquals(18f, dark.fontSize)
        assertEquals(TermFontKind.GoogleSansCode, dark.font)
        assertEquals(5000, dark.scrollback)
        assertEquals(1.2, dark.lineHeight, 0.0)
        assertEquals(1.5, dark.letterSpacing, 0.0)
        assertEquals(customized, dark.withTheme(AppearanceCodec.default(false)))
    }

    @Test
    fun roundTripKeepsColorsSizeFontAndScrollback() {
        val theme = AppearanceCodec.builtin("2026 Dark")!!
        for (kind in TermFontKind.entries) {
            val edited = theme.withFontSize(18f).withScrollback(5000).withFont(kind)
                .withLineHeight(1.15).withLetterSpacing(-0.5)
            val back = AppearanceCodec.importJson(AppearanceCodec.exportJson(edited))!!
            assertEquals(edited.name, back.name)
            assertEquals(edited.foreground, back.foreground)
            assertEquals(edited.background, back.background)
            assertEquals(edited.ansi, back.ansi)
            assertEquals(18f, back.fontSize)
            assertEquals(kind, back.font)
            assertEquals(5000, back.scrollback)
            assertEquals(edited, back)
        }
    }

    @Test
    fun webFontStacksImportAsMatchingBuiltinFont() {
        assertEquals(
            TermFontKind.JetBrainsMono,
            AppearanceCodec.importJson("""{"font":{"family":"\"JetBrains Mono\", Consolas, monospace"}}""")!!.font,
        )
        assertEquals(
            TermFontKind.GoogleSansCode,
            AppearanceCodec.importJson("""{"font":{"family":"\"Google Sans Code\", monospace"}}""")!!.font,
        )
        assertEquals(
            TermFontKind.ZedMono,
            AppearanceCodec.importJson("""{"font":{"family":"\"Zed Mono\", monospace"}}""")!!.font,
        )
        assertEquals(
            TermFontKind.Mono,
            AppearanceCodec.importJson("""{"font":{"family":"Consolas, monospace"}}""")!!.font,
        )
    }

    @Test
    fun badFieldsFallBackAndGarbageIsRejected() {
        assertNull(AppearanceCodec.importJson("not json"))
        val base = AppearanceCodec.default().withFontSize(16f)
        val imported = AppearanceCodec.importJson(
            """{"theme":{"name":"2026 Light","colors":{"background":"red","foreground":"#112233"}},"font":{"size":99},"terminal":{"scrollback":-4}}""",
            base,
        )!!
        assertEquals("2026 Light", imported.name)
        assertEquals(0x112233, imported.foreground)
        assertEquals(base.background, imported.background)
        assertEquals(48f, imported.fontSize)
        assertEquals(0, imported.scrollback)
    }

    @Test
    fun themeNameAloneSelectsBuiltin() {
        val imported = AppearanceCodec.importJson("""{"theme":{"name":"Tokyo Night"}}""")!!
        assertEquals(AppearanceCodec.builtin("Tokyo Night")!!.background, imported.background)
    }

    @Test
    fun importsWebSpacingAndKeepsExistingValuesWhenMissingOrInvalid() {
        val base = AppearanceCodec.default().withFontSize(16f).withLineHeight(1.2).withLetterSpacing(2.0)
        val old = AppearanceCodec.importJson("""{"font":{"size":12},"theme":{"name":"Tokyo Night"}}""", base)!!
        assertEquals(12f, old.fontSize)
        assertEquals(base.lineHeight, old.lineHeight, 0.0)
        assertEquals(base.letterSpacing, old.letterSpacing, 0.0)
        val imported = AppearanceCodec.importJson("""{"font":{"lineHeight":1.15,"letterSpacing":-0.5}}""", base)!!
        assertEquals(1.15, imported.lineHeight, 0.0)
        assertEquals(-0.5, imported.letterSpacing, 0.0)
        val invalid = AppearanceCodec.importJson("""{"font":{"size":"NaN","lineHeight":"NaN","letterSpacing":"Infinity"}}""", base)!!
        assertEquals(base, invalid)
        val clamped = AppearanceCodec.importJson("""{"font":{"lineHeight":100,"letterSpacing":-99}}""", base)!!
        assertEquals(3.0, clamped.lineHeight, 0.0)
        assertEquals(-5.0, clamped.letterSpacing, 0.0)
    }
}
