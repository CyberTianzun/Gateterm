package ai.eclosion.gateterm.android.update

import org.junit.Assert.assertEquals
import org.junit.Test

class UpdateCheckerTest {
    private fun releaseJson(
        tag: String,
        url: String = "https://github.com/CyberTianzun/Gateterm/releases/tag/$tag",
        draft: Boolean = false,
        prerelease: Boolean = false,
    ): String = """
        {
          "tag_name": "$tag",
          "html_url": "$url",
          "draft": $draft,
          "prerelease": $prerelease
        }
    """.trimIndent()

    @Test
    fun comparesVersionsNumericallyAndAcceptsReleaseTags() {
        for (tag in listOf("release-v0.3.5", "v0.3.5", "0.3.5")) {
            val expectedUrl = "https://github.com/CyberTianzun/Gateterm/releases/tag/$tag"
            assertEquals(
                UpdateCheckResult.UpdateAvailable("0.3.5", expectedUrl),
                UpdateChecker.parseLatestRelease(releaseJson(tag), "0.3.4"),
            )
        }
        for (tag in listOf("release-v0.3.4", "release-v0.3.3", "release-v0.2.10")) {
            assertEquals(
                UpdateCheckResult.UpToDate,
                UpdateChecker.parseLatestRelease(releaseJson(tag), "0.3.4"),
            )
        }
    }

    @Test
    fun ignoresBuildMetadataAndPrereleaseOffers() {
        assertEquals(
            UpdateCheckResult.UpToDate,
            UpdateChecker.parseLatestRelease(releaseJson("release-v0.3.4+other"), "0.3.4+local"),
        )
        assertEquals(
            UpdateCheckResult.UpToDate,
            UpdateChecker.parseLatestRelease(releaseJson("release-v0.4.0", draft = true), "0.3.4"),
        )
        assertEquals(
            UpdateCheckResult.UpToDate,
            UpdateChecker.parseLatestRelease(releaseJson("release-v0.4.0", prerelease = true), "0.3.4"),
        )
        assertEquals(
            UpdateCheckResult.UpToDate,
            UpdateChecker.parseLatestRelease(releaseJson("release-v0.4.0-rc.1"), "0.3.4"),
        )
        assertEquals(
            UpdateCheckResult.UpdateAvailable(
                "0.3.4",
                "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v0.3.4",
            ),
            UpdateChecker.parseLatestRelease(releaseJson("release-v0.3.4"), "0.3.4-rc.1"),
        )
    }

    @Test
    fun rejectsMalformedOrUntrustedUrls() {
        val badUrls = listOf(
            "javascript:alert(1)",
            "http://github.com/CyberTianzun/Gateterm/releases/tag/v1",
            "https://example.com/CyberTianzun/Gateterm/releases/tag/v1",
            "https://github.com/other/project/releases/tag/v1",
            "https://user@github.com/CyberTianzun/Gateterm/releases/tag/v1",
        )
        for (url in badUrls) {
            assertEquals(
                UpdateCheckResult.Failed,
                UpdateChecker.parseLatestRelease(releaseJson("release-v0.4.0", url = url), "0.3.4"),
            )
        }
        assertEquals(
            UpdateCheckResult.Failed,
            UpdateChecker.parseLatestRelease(releaseJson("nightly"), "0.3.4"),
        )
        assertEquals(
            UpdateCheckResult.Failed,
            UpdateChecker.parseLatestRelease("not-json", "0.3.4"),
        )
    }
}
