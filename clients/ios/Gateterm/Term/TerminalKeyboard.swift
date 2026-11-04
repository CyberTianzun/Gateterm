import Foundation

/// Logical keys retain their identity until the server-specific encoding step.
/// `vk`/`scan` are Windows virtual-key and Set-1 scan codes (win32-input-mode).
enum TerminalKey: CaseIterable {
    case escape, backspace, tab, enter, space
    case up, down, left, right, home, end, pageUp, pageDown, insert, delete
    case shiftLeft, shiftRight, controlLeft, controlRight, altLeft, altRight, capsLock, numLock, scrollLock, pause, printScreen, menu
    case a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x, y, z
    case digit0, digit1, digit2, digit3, digit4, digit5, digit6, digit7, digit8, digit9
    case minus, equal, bracketLeft, bracketRight, semicolon, quote, backquote, backslash, comma, period, slash
    case f1, f2, f3, f4, f5, f6, f7, f8, f9, f10, f11, f12
    case numpad0, numpad1, numpad2, numpad3, numpad4, numpad5, numpad6, numpad7, numpad8, numpad9
    case numpadEnter, numpadMultiply, numpadAdd, numpadSubtract, numpadDecimal, numpadDivide

    private static let table: [TerminalKey: (vk: Int, scan: Int, enhanced: Bool, character: Int)] = {
        var t: [TerminalKey: (Int, Int, Bool, Int)] = [
            .escape: (27, 1, false, 27), .backspace: (8, 14, false, 8), .tab: (9, 15, false, 9),
            .enter: (13, 28, false, 13), .space: (32, 57, false, 32),
            .up: (38, 72, true, 0), .down: (40, 80, true, 0), .left: (37, 75, true, 0), .right: (39, 77, true, 0),
            .home: (36, 71, true, 0), .end: (35, 79, true, 0), .pageUp: (33, 73, true, 0), .pageDown: (34, 81, true, 0),
            .insert: (45, 82, true, 0), .delete: (46, 83, true, 0),
            .shiftLeft: (16, 42, false, 0), .shiftRight: (16, 54, false, 0), .controlLeft: (17, 29, false, 0),
            .controlRight: (17, 29, true, 0), .altLeft: (18, 56, false, 0), .altRight: (18, 56, true, 0),
            .capsLock: (20, 58, false, 0), .numLock: (144, 69, true, 0), .scrollLock: (145, 70, false, 0),
            .pause: (19, 69, false, 0), .printScreen: (44, 55, true, 0), .menu: (93, 93, true, 0),
            .minus: (189, 12, false, 45), .equal: (187, 13, false, 61), .bracketLeft: (219, 26, false, 91),
            .bracketRight: (221, 27, false, 93), .semicolon: (186, 39, false, 59), .quote: (222, 40, false, 39),
            .backquote: (192, 41, false, 96), .backslash: (220, 43, false, 92), .comma: (188, 51, false, 44),
            .period: (190, 52, false, 46), .slash: (191, 53, false, 47),
            .numpadEnter: (13, 28, true, 13), .numpadMultiply: (106, 55, false, 42), .numpadAdd: (107, 78, false, 43),
            .numpadSubtract: (109, 74, false, 45), .numpadDecimal: (110, 83, false, 46), .numpadDivide: (111, 53, true, 47),
        ]
        let letterScans = [30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44]
        for (index, key) in letters.enumerated() { t[key] = (65 + index, letterScans[index], false, 97 + index) }
        let digitScans = [11, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        for (index, key) in digits.enumerated() { t[key] = (48 + index, digitScans[index], false, 48 + index) }
        let fScans = [59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88]
        for (index, key) in functionKeys.enumerated() { t[key] = (112 + index, fScans[index], false, 0) }
        let padScans = [82, 79, 80, 81, 75, 76, 77, 71, 72, 73]
        for (index, key) in numpadDigits.enumerated() { t[key] = (96 + index, padScans[index], false, 48 + index) }
        return t.mapValues { (vk: $0.0, scan: $0.1, enhanced: $0.2, character: $0.3) }
    }()

    static let letters: [TerminalKey] = [.a, .b, .c, .d, .e, .f, .g, .h, .i, .j, .k, .l, .m, .n, .o, .p, .q, .r, .s, .t, .u, .v, .w, .x, .y, .z]
    static let digits: [TerminalKey] = [.digit0, .digit1, .digit2, .digit3, .digit4, .digit5, .digit6, .digit7, .digit8, .digit9]
    static let functionKeys: [TerminalKey] = [.f1, .f2, .f3, .f4, .f5, .f6, .f7, .f8, .f9, .f10, .f11, .f12]
    static let numpadDigits: [TerminalKey] = [.numpad0, .numpad1, .numpad2, .numpad3, .numpad4, .numpad5, .numpad6, .numpad7, .numpad8, .numpad9]
    static let modifierKeys: Set<TerminalKey> = [.shiftLeft, .shiftRight, .controlLeft, .controlRight, .altLeft, .altRight, .capsLock, .numLock, .scrollLock]

    var vk: Int { Self.table[self]!.vk }
    var scan: Int { Self.table[self]!.scan }
    var enhanced: Bool { Self.table[self]!.enhanced }
    var character: Int { Self.table[self]!.character }
    var isLetter: Bool { Self.letters.contains(self) }

    static func fromASCII(_ char: Character) -> TerminalKey? {
        guard let ascii = char.asciiValue else { return nil }
        switch char {
        case "a"..."z": return letters[Int(ascii - 97)]
        case "A"..."Z": return letters[Int(ascii - 65)]
        case "0"..."9": return digits[Int(ascii - 48)]
        case "\r", "\n", "\r\n": return .enter
        case "\t": return .tab
        case "\u{1b}": return .escape
        case " ": return .space
        default: break
        }
        // Declaration order matches Android's enum, so ties ('*', '+') resolve to the same key.
        if let key = allCases.first(where: { $0.character == Int(ascii) && $0.character != 0 }) {
            return key
        }
        let shifted: [Character: TerminalKey] = [
            "!": .digit1, "@": .digit2, "#": .digit3, "$": .digit4, "%": .digit5, "^": .digit6, "&": .digit7, "*": .digit8,
            "(": .digit9, ")": .digit0, "_": .minus, "+": .equal, "{": .bracketLeft, "}": .bracketRight, ":": .semicolon,
            "\"": .quote, "~": .backquote, "|": .backslash, "<": .comma, ">": .period, "?": .slash,
        ]
        return shifted[char]
    }
}

struct KeyModifiers: Equatable {
    var shift = false
    var ctrl = false
    var alt = false
    var leftCtrl = false
    var rightCtrl = false
    var leftAlt = false
    var rightAlt = false
    var capsLock = false
    var numLock = false
    var scrollLock = false

