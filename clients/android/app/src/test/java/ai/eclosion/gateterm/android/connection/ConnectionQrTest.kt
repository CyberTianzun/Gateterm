package ai.eclosion.gateterm.android.connection

import ai.eclosion.gateterm.android.ui.connections.ConnectionController
import ai.eclosion.gateterm.android.ui.connections.EditorState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class ConnectionQrTest {
    @Test fun desktopLinkFillsAddressAndTokenSeparately() {
        val result = ConnectionQr.parse("http://192.168.1.2:49152/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81")
        assertEquals("ws://192.168.1.2:49152/ws", result.url)
        assertEquals("a+b &=#密码", result.token)
        assertEquals("192.168.1.2:49152", result.name)

        val titled = ConnectionQr.parse(
            "http://192.168.1.2:49152/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81&title=%E7%A0%94%E5%8F%91+1%E5%8F%B7%2Ba%26b%3D%23%E6%B5%8B%E8%AF%95",
        )
        assertEquals(
            ScannedConnection(
                url = "ws://192.168.1.2:49152/ws",
                token = "a+b &=#密码",
                name = "研发 1号+a&b=#测试",
            ),
            titled,
        )

        assertEquals(
            "192.168.1.2:7683",
            ConnectionQr.parse("http://192.168.1.2:7683/#token=t&title=%20%0D%0A").name,
        )
        assertEquals(
            "Query Box",
            ConnectionQr.parse("http://192.168.1.2:7683/?token=t&title=Query+Box").name,
        )
        assertEquals(
            "Fragment Box",
            ConnectionQr.parse("http://192.168.1.2:7683/?token=t&title=Query+Box#token=t&title=Fragment+Box").name,
        )
        val emoji130 = "%F0%9F%9A%80".repeat(130)
        assertEquals(
            "🚀".repeat(128),
            ConnectionQr.parse("http://192.168.1.2:7683/#token=t&title=$emoji130").name,
        )
    }

    @Test fun prefillAndScanPopulateTitleWhenBlankAndPreserveExistingName() {
        val titledUrl = "http://192.168.1.2:7683/#token=secret&title=Studio+Box"
        val blankPrefill = ConnectionController.applyPrefillState(EditorState(), titledUrl)
        assertEquals("Studio Box", blankPrefill.name)
        assertEquals("secret", blankPrefill.token)

        val fallbackPrefill = ConnectionController.applyPrefillState(
            EditorState(),
            "http://192.168.1.2:7683/#token=secret&title=%20%0D%0A",
        )
        assertEquals("192.168.1.2:7683", fallbackPrefill.name)

        val preservedPrefill = ConnectionController.applyPrefillState(
            EditorState(name = "My Custom Server"),
            titledUrl,
        )
        assertEquals("My Custom Server", preservedPrefill.name)

        val scanned = ConnectionQr.parse(titledUrl)
        assertEquals("Studio Box", ConnectionController.applyScannedState(EditorState(), scanned).name)
        assertEquals(
            "Existing Server",
            ConnectionController.applyScannedState(EditorState(name = "Existing Server"), scanned).name,
        )
    }

    @Test fun ipv6AndSecureHostsKeepTheirTransport() {
        assertEquals("ws://[fd00::2]:9000/ws", ConnectionQr.parse("http://[fd00::2]:9000/#token=t").url)
        assertEquals("wss://term.example:9443/ws", ConnectionQr.parse("https://term.example:9443/#token=t").url)
        assertEquals("", ConnectionQr.parse("http://192.168.1.2:7683/").token)
    }

    @Test fun rejectsNonConnectionCodesAndNonRoutableTargets() {
        for (raw in listOf("not a url", "javascript:alert(1)", "file:///etc/passwd", "http://0.0.0.0/",
            "http://127.0.0.1/", "http://127.4.3.2/", "http://localhost/", "http://[::1]/", "http://[::]/",
            "http://user:pass@192.168.1.2/", "http://host:0/", "http://host:70000/", "http://host/#token=%ZZ",
            "http://host/#token=%FF", "http://192.168.1.2:7683/#token=ok&title=%ZZ",
            "http://192.168.1.2:7683/#token=ok&title=%FF",
            "http://host/#token=" + "x".repeat(8192))) {
            val err = assertThrows(raw, ConnectionUrlException::class.java) { ConnectionQr.parse(raw) }
            assertEquals(raw, "qr", err.message)
        }
    }
}
