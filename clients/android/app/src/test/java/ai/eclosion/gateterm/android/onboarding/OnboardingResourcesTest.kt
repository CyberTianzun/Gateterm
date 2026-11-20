package ai.eclosion.gateterm.android.onboarding

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.charset.StandardCharsets
import javax.xml.parsers.DocumentBuilderFactory

class OnboardingResourcesTest {
    private val resDir: File by lazy {
        listOf(File("src/main/res"), File("app/src/main/res"))
            .firstOrNull { it.isDirectory }
            ?: error("Could not locate src/main/res from working dir ${File(".").absolutePath}")
    }

    @Test
    fun requiredStringsExistInBothLocalesWithMatchingPlaceholders() {
        val zhStrings = parseStringsXml(File(resDir, "values/strings.xml"))
        val enStrings = parseStringsXml(File(resDir, "values-en/strings.xml"))

        val requiredKeys = listOf(
            "onboarding_skip",
            "onboarding_close",
            "onboarding_next",
            "onboarding_prev",
            "onboarding_done",
            "onboarding_enter_app",
            "onboarding_scan_now",
            "onboarding_step_1_badge",
            "onboarding_step_1_title",
            "onboarding_step_1_body",
            "onboarding_step_1_host_status",
            "onboarding_step_1_link_label",
            "onboarding_step_1_phone_synced",
            "onboarding_step_2_badge",
            "onboarding_step_2_title",
            "onboarding_step_2_body",
            "onboarding_step_2_callout",
            "onboarding_step_2_desktop_desc",
            "onboarding_step_2_server_desc",
            "onboarding_copy_link",
            "onboarding_share_link",
            "onboarding_open_link",
            "onboarding_share_text",
            "onboarding_step_3_badge",
            "onboarding_step_3_title",
            "onboarding_step_3_body",
            "onboarding_step_3_window_title",
            "onboarding_step_3_callout_listen",
            "onboarding_step_3_callout_qr",
            "onboarding_step_3_screenshot_desc",
            "onboarding_step_4_badge",
            "onboarding_step_4_title",
            "onboarding_step_4_body",
            "onboarding_step_4_auto_fill",
            "servers_empty_guide",
            "servers_empty_body",
            "settings_guide_title",
            "settings_guide_hint",
        )

        for (key in requiredKeys) {
            val zh = zhStrings[key]
            val en = enStrings[key]
            assertNotNull("Missing zh-CN string for key '$key'", zh)
            assertNotNull("Missing en string for key '$key'", en)
            assertFalse("Blank zh-CN string for key '$key'", zh!!.isBlank())
            assertFalse("Blank en string for key '$key'", en!!.isBlank())
            assertEquals(
                "Format placeholders differ for key '$key'",
                extractPlaceholders(zh),
                extractPlaceholders(en),
            )
        }

        val zhEmptyBody = zhStrings.getValue("servers_empty_body")
        val enEmptyBody = enStrings.getValue("servers_empty_body")
        assertTrue(zhEmptyBody.contains("Gateterm-Desktop-Web") && zhEmptyBody.contains("Gateterm-Server"))
        assertTrue(enEmptyBody.contains("Gateterm-Desktop-Web") && enEmptyBody.contains("Gateterm-Server"))
    }

    @Test
    fun webpScreenshotsExistWithinSizeBudgetAndMatchExpectedDimensions() {
        val terminal = File(resDir, "drawable-nodpi/tutorial_web_terminal.webp")
        val settingsZh = File(resDir, "drawable-nodpi/tutorial_desktop_settings.webp")
        val settingsEn = File(resDir, "drawable-en-nodpi/tutorial_desktop_settings.webp")

        verifyWebpAsset(file = terminal, maxBytes = 40_960, expectedWidth = 800, expectedHeight = 500)
        verifyWebpAsset(file = settingsZh, maxBytes = 122_880, expectedWidth = 1120, expectedHeight = 880)
        verifyWebpAsset(file = settingsEn, maxBytes = 122_880, expectedWidth = 1120, expectedHeight = 880)

        assertEquals(1120f / 880f, OnboardingConfig.DESKTOP_SETTINGS_ASPECT_RATIO, 0.0001f)
    }