    var active: Bool { shift || ctrl || alt }

    static func + (a: KeyModifiers, b: KeyModifiers) -> KeyModifiers {
        KeyModifiers(shift: a.shift || b.shift, ctrl: a.ctrl || b.ctrl, alt: a.alt || b.alt,
                     leftCtrl: a.leftCtrl || b.leftCtrl, rightCtrl: a.rightCtrl || b.rightCtrl,
                     leftAlt: a.leftAlt || b.leftAlt, rightAlt: a.rightAlt || b.rightAlt,
                     capsLock: a.capsLock || b.capsLock, numLock: a.numLock || b.numLock, scrollLock: a.scrollLock || b.scrollLock)
    }

    func windowsState(enhanced: Bool) -> Int {
        var state = enhanced ? 0x100 : 0
        if shift { state |= 0x10 }
        if ctrl {
            if rightCtrl { state |= 0x4 }
            if leftCtrl || !rightCtrl { state |= 0x8 }
        }
        if alt {
            if rightAlt { state |= 0x1 }
            if leftAlt || !rightAlt { state |= 0x2 }
        }
        if capsLock { state |= 0x80 }
        if numLock { state |= 0x20 }
        if scrollLock { state |= 0x40 }
        return state
    }
}

struct KeyStroke: Equatable {
    var key: TerminalKey
    var codePoint: Int
    var modifiers: KeyModifiers

