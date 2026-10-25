package ai.eclosion.octoterm.android.connection

import java.net.URI

data class ScannedConnection(val url: String, val token: String, val name: String)

/** A QR is a normal desktop access URL. Never open scanned links in a browser or
 * connect automatically: strip the credential from the URL and fill the editor. */
object ConnectionQr {
    fun parse(raw: String): ScannedConnection {
        val text = raw.trim()
        if (text.length > 8192 || !ConnectionUrl.looksLikeUrl(text)) throw ConnectionUrlException("qr")
        return try {
            val uri = URI(text)
            if (uri.rawUserInfo != null || uri.port == 0 || uri.port > 65535) throw ConnectionUrlException("qr")
            val parsed = ConnectionUrl.parse(text)
            val host = parsed.host.removePrefix("[").removeSuffix("]").lowercase()
            if (parsed.isLoopback || parsed.isWildcardBind || host.startsWith("127.") || host.startsWith("::ffff:127.")) {
                throw ConnectionUrlException("qr")
            }
            ScannedConnection(parsed.webSocketUrl, parsed.token.orEmpty(), parsed.displayHost)
        } catch (_: IllegalArgumentException) {
            throw ConnectionUrlException("qr")
        } catch (_: java.net.URISyntaxException) {
            throw ConnectionUrlException("qr")
        }
    }
}
