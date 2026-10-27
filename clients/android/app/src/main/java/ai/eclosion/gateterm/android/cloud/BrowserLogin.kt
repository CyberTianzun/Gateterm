package ai.eclosion.gateterm.android.cloud

import java.net.URI
import java.net.URLDecoder
import java.net.URLEncoder
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64

const val CLOUD_CALLBACK_URI = "ai.eclosion.gateterm.android://cloud/auth/callback"
const val BROWSER_LOGIN_TIMEOUT_MS = 10 * 60 * 1000L

data class PendingCloudLogin(
    val region: RelayRegion,
    val state: String,
    val verifier: String,
    val createdAt: Long,
    val language: String,
    val label: String,
) {
    fun expired(now: Long): Boolean = now - createdAt !in 0 until BROWSER_LOGIN_TIMEOUT_MS

    fun authorizationUrl(): String {
        val parameters = linkedMapOf(
            "redirect_uri" to CLOUD_CALLBACK_URI, "state" to state,
            "code_challenge" to pkceChallenge(verifier), "code_challenge_method" to "S256",
            "label" to label, "lang" to language,
        )
        return region.url + "auth/mobile?" + parameters.entries.joinToString("&") {
            URLEncoder.encode(it.key, "UTF-8") + "=" + URLEncoder.encode(it.value, "UTF-8")
        }
    }

    companion object {
        fun create(region: RelayRegion, language: String, label: String, now: Long = System.currentTimeMillis()): PendingCloudLogin {
            val random = SecureRandom()
            fun nonce(size: Int): String = ByteArray(size).also(random::nextBytes).let { Base64.getUrlEncoder().withoutPadding().encodeToString(it) }
            return PendingCloudLogin(region, nonce(32), nonce(64), now, if (language == "zh-CN") "zh-CN" else "en", label.trim().take(64).ifEmpty { "Android" })
        }
    }
}

fun pkceChallenge(verifier: String): String = Base64.getUrlEncoder().withoutPadding()
    .encodeToString(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII)))

fun isCloudCallback(raw: String): Boolean = runCatching {
    val uri = URI(raw)
    uri.scheme == "ai.eclosion.gateterm.android" && uri.host == "cloud" && uri.path == "/auth/callback" &&
        uri.port == -1 && uri.rawUserInfo == null && uri.rawFragment == null
}.getOrDefault(false)

/** A mismatched or malformed callback leaves the real pending attempt intact. */
fun callbackCode(raw: String, pending: PendingCloudLogin, now: Long): String? = runCatching {
    if (!isCloudCallback(raw) || pending.expired(now)) return null
    val pairs = URI(raw).rawQuery?.split('&')?.map {
        val pair = it.split('=', limit = 2)
        URLDecoder.decode(pair[0], "UTF-8") to URLDecoder.decode(pair.getOrElse(1) { "" }, "UTF-8")
    } ?: return null
    if (pairs.map { it.first }.toSet().size != pairs.size) return null
    val parameters = pairs.toMap()
    val state = parameters["state"] ?: return null
    if (!MessageDigest.isEqual(state.toByteArray(Charsets.UTF_8), pending.state.toByteArray(Charsets.UTF_8))) return null
    parameters["code"]?.takeIf { it.isNotEmpty() && it.length <= 256 }
}.getOrNull()