    init(_ key: TerminalKey, codePoint: Int? = nil, modifiers: KeyModifiers = KeyModifiers()) {
        self.key = key
        self.codePoint = codePoint ?? key.character
        self.modifiers = modifiers
    }
}

/// One instance per session. Physical key-up uses the identity saved at key-down.
/// Byte-for-byte the same encodings as Android `TerminalKeyboard` and web `keys.ts`.
final class TerminalKeyboard {
    private struct Pressed {
        let stroke: KeyStroke
        let win32: Bool
        let character: Int
        let kittyFlags: Int
    }
    let serverOS: String
    let win32InputMode: Bool
    private var pressed: [Int: Pressed] = [:]
    private var order: [Int] = []

    init(serverOS: String, win32InputMode: Bool = false) {
        self.serverOS = serverOS
        self.win32InputMode = win32InputMode
    }

    func hasPressed(_ id: Int) -> Bool { pressed[id] != nil }

    private func usesWin32(_ stroke: KeyStroke) -> Bool { serverOS == "windows" && win32InputMode && (0...127).contains(stroke.codePoint) }

    func keyDown(
        id: Int,
        _ stroke: KeyStroke,
        applicationCursor: Bool = false,
        modifyOtherKeys: Int = 0,
        kittyKeyboardFlags: Int = 0
    ) -> Data {
        let win32 = usesWin32(stroke)
        let isRepeat = pressed[id] != nil
        if !isRepeat { order.append(id) }
        pressed[id] = Pressed(stroke: stroke, win32: win32, character: character(stroke), kittyFlags: kittyKeyboardFlags)
        if win32 {
            return packet(stroke, down: true)
        }
        return legacy(
            stroke,
            applicationCursor: applicationCursor,
            modifyOtherKeys: modifyOtherKeys,
            kittyKeyboardFlags: kittyKeyboardFlags,
            eventType: isRepeat ? 2 : 1
        )
    }

    func keyUp(id: Int, modifiers: KeyModifiers, kittyKeyboardFlags: Int = 0) -> Data? {
        guard let previous = pressed.removeValue(forKey: id) else { return nil }
        order.removeAll { $0 == id }
        if previous.win32 {
            var stroke = previous.stroke
            stroke.modifiers = modifiers
            return packet(stroke, down: false, savedCharacter: previous.character)
        }
        let flags = kittyKeyboardFlags != 0 ? kittyKeyboardFlags : previous.kittyFlags
        if (flags & 2) != 0 {
            var stroke = previous.stroke
            stroke.modifiers = modifiers
            return encodeKitty(stroke, flags: flags, eventType: 3) ?? Data()
        }
        return Data()
    }

    func tap(
        _ stroke: KeyStroke,
        applicationCursor: Bool = false,
        modifyOtherKeys: Int = 0,
        kittyKeyboardFlags: Int = 0
    ) -> Data {
        if usesWin32(stroke) { return packet(stroke, down: true) + packet(stroke, down: false) }
        return legacy(
            stroke,
            applicationCursor: applicationCursor,
            modifyOtherKeys: modifyOtherKeys,
            kittyKeyboardFlags: kittyKeyboardFlags,
            eventType: 1
        )
    }

    /// IME commits are text, never bracketed paste. Explicit modifiers apply to one ASCII key.
    func committedText(
        _ text: String,
        modifiers: KeyModifiers = KeyModifiers(),
        modifyOtherKeys: Int = 0,
        kittyKeyboardFlags: Int = 0
    ) -> Data {
        if text.count == 1, let char = text.first, let ascii = char.asciiValue, ascii < 128,
           let key = TerminalKey.fromASCII(char),
           modifiers.active || text == "\n" || text == "\r" || text == "\t" || (kittyKeyboardFlags & 8) != 0 {
            let cp = modifiers.shift && char.isLowercase ? Int(char.uppercased().first!.asciiValue!) : Int(ascii)
            return tap(
                KeyStroke(key, codePoint: cp, modifiers: modifiers),
                modifyOtherKeys: modifyOtherKeys,
                kittyKeyboardFlags: kittyKeyboardFlags
            )
        }
        return Data(text.utf8)
    }

