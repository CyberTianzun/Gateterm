import Foundation

/// Binary WebSocket frame: `u32 channel (LE) | u8 flags | payload`. Channel 0 is the JSON control channel.
enum Frame {
    static let controlChannel: UInt32 = 0
    static let headerSize = 5

    struct Decoded: Equatable {
        let channel: UInt32
        let flags: UInt8
        let payload: Data
    }

    static func encode(channel: UInt32, payload: Data, flags: UInt8 = 0) -> Data {
        var data = Data(capacity: headerSize + payload.count)
        withUnsafeBytes(of: channel.littleEndian) { data.append(contentsOf: $0) }
        data.append(flags)
        data.append(payload)
        return data
    }

    static func decode(_ data: Data) -> Decoded? {
        guard data.count >= headerSize else { return nil }
        let bytes = [UInt8](data.prefix(headerSize))
        let channel = UInt32(bytes[0]) | UInt32(bytes[1]) << 8 | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24
        return Decoded(channel: channel, flags: bytes[4], payload: data.dropFirst(headerSize).withUnsafeBytes { Data($0) })
    }
}
