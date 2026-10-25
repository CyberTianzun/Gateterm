package ai.eclosion.octoterm.android.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class ConnectionQrTest {
    @Test fun desktopLinkFillsAddressAndTokenSeparately() {
        val result = ConnectionQr.parse("http://192.168.1.2:49152/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81")
        assertEquals("ws://192.168.1.2:49152/ws", result.url)
        assertEquals("a+b &=#密码", result.token)
        assertEquals("192.168.1.2:49152", result.name)
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
            "http://host/#token=" + "x".repeat(8192))) {
            assertThrows(raw, ConnectionUrlException::class.java) { ConnectionQr.parse(raw) }
        }
    }
}
