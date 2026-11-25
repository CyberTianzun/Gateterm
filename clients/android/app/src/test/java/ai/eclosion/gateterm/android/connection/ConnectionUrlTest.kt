package ai.eclosion.gateterm.android.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionUrlTest {
    @Test
    fun parsesDaemonStartupLink() {
        val parsed = ConnectionUrl.parse("http://192.168.1.10:7683/#token=s3cret")
        assertEquals("ws://192.168.1.10:7683/ws", parsed.webSocketUrl)
        assertEquals("http://192.168.1.10:7683", parsed.httpOrigin())
        assertEquals("s3cret", parsed.token)
        assertEquals("192.168.1.10:7683", parsed.displayHost)
        assertNull(parsed.title)
        assertFalse(parsed.isLoopback)
    }

    @Test
    fun acceptsBareHostPort() {
        val parsed = ConnectionUrl.parse("10.0.0.2:9000", "abc")
        assertEquals("ws://10.0.0.2:9000/ws", parsed.webSocketUrl)
        assertEquals("abc", parsed.token)
    }

    @Test
    fun httpsBecomesWss() {
        val parsed = ConnectionUrl.parse("https://term.example/#token=x")
        assertEquals("wss://term.example/ws", parsed.webSocketUrl)
        assertEquals("x", parsed.token)
    }

    @Test
    fun keepsExplicitWsPath() {
        val parsed = ConnectionUrl.parse("ws://host.internal:7683/ws#token=t")
        assertEquals("ws://host.internal:7683/ws", parsed.webSocketUrl)
    }

    @Test
    fun overrideWinsOverFragment() {
        val parsed = ConnectionUrl.parse("http://h:1/#token=old", "new")
        assertEquals("new", parsed.token)
    }

    @Test
    fun readsQueryTokenIfSomeonePastesIt() {
        val parsed = ConnectionUrl.parse("http://h:1/?token=from-query")
        assertEquals("from-query", parsed.token)
    }

    @Test
    fun flagsLoopbackAndWildcard() {
        assertTrue(ConnectionUrl.parse("http://127.0.0.1:7683/#token=a").isLoopback)
        assertTrue(ConnectionUrl.parse("http://localhost:7683/#token=a").isLoopback)
        assertTrue(ConnectionUrl.parse("http://0.0.0.0:7683/#token=a").isWildcardBind)
        assertNull(ConnectionUrl.parse("http://192.168.0.4:7683/").token)
    }

    @Test
    fun looksLikeUrl() {
        assertTrue(ConnectionUrl.looksLikeUrl("  https://x/#token=1 "))
        assertFalse(ConnectionUrl.looksLikeUrl("not a url"))
        assertFalse(ConnectionUrl.looksLikeUrl(""))
    }

    @Test(expected = ConnectionUrlException::class)
    fun rejectsEmpty() {
        ConnectionUrl.parse("   ")
    }

    @Test
    fun parsesFragmentTitleWithSpacesSpecialCharsAndCjk() {
        val parsed = ConnectionUrl.parse(
            "http://192.168.1.2:49152/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81&title=%E7%A0%94%E5%8F%91+1%E5%8F%B7%2Ba%26b%3D%23%E6%B5%8B%E8%AF%95",
        )
        assertEquals("a+b &=#密码", parsed.token)
        assertEquals("研发 1号+a&b=#测试", parsed.title)
    }

    @Test
    fun readsTitleFromQueryFallbackAndPrefersFragment() {
        val queryOnly = ConnectionUrl.parse("http://192.168.1.2:7683/?token=t&title=Query+Box")
        assertEquals("t", queryOnly.token)
        assertEquals("Query Box", queryOnly.title)

        val both = ConnectionUrl.parse("http://192.168.1.2:7683/?token=qt&title=Query+Box#token=ft&title=Fragment+Box")
        assertEquals("ft", both.token)
        assertEquals("Fragment Box", both.title)

        val withOverride = ConnectionUrl.parse("http://192.168.1.2:7683/#token=old&title=Studio", "new")
        assertEquals("new", withOverride.token)
        assertEquals("Studio", withOverride.title)
    }

    @Test
    fun normalizesNewlinesAndBlankTitles() {
        assertNull(ConnectionUrl.parse("http://192.168.1.2:7683/#token=t&title=%20%0D%0A").title)
        assertNull(ConnectionUrl.parse("http://192.168.1.2:7683/#token=t&title=").title)
        assertEquals("Mac Studio", ConnectionUrl.parse("http://192.168.1.2:7683/#token=t&title=Mac%0D%0AStudio").title)
        assertEquals("Mac Mini Pro", ConnectionUrl.parse("http://192.168.1.2:7683/#token=t&title=++Mac%0AMini%0DPro++").title)
    }

    @Test
    fun truncatesTitleTo128CodePointsWithoutSplittingSurrogatePairs() {
        val encoded130 = "%F0%9F%9A%80".repeat(130)
        val parsed = ConnectionUrl.parse("http://192.168.1.2:7683/#token=t&title=$encoded130")
        val title = parsed.title!!
        assertEquals(128, title.codePointCount(0, title.length))
        assertEquals("🚀".repeat(128), title)

        val trimmedAfterCap = ConnectionUrl.parse(
            "http://192.168.1.2:7683/#token=t&title=${"a".repeat(127)}+b",
        )
        assertEquals("a".repeat(127), trimmedAfterCap.title)
    }

    @Test
    fun rejectsMalformedPercentEncodingAndInvalidUtf8InTokenOrTitle() {
        for (bad in listOf(
            "http://192.168.1.2:7683/#token=ok&title=%ZZ",
            "http://192.168.1.2:7683/#token=ok&title=%FF",
            "http://192.168.1.2:7683/#token=ok&title=%E",
            "http://192.168.1.2:7683/#token=%ZZ",
            "http://192.168.1.2:7683/#token=%FF",
        )) {
            val err = assertThrows(bad, ConnectionUrlException::class.java) { ConnectionUrl.parse(bad) }
            assertEquals(bad, "malformed", err.message)
        }
    }
}
