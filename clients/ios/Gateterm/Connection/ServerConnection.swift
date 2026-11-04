import Foundation
import Security

struct ServerConnection: Identifiable, Equatable, Hashable {
    let id: String
    var name: String
    var url: String
    var token: String
    var createdAt: Int64
    var lastUsedAt: Int64? = nil
    /// server 登录 Gateterm Cloud 后在握手里回带的 id;推送靠它找回这条连接
    var cloudServerId: String? = nil

    var title: String {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? ConnectionUrl.displayHost(url) : trimmed
    }

    var subtitle: String { ConnectionUrl.displayHost(url) }
}

enum ServerConnectionStatus: Equatable {
    case disconnected, connecting, connected, reconnecting, failed
    var active: Bool { self == .connecting || self == .connected || self == .reconnecting }
}

/// 推送里的 server_id ↔ 本地连接。
enum CloudLink {
    /// 同一台 server 可能存了多条连接(不同地址):取最近用过的那条。
    static func find(_ items: [ServerConnection], cloudServerId: String) -> ServerConnection? {
        items.filter { $0.cloudServerId == cloudServerId }.max { ($0.lastUsedAt ?? 0) < ($1.lastUsedAt ?? 0) }
    }

    /// 把握手拿到的 id 记到连接上;没有变化返回 nil,调用方据此跳过持久化。
    static func record(_ items: [ServerConnection], connectionId: String, cloudServerId: String?) -> [ServerConnection]? {
        guard let current = items.first(where: { $0.id == connectionId }), current.cloudServerId != cloudServerId else { return nil }
        return items.map { $0.id == connectionId ? { var c = $0; c.cloudServerId = cloudServerId; return c }($0) : $0 }
    }
}

/// Connection metadata lives in UserDefaults; tokens live in the Keychain (this device only).
final class ConnectionStore {
    private let defaults: UserDefaults
    private let keychain: KeychainTokens
    private static let key = "gateterm.connections"

    init(defaults: UserDefaults = .standard, keychain: KeychainTokens = KeychainTokens()) {
        self.defaults = defaults
        self.keychain = keychain
    }

    func load() -> [ServerConnection] {
        guard let data = defaults.data(forKey: Self.key),
              let array = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return [] }
        return array.compactMap { raw in
            let obj = JSON(raw)
            let id = obj.string("id"), url = obj.string("url")
            guard !id.isEmpty, !url.isEmpty else { return nil }
            let lastUsed = obj.int64("lastUsedAt")
            let cloud = obj.nullableString("cloudServerId").flatMap { $0.isEmpty ? nil : $0 }
            return ServerConnection(id: id, name: obj.string("name"), url: url, token: keychain.read(id) ?? "",
                                    createdAt: obj.int64("createdAt"), lastUsedAt: lastUsed > 0 ? lastUsed : nil,
                                    cloudServerId: cloud)
        }
    }

    func save(_ items: [ServerConnection]) {
        let array: [[String: Any]] = items.map {
            ["id": $0.id, "name": $0.name, "url": $0.url, "createdAt": $0.createdAt,
             "lastUsedAt": $0.lastUsedAt ?? 0, "cloudServerId": orNull($0.cloudServerId)]
        }
        if let data = try? JSONSerialization.data(withJSONObject: array) { defaults.set(data, forKey: Self.key) }
        let keep = Set(items.map(\.id))
        for item in items { keychain.write(item.id, token: item.token) }
        keychain.removeAll(except: keep)
    }
}

struct KeychainTokens {
    private let service = "ai.eclosion.gateterm.server-token"

    func read(_ id: String) -> String? {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                    kSecAttrAccount as String: id, kSecReturnData as String: true]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func write(_ id: String, token: String) {
        guard read(id) != token else { return }
        let base: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                   kSecAttrAccount as String: id]
        SecItemDelete(base as CFDictionary)
        var add = base
        add[kSecValueData as String] = Data(token.utf8)
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(add as CFDictionary, nil)
    }

    func removeAll(except keep: Set<String>) {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                    kSecReturnAttributes as String: true, kSecMatchLimit as String: kSecMatchLimitAll]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let rows = result as? [[String: Any]] else { return }
        for row in rows {
            guard let account = row[kSecAttrAccount as String] as? String, !keep.contains(account) else { continue }
            SecItemDelete([kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                           kSecAttrAccount as String: account] as CFDictionary)
        }
    }
}
