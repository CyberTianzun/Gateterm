import Foundation

/// Lenient accessors over `JSONSerialization` output, mirroring Android's `optString`/`optLong`.
struct JSON {
    let raw: [String: Any]

    init(_ raw: [String: Any]) { self.raw = raw }

    init?(data: Data) {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        raw = object
    }

    init?(string: String) { self.init(data: Data(string.utf8)) }

    func has(_ key: String) -> Bool { raw[key] != nil && !(raw[key] is NSNull) }
    func string(_ key: String) -> String { raw[key] as? String ?? "" }
    /// `nil` when missing or JSON null; never the literal "null".
    func nullableString(_ key: String) -> String? { raw[key] as? String }
    func int(_ key: String, default value: Int = 0) -> Int { (raw[key] as? NSNumber)?.intValue ?? value }
    func int64(_ key: String, default value: Int64 = 0) -> Int64 { (raw[key] as? NSNumber)?.int64Value ?? value }
    func nullableInt64(_ key: String) -> Int64? { (raw[key] as? NSNumber)?.int64Value }
    func nullableInt(_ key: String) -> Int? { (raw[key] as? NSNumber)?.intValue }
    func double(_ key: String) -> Double? { (raw[key] as? NSNumber)?.doubleValue }
    /// Only a JSON boolean counts; `NSNumber(1)` from a numeric field is not "true".
    func bool(_ key: String, default value: Bool = false) -> Bool {
        guard let number = raw[key] as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else { return value }
        return number.boolValue
    }
    func isTrue(_ key: String) -> Bool { bool(key, default: false) }
    func object(_ key: String) -> JSON? { (raw[key] as? [String: Any]).map(JSON.init) }
    func array(_ key: String) -> [Any]? { raw[key] as? [Any] }
    func objects(_ key: String) -> [JSON] { (array(key) ?? []).compactMap { ($0 as? [String: Any]).map(JSON.init) } }
}

/// JSON `null` for a missing optional (a bare `Optional` inside `[String: Any]` is not serializable).
func orNull(_ value: Any?) -> Any { value ?? NSNull() }

enum ControlJson {
    static let protoVersion = 1

    static func encode(_ msg: ClientMsg) -> Data {
        var obj: [String: Any] = [:]
        func colors(_ c: TermColors?) {
            // 可选字段(协议 X4):没有就不写,旧服务端照常解析。
            if let c { obj["colors"] = ["fg": c.fg, "bg": c.bg] }
        }
        switch msg {
        case let .hello(token, proto):
            obj = ["type": "hello", "token": token, "proto": proto]
        case .listSessions:
            obj = ["type": "list-sessions"]
        case let .newSession(name, command, cwd):
            obj = ["type": "new-session", "name": orNull(name), "command": orNull(command), "cwd": orNull(cwd)]
        case let .killSession(id):
            obj = ["type": "kill-session", "id": id]
        case let .renameSession(id, name):
            obj = ["type": "rename-session", "id": id, "name": name]
        case let .attach(id, channel, lastSeq, cols, rows, c):
            obj = ["type": "attach", "id": id, "channel": channel, "last_seq": orNull(lastSeq), "cols": cols, "rows": rows]
            colors(c)
        case let .detach(channel):
            obj = ["type": "detach", "channel": channel]
        case let .resize(channel, cols, rows, c):
            obj = ["type": "resize", "channel": channel, "cols": cols, "rows": rows]
            colors(c)
        }
        let payload = (try? JSONSerialization.data(withJSONObject: obj)) ?? Data()
        return Frame.encode(channel: Frame.controlChannel, payload: payload)
    }

    static func parse(_ payload: Data) -> ServerMsg? {
        guard let obj = JSON(data: payload) else { return nil }
        let type = obj.string("type")
        func channel(_ key: String = "channel") -> UInt32? { obj.nullableInt64(key).map { UInt32(truncatingIfNeeded: $0) } }
        switch type {
        case "hello-ok":
            let os = obj.nullableString("os") ?? ""
            let cloud = obj.nullableString("cloud_server_id").flatMap { $0.isEmpty ? nil : $0 }
            let win32InputMode = obj.has("win32_input_mode")
                ? obj.bool("win32_input_mode")
                : os.caseInsensitiveCompare("windows") == .orderedSame
            return .helloOk(proto: obj.int("proto", default: protoVersion), os: os,
                            cloudServerId: cloud, win32InputMode: win32InputMode)
        case "error":
            return .error(message: obj.string("message"), channel: channel())
        case "sessions":
            return .sessions(obj.objects("sessions").compactMap(parseSession))
        case "session-event":
            guard let s = obj.object("session").flatMap(parseSession) else { return nil }
            let kind: SessionEventKind = switch obj.string("event") {
            case "renamed": .renamed
            case "closed": .closed
            default: .created
            }
            return .sessionEvent(kind, s)
        case "attached":
            guard let ch = channel() else { return nil }
            return .attached(channel: ch, seq: obj.int64("seq"), mode: obj.string("mode") == "replay" ? .replay : .resync)
        case "resized":
            guard let ch = channel() else { return nil }
            return .resized(channel: ch, cols: obj.int("cols"), rows: obj.int("rows"))
        case "resync-begin":
            guard let ch = channel() else { return nil }
            return .resyncBegin(channel: ch)
        case "resync-end":
            guard let ch = channel() else { return nil }
            return .resyncEnd(channel: ch, seq: obj.int64("seq"))
        case "session-exited":
            guard let ch = channel(), let id = obj.nullableInt64("id") else { return nil }
            return .sessionExited(channel: ch, id: id)
        case "agent-event":
            // 字段不全就当不认识:X2 要求客户端忽略看不懂的消息,而不是断线
            return AgentSessions.parseSession(obj).map(ServerMsg.agentEvent) ?? .unknown(type)
        default:
            return .unknown(type)
        }
    }

    static func parseSession(_ obj: JSON) -> SessionInfo? {
        guard let id = obj.nullableInt64("id") else { return nil }
        let cwd = obj.nullableString("cwd").flatMap { $0.isEmpty ? nil : $0 }
        return SessionInfo(id: id, name: obj.string("name"), cols: obj.int("cols"), rows: obj.int("rows"),
                           createdAt: obj.int64("created_at"), cwd: cwd)
    }
}