    @Test
    fun vectorDrawablesExistAndHave24dpViewport() {
        val icons = listOf(
            "drawable/ic_content_copy.xml",
            "drawable/ic_open_in_new.xml",
            "drawable/ic_guide.xml",
        )
        for (relPath in icons) {
            val file = File(resDir, relPath)
            assertTrue("Missing vector drawable: ${file.path}", file.isFile)
            val doc = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
            val root = doc.documentElement
            assertEquals("vector", root.tagName)
            assertEquals("24dp", root.getAttribute("android:width"))
            assertEquals("24dp", root.getAttribute("android:height"))
            assertEquals("24", root.getAttribute("android:viewportWidth"))
            assertEquals("24", root.getAttribute("android:viewportHeight"))
        }
    }

    private fun verifyWebpAsset(
        file: File,
        maxBytes: Long,
        expectedWidth: Int,
        expectedHeight: Int,
    ) {
        assertTrue("Missing WebP asset: ${file.path}", file.isFile)
        val bytes = file.readBytes()
        assertTrue(
            "WebP asset ${file.name} exceeds size budget (${bytes.size} >= $maxBytes)",
            bytes.size < maxBytes,
        )
        assertTrue("WebP header too short for ${file.name}", bytes.size >= 30)
        assertEquals("RIFF", String(bytes, 0, 4, StandardCharsets.US_ASCII))
        assertEquals("WEBP", String(bytes, 8, 4, StandardCharsets.US_ASCII))
        val (width, height) = readWebpDimensions(bytes)
        assertEquals("Unexpected width for ${file.name}", expectedWidth, width)
        assertEquals("Unexpected height for ${file.name}", expectedHeight, height)
    }

    private fun readWebpDimensions(bytes: ByteArray): Pair<Int, Int> {
        val chunk = String(bytes, 12, 4, StandardCharsets.US_ASCII)
        return when (chunk) {
            "VP8 " -> {
                assertEquals(0x9D, bytes[23].toInt() and 0xFF)
                assertEquals(0x01, bytes[24].toInt() and 0xFF)
                assertEquals(0x2A, bytes[25].toInt() and 0xFF)
                val w = ((bytes[26].toInt() and 0xFF) or ((bytes[27].toInt() and 0xFF) shl 8)) and 0x3FFF
                val h = ((bytes[28].toInt() and 0xFF) or ((bytes[29].toInt() and 0xFF) shl 8)) and 0x3FFF
                w to h
            }
            "VP8L" -> {
                assertEquals(0x2F, bytes[20].toInt() and 0xFF)
                val b0 = bytes[21].toInt() and 0xFF
                val b1 = bytes[22].toInt() and 0xFF
                val b2 = bytes[23].toInt() and 0xFF
                val b3 = bytes[24].toInt() and 0xFF
                val bits = b0 or (b1 shl 8) or (b2 shl 16) or (b3 shl 24)
                val w = (bits and 0x3FFF) + 1
                val h = ((bits ushr 14) and 0x3FFF) + 1
                w to h
            }
            "VP8X" -> {
                val w = ((bytes[24].toInt() and 0xFF) or
                    ((bytes[25].toInt() and 0xFF) shl 8) or
                    ((bytes[26].toInt() and 0xFF) shl 16)) + 1
                val h = ((bytes[27].toInt() and 0xFF) or
                    ((bytes[28].toInt() and 0xFF) shl 8) or
                    ((bytes[29].toInt() and 0xFF) shl 16)) + 1
                w to h
            }
            else -> error("Unsupported WebP chunk type: $chunk")
        }
    }

    private fun parseStringsXml(file: File): Map<String, String> {
        assertTrue("Missing strings file: ${file.path}", file.isFile)
        val doc = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
        val nodes = doc.getElementsByTagName("string")
        val map = LinkedHashMap<String, String>(nodes.length)
        for (i in 0 until nodes.length) {
            val node = nodes.item(i)
            val name = node.attributes?.getNamedItem("name")?.nodeValue ?: continue
            map[name] = node.textContent ?: ""
        }
        return map
    }

    private fun extractPlaceholders(text: String): List<String> =
        Regex("%(?:\\d+\\$)?(?:\\.\\d+)?[sdfoxegc]")
            .findAll(text)
            .map { it.value }
            .toList()
}
