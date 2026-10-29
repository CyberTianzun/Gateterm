package ai.eclosion.gateterm.android.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.charset.StandardCharsets

class FrameAndControlTest {
    @Test
    fun frameRoundtrip() {
        val encoded = Frame.encode(7, "hello".toByteArray())
        val decoded = Frame.decode(encoded)
        assertEquals(7, decoded.channel)
        assertEquals(0, decoded.flags)
        assertEquals("hello", String(decoded.payload, StandardCharsets.UTF_8))
    }

    @Test
    fun helloEscapesTokenQuotes() {
        val bytes = ControlJson.encodeHello("a\"b")
        val decoded = Frame.decode(bytes)
        assertEquals(0, decoded.channel)
        val json = String(decoded.payload, StandardCharsets.UTF_8)
        assertTrue(json.contains("\"type\":\"hello\""))
        assertTrue(json.contains("\"token\":\"a\\\"b\""))
        assertTrue(json.contains("\"proto\":1"))
    }

    @Test
    fun attachAndResizeCarryColorsOnlyWhenGiven() {
        fun json(msg: ClientMsg) = String(Frame.decode(ControlJson.encode(msg)).payload, StandardCharsets.UTF_8)
        val colors = TermColors.fromRgb(0xD4D4D4, 0x191A1B)
        assertEquals(TermColors("#d4d4d4", "#191a1b"), colors)
        assertTrue(json(ClientMsg.Attach(1, 1, null, 80, 24, colors)).contains("\"colors\":{\"fg\":\"#d4d4d4\",\"bg\":\"#191a1b\"}"))
        assertTrue(json(ClientMsg.Resize(1, 80, 24, colors)).contains("\"colors\":{"))
        assertTrue(!json(ClientMsg.Resize(1, 80, 24)).contains("colors"))
    }

    @Test
    fun parseHelloOkAndError() {
        val ok = ControlJson.parse("""{"type":"hello-ok","proto":1}""".toByteArray())
        assertEquals(ServerMsg.HelloOk(1), ok)
        assertEquals(ServerMsg.HelloOk(1, "windows"), ControlJson.parse(
            """{"type":"hello-ok","proto":1,"os":"windows"}""".toByteArray(),
        ))
        val err = ControlJson.parse("""{"type":"error","message":"bad hello"}""".toByteArray())
        assertEquals(ServerMsg.Error("bad hello", null), err)
    }

    @Test
    fun helloOkInputCapabilityDefaultsToVtAndAcceptsExplicitSupport() {
        for (field in listOf("", ",\"win32_input_mode\":false", ",\"win32_input_mode\":true")) {
            val msg = ControlJson.parse("""{"type":"hello-ok","proto":1,"os":"windows"$field}""".toByteArray()) as ServerMsg.HelloOk
            assertEquals(field.endsWith("true"), msg.win32InputMode)
        }
    }

    @Test
    fun parseSessionsAndResized() {
        val sessions = ControlJson.parse(
            """{"type":"sessions","sessions":[{"id":2,"name":"zsh","cols":80,"rows":24,"created_at":1,"cwd":"/Users/alice/work/gateterm"}]}""".toByteArray(),
        ) as ServerMsg.Sessions
        assertEquals(1, sessions.sessions.size)
        assertEquals(2L, sessions.sessions[0].id)
        assertEquals("/Users/alice/work/gateterm", sessions.sessions[0].cwd)
        val resized = ControlJson.parse("""{"type":"resized","channel":1,"cols":100,"rows":30}""".toByteArray())
        assertEquals(ServerMsg.Resized(1, 100, 30), resized)
    }

    @Test
    fun encodeAttachNullSeq() {
        val bytes = ControlJson.encode(ClientMsg.Attach(3, 1, null, 80, 24))
        val json = String(Frame.decode(bytes).payload, StandardCharsets.UTF_8)
        assertTrue(json.contains("\"type\":\"attach\""))
        assertTrue(json.contains("\"last_seq\":null"))
        assertNull((ControlJson.parse("""{"type":"error","message":"x"}""".toByteArray()) as ServerMsg.Error).channel)
    }

    @Test(expected = IllegalArgumentException::class)
    fun shortFrameRejected() {
        Frame.decode(byteArrayOf(1, 2, 3))
    }

    @Test
    fun helloOkCarriesCloudServerId() {
        val with = ControlJson.parse("""{"type":"hello-ok","proto":1,"os":"macos","cloud_server_id":"srv-1"}""".toByteArray())
        assertEquals(ServerMsg.HelloOk(1, "macos", "srv-1"), with)
        val without = ControlJson.parse("""{"type":"hello-ok","proto":1}""".toByteArray())
        assertEquals(ServerMsg.HelloOk(1, "", null), without)
    }
}
