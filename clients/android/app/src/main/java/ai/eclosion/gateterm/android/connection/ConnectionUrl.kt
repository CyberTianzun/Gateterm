package ai.eclosion.gateterm.android.connection

import java.io.ByteArrayOutputStream
import java.net.URI
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

data class ParsedEndpoint(
    val webSocketUrl: String,
    val token: String?,
    val host: String,
    val displayHost: String,
    val isLoopback: Boolean,
    val isWildcardBind: Boolean,
    val title: String? = null,
) {
    /** `GET /api/launchers` 用的源站。WebSocket 地址换成 http(s)，去掉路径。 */
    fun httpOrigin(): String {
        val uri = try {
            URI(webSocketUrl)
        } catch (_: Exception) {
            return ""
        }
        val scheme = if (uri.scheme == "wss") "https" else "http"
        val host = uri.host ?: return ""
        val hostForUrl = if (host.contains(':') && !host.startsWith("[")) "[$host]" else host
        val port = if (uri.port > 0) ":${uri.port}" else ""
        return "$scheme://$hostForUrl$port"
    }
}

class ConnectionUrlException(message: String) : IllegalArgumentException(message)

object ConnectionUrl {
    fun looksLikeUrl(raw: String): Boolean {
        val t = raw.trim()
        if (t.isEmpty()) return false
        val lower = t.lowercase()
        return lower.startsWith("http://") ||
            lower.startsWith("https://") ||
            lower.startsWith("ws://") ||
            lower.startsWith("wss://")
    }

    fun parse(raw: String, tokenOverride: String? = null): ParsedEndpoint {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) {
            throw ConnectionUrlException("empty")
        }
        val normalized = if (trimmed.contains("://")) trimmed else "http://$trimmed"
        val uri = try {
            URI(normalized)
        } catch (_: Exception) {
            throw ConnectionUrlException("malformed")
        }
        val scheme = uri.scheme?.lowercase()
            ?: throw ConnectionUrlException("malformed")
        val wsScheme = when (scheme) {
            "http", "ws" -> "ws"
            "https", "wss" -> "wss"
            else -> throw ConnectionUrlException("scheme")
        }
        val host = uri.host?.trim().orEmpty()
        if (host.isEmpty()) {
            throw ConnectionUrlException("host")
        }
        val portSuffix = if (uri.port > 0) ":${uri.port}" else ""
        val path = websocketPath(uri.rawPath)
        val token = tokenOverride?.trim()?.takeIf { it.isNotEmpty() }
            ?: paramFrom(uri.rawQuery, uri.rawFragment, "token")
        val title = normalizeTitle(paramFrom(uri.rawQuery, uri.rawFragment, "title"))
        val hostForUrl = if (host.contains(':') && !host.startsWith("[")) "[$host]" else host
        val displayHost = hostForUrl + portSuffix
        return ParsedEndpoint(
            webSocketUrl = "$wsScheme://$hostForUrl$portSuffix$path",
            token = token,
            host = host,
            displayHost = displayHost,
            isLoopback = isLoopback(host),
            isWildcardBind = isWildcard(host),
            title = title,
        )
    }

    fun displayHost(rawUrl: String): String {
        return try {
            parse(rawUrl).displayHost
        } catch (_: ConnectionUrlException) {
            rawUrl.trim()
        }
    }

    private fun websocketPath(rawPath: String?): String {
        val path = rawPath?.takeIf { it.isNotEmpty() } ?: "/"
        return if (path == "/" || path.isEmpty()) {
            "/ws"
        } else if (path == "/ws" || path.endsWith("/ws")) {
            path
        } else {
            path.trimEnd('/') + "/ws"
        }
    }

    private fun paramFrom(rawQuery: String?, rawFragment: String?, targetKey: String): String? {
        return firstParam(rawFragment, targetKey) ?: firstParam(rawQuery, targetKey)
    }

    private fun firstParam(encoded: String?, targetKey: String): String? {
        if (encoded.isNullOrEmpty()) return null
        return encoded.split('&').firstNotNullOfOrNull { part ->
            val idx = part.indexOf('=')
            if (idx <= 0) return@firstNotNullOfOrNull null
            val key = part.substring(0, idx)
            if (key != targetKey) return@firstNotNullOfOrNull null
            decode(part.substring(idx + 1)).takeIf { it.isNotEmpty() }
        }
    }

    private fun normalizeTitle(raw: String?): String? {
        if (raw == null) return null
        val collapsed = raw.replace("\r\n", " ").replace('\r', ' ').replace('\n', ' ').trim()
        if (collapsed.isEmpty()) return null
        val capped = if (collapsed.codePointCount(0, collapsed.length) > 128) {
            collapsed.substring(0, collapsed.offsetByCodePoints(0, 128))
        } else {
            collapsed
        }
        return capped.trimEnd().takeIf { it.isNotEmpty() }
    }

    private fun decode(value: String): String {
        val out = ByteArrayOutputStream(value.length)
        var i = 0
        while (i < value.length) {
            val ch = value[i]
            when {
                ch == '+' -> {
                    out.write(' '.code)
                    i += 1
                }
                ch == '%' -> {
                    if (i + 2 >= value.length) throw ConnectionUrlException("malformed")
                    val hi = hexDigit(value[i + 1])
                    val lo = hexDigit(value[i + 2])
                    if (hi < 0 || lo < 0) throw ConnectionUrlException("malformed")
                    out.write((hi shl 4) or lo)
                    i += 3
                }
                ch.code < 0x80 -> {
                    out.write(ch.code)
                    i += 1
                }
                Character.isHighSurrogate(ch) && i + 1 < value.length && Character.isLowSurrogate(value[i + 1]) -> {
                    out.write(value.substring(i, i + 2).toByteArray(StandardCharsets.UTF_8))
                    i += 2
                }
                Character.isSurrogate(ch) -> throw ConnectionUrlException("malformed")
                else -> {
                    out.write(ch.toString().toByteArray(StandardCharsets.UTF_8))
                    i += 1
                }
            }
        }
        return try {
            StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(out.toByteArray()))
                .toString()
        } catch (_: CharacterCodingException) {
            throw ConnectionUrlException("malformed")
        }
    }

    private fun hexDigit(c: Char): Int = when (c) {
        in '0'..'9' -> c - '0'
        in 'a'..'f' -> c - 'a' + 10
        in 'A'..'F' -> c - 'A' + 10
        else -> -1
    }

    private fun isLoopback(host: String): Boolean {
        val h = host.trim().lowercase().removePrefix("[").removeSuffix("]")
        return h == "localhost" || h == "127.0.0.1" || h == "::1"
    }

    private fun isWildcard(host: String): Boolean {
        val h = host.trim().lowercase().removePrefix("[").removeSuffix("]")
        return h == "0.0.0.0" || h == "::"
    }
}
