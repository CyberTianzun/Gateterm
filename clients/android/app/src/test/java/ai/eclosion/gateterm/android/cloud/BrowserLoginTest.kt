package ai.eclosion.gateterm.android.cloud

import java.net.URI
import java.net.URLDecoder
import org.junit.Assert.*
import org.junit.Test

class BrowserLoginTest {
    private val pending = PendingCloudLogin.create(RelayRegion.China, "zh-CN", "Pixel", now = 1000L)

    @Test
    fun pkceMatchesTheRfcVectorAndOnlyTheChallengeLeavesTheApp() {
        assertEquals("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM", pkceChallenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"))
        val url = pending.authorizationUrl()
        val query = URI(url).rawQuery.split('&').associate {
            val pair = it.split('=', limit = 2)
            URLDecoder.decode(pair[0], "UTF-8") to URLDecoder.decode(pair[1], "UTF-8")
        }
        assertEquals("/auth/mobile", URI(url).path)
        assertEquals(CLOUD_CALLBACK_URI, query["redirect_uri"])
        assertEquals(pending.state, query["state"])
        assertEquals("S256", query["code_challenge_method"])
        assertEquals("zh-CN", query["lang"])
        assertFalse(url.contains(pending.verifier))
        assertTrue(pending.verifier.length in 43..128)
        assertNotEquals(pending.state, PendingCloudLogin.create(RelayRegion.China, "en", "Pixel").state)
    }

    @Test
    fun callbackMustMatchTheExactAppPathAndPendingState() {
        val query = "?state=${pending.state}&code=single-use-code"
        assertEquals("single-use-code", callbackCode(CLOUD_CALLBACK_URI + query, pending, 2000L))
        for (base in listOf("https://cloud/auth/callback", "other.app://cloud/auth/callback", "ai.eclosion.gateterm.android://evil/auth/callback", "$CLOUD_CALLBACK_URI/extra", "$CLOUD_CALLBACK_URI#fragment")) {
            assertNull(callbackCode(base + query, pending, 2000L))
        }
        assertNull(callbackCode("$CLOUD_CALLBACK_URI?state=wrong&code=x", pending, 2000L))
        assertNull(callbackCode("$CLOUD_CALLBACK_URI$query&state=${pending.state}", pending, 2000L))
        assertNull(callbackCode("$CLOUD_CALLBACK_URI?state=${pending.state}&code=${"x".repeat(257)}", pending, 2000L))
        assertNull(callbackCode("$CLOUD_CALLBACK_URI?state=${pending.state}", pending, 2000L))
    }

    @Test
    fun expiredOrFutureAttemptsCannotBeCompleted() {
        val callback = "$CLOUD_CALLBACK_URI?state=${pending.state}&code=x"
        assertNotNull(callbackCode(callback, pending, 1000L + BROWSER_LOGIN_TIMEOUT_MS - 1))
        assertNull(callbackCode(callback, pending, 1000L + BROWSER_LOGIN_TIMEOUT_MS))
        assertNull(callbackCode(callback, pending, 999L))
    }
}
