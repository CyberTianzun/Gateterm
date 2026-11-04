import Foundation

struct ParsedEndpoint: Equatable {
    let webSocketURL: String
    let token: String?
    let host: String
    let displayHost: String
    let isLoopback: Bool
    let isWildcardBind: Bool

    /// `GET /api/...` 用的源站。WebSocket 地址换成 http(s),去掉路径。
    func httpOrigin() -> String {
        guard let components = URLComponents(string: webSocketURL), let host = components.host else { return "" }
        let scheme = components.scheme == "wss" ? "https" : "http"
        let hostForURL = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
        let port = components.port.map { ":\($0)" } ?? ""
        return "\(scheme)://\(hostForURL)\(port)"
    }
}

struct ConnectionUrlError: Error, Equatable {
    let reason: String
}

/// Same rules as Android `ConnectionUrl` and the web client: accept http(s)/ws(s) or a bare host,
/// append `/ws`, and read the token from `#token=` (preferred) or `?token=`.
enum ConnectionUrl {
    static func looksLikeURL(_ raw: String) -> Bool {
        let lower = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return ["http://", "https://", "ws://", "wss://"].contains { lower.hasPrefix($0) }
    }

    static func parse(_ raw: String, tokenOverride: String? = nil) throws -> ParsedEndpoint {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw ConnectionUrlError(reason: "empty") }
        let normalized = trimmed.contains("://") ? trimmed : "http://\(trimmed)"
        guard let components = URLComponents(string: normalized), let scheme = components.scheme?.lowercased() else {
            throw ConnectionUrlError(reason: "malformed")
        }
        let wsScheme: String
        switch scheme {
        case "http", "ws": wsScheme = "ws"
        case "https", "wss": wsScheme = "wss"
        default: throw ConnectionUrlError(reason: "scheme")
        }
        let host = (components.host ?? "").trimmingCharacters(in: .whitespaces)
        guard !host.isEmpty else { throw ConnectionUrlError(reason: "host") }
        if let port = components.port, port <= 0 || port > 65535 { throw ConnectionUrlError(reason: "malformed") }
        let portSuffix = components.port.map { ":\($0)" } ?? ""
        let path = websocketPath(components.percentEncodedPath)
        let override = tokenOverride?.trimmingCharacters(in: .whitespacesAndNewlines)
        // Read the raw query/fragment from the input: URLComponents re-escapes a malformed `%ZZ`.
        let hash = normalized.firstIndex(of: "#")
        let rawFragment = hash.map { String(normalized[normalized.index(after: $0)...]) }
        let beforeHash = normalized[..<(hash ?? normalized.endIndex)]
        let rawQuery = beforeHash.firstIndex(of: "?").map { String(beforeHash[beforeHash.index(after: $0)...]) }
        let token = try (override?.isEmpty == false ? override : nil) ?? firstToken(rawFragment) ?? firstToken(rawQuery)
        let bare = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
        let hostForURL = bare.contains(":") ? "[\(bare)]" : bare
        return ParsedEndpoint(
            webSocketURL: "\(wsScheme)://\(hostForURL)\(portSuffix)\(path)",
            token: token,
            host: bare,
            displayHost: hostForURL + portSuffix,
            isLoopback: ["localhost", "127.0.0.1", "::1"].contains(bare.lowercased()),
            isWildcardBind: ["0.0.0.0", "::"].contains(bare.lowercased())
        )
    }

    static func displayHost(_ raw: String) -> String {
        (try? parse(raw).displayHost) ?? raw.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func websocketPath(_ raw: String) -> String {
        let path = raw.isEmpty ? "/" : raw
        if path == "/" { return "/ws" }
        if path.hasSuffix("/ws") { return path }
        var trimmed = path
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        return trimmed + "/ws"
    }

    /// Like `URLDecoder`: `+` is a space and a malformed escape is an error, not a silent miss.
    private static func firstToken(_ encoded: String?) throws -> String? {
        guard let encoded, !encoded.isEmpty else { return nil }
        for part in encoded.split(separator: "&") {
            guard let eq = part.firstIndex(of: "="), eq != part.startIndex, part[..<eq] == "token" else { continue }
            let value = String(part[part.index(after: eq)...]).replacingOccurrences(of: "+", with: " ")
            guard let decoded = value.removingPercentEncoding else { throw ConnectionUrlError(reason: "malformed") }
            if !decoded.isEmpty { return decoded }
        }
        return nil
    }
}

struct ScannedConnection: Equatable {
    let url: String
    let token: String
    let name: String
}

/// A QR is a normal desktop access URL. Never open scanned links in a browser or
/// connect automatically: strip the credential from the URL and fill the editor.
enum ConnectionQr {
    static func parse(_ raw: String) throws -> ScannedConnection {
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.count <= 8192, ConnectionUrl.looksLikeURL(text),
              let components = URLComponents(string: text),
              components.user == nil, components.password == nil, components.port != 0 else {
            throw ConnectionUrlError(reason: "qr")
        }
        let parsed = try ConnectionUrl.parse(text)
        let host = parsed.host.lowercased()
        if parsed.isLoopback || parsed.isWildcardBind || host.hasPrefix("127.") || host.hasPrefix("::ffff:127.") {
            throw ConnectionUrlError(reason: "qr")
        }
        return ScannedConnection(url: parsed.webSocketURL, token: parsed.token ?? "", name: parsed.displayHost)
    }
}
