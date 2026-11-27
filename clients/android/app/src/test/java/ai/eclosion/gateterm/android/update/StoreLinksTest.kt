package ai.eclosion.gateterm.android.update

import org.junit.Assert.assertEquals
import org.junit.Test

class StoreLinksTest {
    @Test
    fun buildsGooglePlayMarketAndHttpsWebUrlsForAppPackage() {
        assertEquals("market://details?id=ai.eclosion.gateterm.android", StoreLinks.marketUri())
        assertEquals(
            "https://play.google.com/store/apps/details?id=ai.eclosion.gateterm.android",
            StoreLinks.webUrl(),
        )
    }

    @Test
    fun buildsGooglePlayUrlsForExplicitPackageId() {
        assertEquals("market://details?id=com.example.app", StoreLinks.marketUri("com.example.app"))
        assertEquals(
            "https://play.google.com/store/apps/details?id=com.example.app",
            StoreLinks.webUrl("com.example.app"),
        )
    }
}
