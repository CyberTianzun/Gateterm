package ai.eclosion.gateterm.android.onboarding

import android.content.ActivityNotFoundException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OnboardingLinksTest {
    @Test
    fun sharePayloadIncludesReleasesUrl() {
        val text = OnboardingLinks.sharePayload("Download Gateterm-Desktop-Web: %1\$s")
        assertTrue(text.contains(OnboardingConfig.RELEASES_URL))
        assertEquals("Download Gateterm-Desktop-Web: ${OnboardingConfig.RELEASES_URL}", text)
    }

    @Test
    fun sharePayloadSupportsCustomUrlAndChineseTemplate() {
        val customUrl = "https://github.com/CyberTianzun/Gateterm/releases/tag/v1.0.0"
        val text = OnboardingLinks.sharePayload(
            shareTemplate = "在电脑安装 Gateterm-Desktop-Web（支持 Windows / macOS）：%1\$s",
            url = customUrl,
        )
        assertEquals(
            "在电脑安装 Gateterm-Desktop-Web（支持 Windows / macOS）：$customUrl",
            text,
        )
    }

    @Test
    fun sharePayloadAppendsUrlWhenTemplateLacksPlaceholder() {
        val text = OnboardingLinks.sharePayload("Install Gateterm-Desktop-Web:")
        assertEquals("Install Gateterm-Desktop-Web: ${OnboardingConfig.RELEASES_URL}", text)
    }

    @Test
    fun dispatchSafelyReturnsTrueOnSuccessAndFalseOnFailure() {
        assertTrue(OnboardingLinks.dispatchSafely { })
        assertFalse(OnboardingLinks.dispatchSafely { throw IllegalStateException("no activity") })
        assertFalse(OnboardingLinks.dispatchSafely { throw ActivityNotFoundException("no browser") })
    }
}