    func releaseAll() -> Data {
        var out = Data()
        for id in order {
            guard let p = pressed[id], p.win32 else { continue }
            var stroke = p.stroke
            stroke.modifiers = KeyModifiers()
            out += packet(stroke, down: false, savedCharacter: p.character)
        }
        reset()
        return out
    }

    func reset() {
        pressed.removeAll()
        order.removeAll()
    }

    private func packet(_ stroke: KeyStroke, down: Bool, savedCharacter: Int? = nil) -> Data {
        let key = stroke.key
        let cp = stroke.codePoint
        var vk = key.vk
        if key.isLetter, let scalar = Unicode.Scalar(cp), scalar.isASCII, Character(scalar).isLetter {
            vk = Int(Character(scalar).uppercased().first!.asciiValue!)
        }
        if let index = TerminalKey.numpadDigits.firstIndex(of: key), cp == 0 {
            vk = [45, 35, 40, 34, 37, 12, 39, 36, 38, 33][index]
        }
        if key == .numpadDecimal && cp == 0 { vk = 46 }
        let char = savedCharacter ?? character(stroke)
        let state = stroke.modifiers.windowsState(enhanced: key.enhanced)
        return Data("\u{1b}[\(vk);\(key.scan);\(char);\(down ? 1 : 0);\(state);1_".utf8)
    }

    private func character(_ stroke: KeyStroke) -> Int {
        let ctrl = stroke.modifiers.ctrl
        switch stroke.key {
        case .enter, .numpadEnter: return ctrl ? 10 : 13
        case .tab: return 9
        case .backspace: return ctrl ? 127 : 8
        case .escape: return 27
        default: return Self.controlCharacter(stroke.codePoint, ctrl: ctrl)
        }
    }

    /// 程序打开 modifyOtherKeys 后,传统编码丢修饰键的键改发 CSI 27;mod;code ~(与 web keys.ts 一致)。
    /// 第二项是保留传统编码的修饰掩码(shift=1 alt=2 ctrl=4)。
    private static let otherKeys: [TerminalKey: (Int, Set<Int>)] = [
        .enter: (13, [0, 2]), .numpadEnter: (13, [0, 2]), .tab: (9, [0, 1, 2]),
        .backspace: (127, [0, 2, 4]), .escape: (27, [0, 2]),
    ]

    private func legacy(
        _ stroke: KeyStroke,
        applicationCursor: Bool,
        modifyOtherKeys: Int,
        kittyKeyboardFlags: Int = 0,
        eventType: Int = 1
    ) -> Data {
        if kittyKeyboardFlags != 0, let kitty = encodeKitty(stroke, flags: kittyKeyboardFlags, eventType: eventType) {
            return kitty
        }
        let key = stroke.key
        let m = stroke.modifiers
        let modifier = 1 + (m.shift ? 1 : 0) + (m.alt ? 2 : 0) + (m.ctrl ? 4 : 0)
        if kittyKeyboardFlags == 0, modifyOtherKeys != 0, let other = Self.otherKeys[key], !other.1.contains(modifier - 1) {
            return Data("\u{1b}[27;\(modifier);\(other.0)~".utf8)
        }
        let letters: [TerminalKey: String] = [.up: "A", .down: "B", .right: "C", .left: "D", .home: "H", .end: "F",
                                              .f1: "P", .f2: "Q", .f3: "R", .f4: "S"]
        if let letter = letters[key] {
            if modifier != 1 { return Data("\u{1b}[1;\(modifier)\(letter)".utf8) }
            if [.f1, .f2, .f3, .f4].contains(key) || applicationCursor { return Data("\u{1b}O\(letter)".utf8) }
            return Data("\u{1b}[\(letter)".utf8)
        }
        let numbers: [TerminalKey: Int] = [.insert: 2, .delete: 3, .pageUp: 5, .pageDown: 6, .f5: 15, .f6: 17, .f7: 18,
                                           .f8: 19, .f9: 20, .f10: 21, .f11: 23, .f12: 24]
        if let number = numbers[key] {
            return Data("\u{1b}[\(number)\(modifier == 1 ? "" : ";\(modifier)")~".utf8)
        }
        if key == .tab && m.shift { return Data("\u{1b}[Z".utf8) }
        if key.character == 0 && stroke.codePoint == 0 { return Data() }
        let cp = key == .backspace ? (m.ctrl ? 8 : 127) : character(stroke)
        guard let scalar = Unicode.Scalar(cp) else { return Data() }
        return Data(((m.alt ? "\u{1b}" : "") + String(Character(scalar))).utf8)
    }

