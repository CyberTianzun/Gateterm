package ai.eclosion.gateterm.android.ui.term

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.sp
import ai.eclosion.gateterm.android.appearance.AppearanceCodec
import ai.eclosion.gateterm.android.appearance.uiPalette
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.gateterm_cloud_ffi.FfiMdAlign
import uniffi.gateterm_cloud_ffi.FfiMdBlock
import uniffi.gateterm_cloud_ffi.FfiMdRun
import uniffi.gateterm_cloud_ffi.parseMarkdown

class ChatMarkdownTest {
    @Test
    fun parsesHeadingsCodeBlocksTablesNestedListsAndSanitizesUnsafeLinksViaFfi() {
        val md = """
            # H1 Title
            ### H3 Title

            ```rust
            fn main() {}
            ```

            - [x] done
            - [ ] todo
              - nested item

            | Left | Center | Right |
            |:-----|:------:|------:|
            | **a** | `b` | c |

            [ok](https://example.com) [xss](javascript:alert(1)) [data](data:text/html,x)
        """.trimIndent()

        val doc = parseMarkdown(md)
        val rootBlocks = doc.root.map { doc.blocks[it.toInt()] }

        val headings = rootBlocks.filterIsInstance<FfiMdBlock.Heading>()
        assertEquals(2, headings.size)
        assertEquals(1.toUByte(), headings[0].level)
        assertEquals("H1 Title", headings[0].runs.joinToString("") { it.text })
        assertEquals(3.toUByte(), headings[1].level)

        val codeBlock = rootBlocks.filterIsInstance<FfiMdBlock.CodeBlock>().single()
        assertEquals("rust", codeBlock.lang)
        assertEquals("fn main() {}", codeBlock.code)

        val listBlock = rootBlocks.filterIsInstance<FfiMdBlock.ListBlock>().single()
        assertEquals(false, listBlock.ordered)
        assertEquals(2, listBlock.items.size)
        assertEquals(true, listBlock.items[0].checked)
        assertEquals(false, listBlock.items[1].checked)
        assertTrue(
            listBlock.items[1].children.any { doc.blocks[it.toInt()] is FfiMdBlock.ListBlock },
        )

        val table = rootBlocks.filterIsInstance<FfiMdBlock.Table>().single()
        assertEquals(listOf(FfiMdAlign.LEFT, FfiMdAlign.CENTER, FfiMdAlign.RIGHT), table.alignments)
        assertEquals(3, table.headers.size)
        assertEquals(1, table.rows.size)
        assertTrue(table.rows[0][0].runs.single().bold)
        assertTrue(table.rows[0][1].runs.single().code)

        val linkParagraph = rootBlocks.filterIsInstance<FfiMdBlock.Paragraph>().last()
        val okRun = linkParagraph.runs.first { it.text == "ok" }
        assertEquals("https://example.com", okRun.link)
        assertTrue(linkParagraph.runs.none { it.link?.startsWith("javascript:", ignoreCase = true) == true })
        assertTrue(linkParagraph.runs.none { it.link?.startsWith("data:", ignoreCase = true) == true })
    }

    @Test
    fun buildAnnotatedStringStylesInlineRunsAndBlocksUnsafeLinks() {
        val palette = AppearanceCodec.default(true).uiPalette()
        val runs = listOf(
            FfiMdRun("bold", bold = true, italic = false, strikethrough = false, code = false, link = null),
            FfiMdRun(" ", bold = false, italic = false, strikethrough = false, code = false, link = null),
            FfiMdRun("both", bold = true, italic = true, strikethrough = false, code = false, link = null),
            FfiMdRun(" ", bold = false, italic = false, strikethrough = false, code = false, link = null),
            FfiMdRun("del", bold = false, italic = false, strikethrough = true, code = false, link = null),
            FfiMdRun(" ", bold = false, italic = false, strikethrough = false, code = false, link = null),
            FfiMdRun("code()", bold = false, italic = false, strikethrough = false, code = true, link = null),
            FfiMdRun(" ", bold = false, italic = false, strikethrough = false, code = false, link = null),
            FfiMdRun("safe", bold = false, italic = false, strikethrough = false, code = false, link = "https://example.com"),
            FfiMdRun(" ", bold = false, italic = false, strikethrough = false, code = false, link = null),
            FfiMdRun("evil", bold = false, italic = false, strikethrough = false, code = false, link = "javascript:alert(1)"),
        )

        val annotated = buildMarkdownAnnotatedString(runs, palette, 13.sp)
        assertEquals("bold both del code() safe evil", annotated.text)

        val boldStart = annotated.text.indexOf("bold")
        assertTrue(
            annotated.spanStyles.any {
                it.start <= boldStart && it.end >= boldStart + 4 &&
                    it.item.fontWeight == FontWeight.Bold &&
                    it.item.fontFamily == FontFamily.Monospace
            },
        )

        val bothStart = annotated.text.indexOf("both")
        assertTrue(
            annotated.spanStyles.any {
                it.start <= bothStart && it.end >= bothStart + 4 &&
                    it.item.fontWeight == FontWeight.Bold &&
                    it.item.fontStyle == FontStyle.Italic
            },
        )

        val delStart = annotated.text.indexOf("del")
        assertTrue(
            annotated.spanStyles.any {
                it.start <= delStart && it.end >= delStart + 3 &&
                    it.item.textDecoration == TextDecoration.LineThrough
            },
        )

        val codeStart = annotated.text.indexOf("code()")
        assertTrue(
            annotated.spanStyles.any {
                it.start <= codeStart && it.end >= codeStart + 6 &&
                    it.item.color == Color(palette.mdCode or 0xFF000000.toInt()) &&
                    it.item.background == Color(palette.panel2 or 0xFF000000.toInt())
            },
        )

        val links = annotated.getLinkAnnotations(0, annotated.length)
        assertEquals(1, links.size)
        val linkRange = links.single()
        val url = (linkRange.item as LinkAnnotation.Url).url
        assertEquals("https://example.com", url)
        assertEquals("safe", annotated.text.substring(linkRange.start, linkRange.end))
    }

