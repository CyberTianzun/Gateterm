package ai.eclosion.gateterm.android.onboarding

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.URI

class OnboardingConfigTest {
    @Test
    fun releasesUrlMatchesGatetermGitHubRepository() {
        val uri = URI(OnboardingConfig.RELEASES_URL)
        assertEquals("https", uri.scheme)
        assertEquals("github.com", uri.host)
        assertEquals("/CyberTianzun/Gateterm/releases/latest", uri.path)
        assertTrue(OnboardingConfig.RELEASES_URL.endsWith(OnboardingConfig.RELEASES_DISPLAY_URL))
    }

    @Test
    fun desktopSettingsHighlightAnchorsAreValidAndNonOverlapping() {
        val listen = OnboardingConfig.DESKTOP_SETTINGS_LISTEN_RECT
        val qr = OnboardingConfig.DESKTOP_SETTINGS_QR_RECT
        assertTrue("listen rect out of bounds: $listen", listen.isValid())
        assertTrue("qr rect out of bounds: $qr", qr.isValid())
        assertFalse("highlight rects must not overlap", listen.overlaps(qr))
        assertTrue(OnboardingConfig.DESKTOP_SETTINGS_ASPECT_RATIO > 1f)
        assertEquals(4, OnboardingConfig.STEP_COUNT)
    }
}