    private func encodeKitty(_ stroke: KeyStroke, flags: Int, eventType: Int) -> Data? {
        let key = stroke.key
        let m = stroke.modifiers
        let modifier = 1 + (m.shift ? 1 : 0) + (m.alt ? 2 : 0) + (m.ctrl ? 4 : 0)
        let reportEvents = (flags & 2) != 0
        let reportAllAsEsc = (flags & 8) != 0
        let etSuffix = (reportEvents && eventType > 1) ? ":\(eventType)" : ""

        let letters: [TerminalKey: String] = [
            .up: "A", .down: "B", .right: "C", .left: "D",
            .home: "H", .end: "F",
            .f1: "P", .f2: "Q", .f3: "R", .f4: "S",
        ]
        if let letter = letters[key] {
            if modifier != 1 || !etSuffix.isEmpty {
                return Data("\u{1b}[1;\(modifier)\(etSuffix)\(letter)".utf8)
            }
            return nil
        }

        let numbers: [TerminalKey: Int] = [
            .insert: 2, .delete: 3, .pageUp: 5, .pageDown: 6,
            .f5: 15, .f6: 17, .f7: 18, .f8: 19,
            .f9: 20, .f10: 21, .f11: 23, .f12: 24,
        ]
        if let number = numbers[key] {
            if modifier != 1 || !etSuffix.isEmpty {
                return Data("\u{1b}[\(number);\(modifier)\(etSuffix)~".utf8)
            }
            return Data("\u{1b}[\(number)~".utf8)
        }

        let isEnterTabBs = key == .enter || key == .numpadEnter || key == .tab || key == .backspace
        let shouldCsiU = reportAllAsEsc || key == .escape || m.ctrl || m.alt || (isEnterTabBs && m.shift) || eventType == 3
        guard shouldCsiU else { return nil }

        let cp: Int = switch key {
        case .escape: 27
        case .enter, .numpadEnter: 13
        case .tab: 9
        case .backspace: 127
        case .space: 32
        default:
            {
                let raw = stroke.codePoint != 0 ? stroke.codePoint : key.character
                return (65...90).contains(raw) ? raw + 32 : raw
            }()
        }
        guard cp > 0 else { return nil }

        let modPart: String
        if !etSuffix.isEmpty {
            modPart = ";\(modifier)\(etSuffix)"
        } else if modifier > 1 {
            modPart = ";\(modifier)"
        } else {
            modPart = ""
        }
        return Data("\u{1b}[\(cp)\(modPart)u".utf8)
    }

    private static func controlCharacter(_ cp: Int, ctrl: Bool) -> Int {
        guard ctrl else { return cp }
        switch cp {
        case 32, 64: return 0
        case 47, 95: return 31
        case 63: return 127
        case 50...56: return [0, 27, 28, 29, 30, 31, 127][cp - 50]
        case 64...95, 97...122: return cp & 31
        default: return cp
        }
    }
}