    @Test
    fun sanitizeLinkUrlRejectsDangerousAndMalformedSchemes() {
        assertEquals("https://example.com/a", sanitizeLinkUrl("  https://example.com/a  "))
        assertEquals("HTTP://EXAMPLE.COM", sanitizeLinkUrl("HTTP://EXAMPLE.COM"))
        assertEquals("mailto:test@example.com", sanitizeLinkUrl("mailto:test@example.com"))

        assertNull(sanitizeLinkUrl(null))
        assertNull(sanitizeLinkUrl(""))
        assertNull(sanitizeLinkUrl("   "))
        assertNull(sanitizeLinkUrl("https://"))
        assertNull(sanitizeLinkUrl("mailto:"))
        assertNull(sanitizeLinkUrl("javascript:alert(1)"))
        assertNull(sanitizeLinkUrl("data:text/html,<script>alert(1)</script>"))
        assertNull(sanitizeLinkUrl("file:///etc/passwd"))
        assertNull(sanitizeLinkUrl("/relative/path"))
        assertNull(sanitizeLinkUrl("https://example.com/\nbad"))
        assertNull(sanitizeLinkUrl("java\tscript:alert(1)"))
    }

    @Test
    fun headingSizesAreCompactAndDistinctForH1H2AndOthers() {
        assertEquals(15.sp, headingFontSize(1))
        assertEquals(14.sp, headingFontSize(2))
        assertEquals(13.sp, headingFontSize(3))
        assertEquals(13.sp, headingFontSize(6))
    }

    @Test
    fun opaqueColorPreservesRgbAndSetsFullAlpha() {
        val c = 0x191A1B.toOpaqueColor()
        assertNotNull(c)
        assertEquals(1f, c.alpha)
        assertEquals(Color(0xFF191A1B), c)
    }

    @Test
    fun flattenDeepBlocksIterativelyCollectsLeavesAndTaskStrikeThrough() {
        val quoteDoc = parseMarkdown("> ".repeat(64) + "leaf")
        val flatQuote = flattenDeepBlocks(quoteDoc.root, quoteDoc, inheritedStrikeThrough = false)
        assertEquals(1, flatQuote.size)
        assertEquals(false, flatQuote[0].second)
        val leafParagraph = quoteDoc.blocks[flatQuote[0].first] as FfiMdBlock.Paragraph
        assertEquals("leaf", leafParagraph.runs.joinToString("") { it.text })

        val extremeDoc = parseMarkdown("> ".repeat(200) + "deepest")
        val flatExtreme = flattenDeepBlocks(extremeDoc.root, extremeDoc, inheritedStrikeThrough = false)
        assertEquals(1, flatExtreme.size)
        assertEquals("deepest", (extremeDoc.blocks[flatExtreme[0].first] as FfiMdBlock.Paragraph).runs.joinToString("") { it.text })

        val taskDoc = parseMarkdown("> ".repeat(20) + "- [x] done\n" + "> ".repeat(20) + "- [ ] todo")
        val flatTasks = flattenDeepBlocks(taskDoc.root, taskDoc, inheritedStrikeThrough = false)
        assertEquals(2, flatTasks.size)
        assertEquals(true, flatTasks[0].second)
        assertEquals(false, flatTasks[1].second)
        assertEquals("done", (taskDoc.blocks[flatTasks[0].first] as FfiMdBlock.Paragraph).runs.joinToString("") { it.text })
        assertEquals("todo", (taskDoc.blocks[flatTasks[1].first] as FfiMdBlock.Paragraph).runs.joinToString("") { it.text })

        val listDoc = parseMarkdown((0 until 40).joinToString("\n") { i -> "${"  ".repeat(i)}- item $i" })
        val flatList = flattenDeepBlocks(listDoc.root, listDoc, inheritedStrikeThrough = false)
        assertEquals(40, flatList.size)
        assertEquals("item 39", (listDoc.blocks[flatList[39].first] as FfiMdBlock.Paragraph).runs.joinToString("") { it.text })
    }
}
