import Foundation

/// Bearer-authenticated JSON calls to the server's HTTP API (`/api/...`), same origin as the WebSocket.
struct ServerHTTP {
    let origin: String
    let token: String

    static let session: URLSession = {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 8
        config.timeoutIntervalForResource = 30
        return URLSession(configuration: config)
    }()

    func url(_ path: String, query: [(String, String)] = []) -> URL? {
        let base = origin.hasSuffix("/") ? String(origin.dropLast()) : origin
        guard var components = URLComponents(string: base + path) else { return nil }
        if !query.isEmpty { components.queryItems = query.map { URLQueryItem(name: $0.0, value: $0.1) } }
        return components.url
    }

    /// Returns the status code and body; throws only on transport failure.
    func request(_ path: String, query: [(String, String)] = [], method: String = "GET", body: Data? = nil) async throws -> (Int, Data) {
        guard let url = url(path, query: query) else { throw URLError(.badURL) }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await Self.session.data(for: request)
        return ((response as? HTTPURLResponse)?.statusCode ?? 0, data)
    }
}
