package ai.eclosion.gateterm.android.update

import ai.eclosion.gateterm.android.BuildConfig
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject
import java.net.URI
import java.util.concurrent.TimeUnit

sealed class UpdateCheckResult {
    data class UpdateAvailable(val version: String, val url: String) : UpdateCheckResult()
    data object UpToDate : UpdateCheckResult()
    data object Failed : UpdateCheckResult()
}

object UpdateChecker {
    private const val LATEST_RELEASE_API =
        "https://api.github.com/repos/CyberTianzun/Gateterm/releases/latest"

    private val defaultClient: OkHttpClient by lazy {
        OkHttpClient.Builder()
            .connectTimeout(5, TimeUnit.SECONDS)
            .callTimeout(10, TimeUnit.SECONDS)
            .build()
    }

    fun checkLatest(
        currentVersion: String = BuildConfig.VERSION_NAME,
        http: OkHttpClient = defaultClient,
        apiUrl: String = LATEST_RELEASE_API,
    ): UpdateCheckResult {
        return try {
            val request = Request.Builder()
                .url(apiUrl)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "gateterm/$currentVersion")
                .build()
            http.newCall(request).execute().use { response ->
                if (response.code == 404) return UpdateCheckResult.UpToDate
                if (!response.isSuccessful) return UpdateCheckResult.Failed
                val body = response.body?.string().orEmpty()
                parseLatestRelease(body, currentVersion)
            }
        } catch (_: Exception) {
            UpdateCheckResult.Failed
        }
    }

    fun parseLatestRelease(body: String, currentVersion: String): UpdateCheckResult {
        val current = SemVer.parse(currentVersion) ?: return UpdateCheckResult.Failed
        val json = try {
            JSONObject(body)
        } catch (_: Exception) {
            return UpdateCheckResult.Failed
        }
        if (json.optBoolean("draft", false) || json.optBoolean("prerelease", false)) {
            return UpdateCheckResult.UpToDate
        }
        val rawTag = json.optString("tag_name")
        val htmlUrl = json.optString("html_url")
        if (rawTag.isEmpty() || htmlUrl.isEmpty()) return UpdateCheckResult.Failed

        val normalizedTag = rawTag.removePrefix("release-").removePrefix("v")
        val candidate = SemVer.parse(normalizedTag) ?: return UpdateCheckResult.Failed
        if (candidate.prerelease.isNotEmpty() || !candidate.isNewerStableThan(current)) {
            return UpdateCheckResult.UpToDate
        }
        val validUrl = validateReleaseUrl(htmlUrl) ?: return UpdateCheckResult.Failed
        return UpdateCheckResult.UpdateAvailable(
            version = "${candidate.major}.${candidate.minor}.${candidate.patch}",
            url = validUrl,
        )
    }

    private fun validateReleaseUrl(rawUrl: String): String? {
        return try {
            val uri = URI(rawUrl)
            if (uri.scheme != "https" ||
                uri.host != "github.com" ||
                uri.rawUserInfo != null ||
                uri.port != -1 ||
                uri.path == null ||
                !uri.path.startsWith("/CyberTianzun/Gateterm/releases/")
            ) {
                null
            } else {
                rawUrl
            }
        } catch (_: Exception) {
            null
        }
    }

    private data class SemVer(
        val major: Int,
        val minor: Int,
        val patch: Int,
        val prerelease: String,
    ) {
        fun isNewerStableThan(current: SemVer): Boolean {
            if (major != current.major) return major > current.major
            if (minor != current.minor) return minor > current.minor
            if (patch != current.patch) return patch > current.patch
            return prerelease.isEmpty() && current.prerelease.isNotEmpty()
        }

        companion object {
            private val NUMERIC_PART = Regex("^(0|[1-9]\\d*)$")

            fun parse(raw: String): SemVer? {
                if (raw.isEmpty()) return null
                val plusIndex = raw.indexOf('+')
                if (plusIndex != -1 && plusIndex == raw.lastIndex) return null
                val withoutBuild = if (plusIndex == -1) raw else raw.substring(0, plusIndex)

                val dashIndex = withoutBuild.indexOf('-')
                if (dashIndex != -1 && dashIndex == withoutBuild.lastIndex) return null
                val core = if (dashIndex == -1) withoutBuild else withoutBuild.substring(0, dashIndex)
                val prerelease = if (dashIndex == -1) "" else withoutBuild.substring(dashIndex + 1)

                val parts = core.split('.')
                if (parts.size != 3) return null
                if (!parts.all { NUMERIC_PART.matches(it) }) return null
                val major = parts[0].toIntOrNull() ?: return null
                val minor = parts[1].toIntOrNull() ?: return null
                val patch = parts[2].toIntOrNull() ?: return null
                return SemVer(major, minor, patch, prerelease)
            }
        }
    }
}
