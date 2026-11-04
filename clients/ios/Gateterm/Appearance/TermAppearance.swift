import Foundation

enum TermFontKind: String, CaseIterable, Identifiable {
    case jetBrainsMono, googleSansCode, zedMono, mono
    var id: String { rawValue }

    var resource: String? {
        switch self {
        case .jetBrainsMono: "JetBrainsMono-Regular"
        case .googleSansCode: "GoogleSansCode-Regular"
        case .zedMono: "ZedMono-Regular"
        case .mono: nil
        }
    }

    var displayName: String {
        switch self {
        case .jetBrainsMono: "JetBrains Mono"
        case .googleSansCode: "Google Sans Code"
        case .zedMono: "Zed Mono"
        case .mono: L.t("appearance_font_mono")
        }
    }
}

/// 纯客户端外观。和 web 的 GateConfig、Android 同一份 JSON:颜色、字号、行高、字距、回滚行数。
/// 导入的文本不可信,坏字段退回 base,整段不是 JSON 时返回 nil。
struct TermAppearance: Equatable {
    var name: String
    var foreground: Int
    var background: Int
    var cursor: Int
    /// ARGB (`#RRGGBBAA` from CSS is stored as alpha in the top byte).
    var selection: UInt32
    var ansi: [Int]
    /// Logical points, like Web CSS pixels; independent of Dynamic Type.
    var fontSize: Double
    var font: TermFontKind
    var scrollback: Int
    var lineHeight: Double = 1.0
    var letterSpacing: Double = 0.0

    func color(index: Int16, stored: UInt32, foreground isForeground: Bool) -> Int {
        switch Int(index) {
        case Int(TermColor.indexDefault): return isForeground ? foreground : background
        case Int(TermColor.indexRGB): return Int(stored & 0xFFFFFF)
        case 0...15: return ansi.indices.contains(Int(index)) ? ansi[Int(index)] : Int(stored & 0xFFFFFF)
        case 16...255: return TermColor.indexed(Int(index), ansi: ansi)
        default: return Int(stored & 0xFFFFFF)
        }
    }

    func withFontSize(_ size: Double) -> TermAppearance {
        guard size.isFinite else { return self }
        var next = self
        next.fontSize = min(48, max(6, (size + 0.5).rounded(.down)))
        return next
    }

    func withLineHeight(_ value: Double) -> TermAppearance {
        guard value.isFinite else { return self }
        var next = self; next.lineHeight = min(3, max(1, value)); return next
    }

    func withLetterSpacing(_ value: Double) -> TermAppearance {
        guard value.isFinite else { return self }
        var next = self; next.letterSpacing = min(10, max(-5, value)); return next
    }

    func withScrollback(_ lines: Int) -> TermAppearance { var next = self; next.scrollback = min(200_000, max(0, lines)); return next }
    func withFont(_ kind: TermFontKind) -> TermAppearance { var next = self; next.font = kind; return next }

    func withTheme(_ theme: TermAppearance) -> TermAppearance {
        var next = theme
        next.fontSize = fontSize
        next.font = font
        next.scrollback = scrollback
        next.lineHeight = lineHeight
        next.letterSpacing = letterSpacing
        return next
    }
}

enum AppearanceCodec {
    static let version = 1
    private static let systemMonoStack = "ui-monospace, SFMono-Regular, \"SF Mono\", Menlo, Consolas, \"DejaVu Sans Mono\", monospace"
    private static let ansiKeys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
                                   "brightBlack", "brightRed", "brightGreen", "brightYellow",
                                   "brightBlue", "brightMagenta", "brightCyan", "brightWhite"]

    static func `default`(prefersDark: Bool = true) -> TermAppearance { prefersDark ? dark2026 : light2026 }

    static func builtin(_ name: String) -> TermAppearance? {
        switch name {
        case "Tokyo Night": tokyoNight
        case "2026 Dark": dark2026
        case "2026 Light": light2026
        default: nil
        }
    }

    static let builtins: [TermAppearance] = [light2026, dark2026, tokyoNight]

    static func exportJSON(_ a: TermAppearance) -> String {
        var colors: [String: Any] = [
            "foreground": hex(a.foreground), "background": hex(a.background), "cursor": hex(a.cursor),
            "selectionBackground": hexCSS(a.selection),
        ]
        for (i, key) in ansiKeys.enumerated() { colors[key] = hex(a.ansi.indices.contains(i) ? a.ansi[i] : 0) }
        let root: [String: Any] = [
            "version": version,
            "theme": ["name": a.name, "colors": colors],
            "font": ["family": fontFamily(a.font), "size": a.fontSize, "lineHeight": a.lineHeight, "letterSpacing": a.letterSpacing],
            "terminal": ["scrollback": a.scrollback],
        ]
        let data = (try? JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    /// 不是 JSON 对象时返回 nil。字段缺失或非法则沿用 base。
    static func importJSON(_ text: String, base: TermAppearance = AppearanceCodec.default()) -> TermAppearance? {
        guard let root = JSON(string: text) else { return nil }
        let theme = root.object("theme")
        let rawName = String((theme?.string("name") ?? "").trimmingCharacters(in: .whitespacesAndNewlines).prefix(80))
        let name = rawName.isEmpty ? base.name : rawName
        var next: TermAppearance
        if let colors = theme?.object("colors") {
            next = fromColors(name, colors, base)
        } else if let named = builtin(name) {
            next = base.withTheme(named)
        } else {
            next = base
            next.name = name
        }
        if let font = root.object("font") {
            if let size = font.double("size") { next = next.withFontSize(size) }
            if font.has("family") { next = next.withFont(fontKind(font.string("family"))) }
            if let lh = font.double("lineHeight") { next = next.withLineHeight(lh) }
            if let ls = font.double("letterSpacing") { next = next.withLetterSpacing(ls) }
        }
        if let terminal = root.object("terminal"), let sb = terminal.nullableInt("scrollback") { next = next.withScrollback(sb) }
        return next
    }

    private static func fromColors(_ name: String, _ colors: JSON, _ base: TermAppearance) -> TermAppearance {
        var next = base
        next.name = name
        next.foreground = parseRGB(colors.string("foreground")) ?? base.foreground
        next.background = parseRGB(colors.string("background")) ?? base.background
        next.cursor = parseRGB(colors.string("cursor")) ?? base.cursor
        next.selection = parseCSS(colors.string("selectionBackground")) ?? base.selection
        next.ansi = ansiKeys.enumerated().map { i, key in
            let fallback = base.ansi.indices.contains(i) ? base.ansi[i] : 0
            return colors.has(key) ? parseRGB(colors.string(key)) ?? fallback : fallback
        }
        return next
    }

    static let tokyoNight = TermAppearance(
        name: "Tokyo Night", foreground: 0xC0CAF5, background: 0x1A1B26, cursor: 0x7AA2F7, selection: 0x6633_4467,
        ansi: [0x151520, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xC0CAF5,
               0x565F89, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xC0CAF5],
        fontSize: 14, font: .jetBrainsMono, scrollback: 20_000)

    static let dark2026 = TermAppearance(
        name: "2026 Dark", foreground: 0xCCCCCC, background: 0x191A1B, cursor: 0xBFBFBF, selection: 0x3339_94BC,
        ansi: [0x000000, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5,
               0x666666, 0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xE5E5E5],
        fontSize: 14, font: .jetBrainsMono, scrollback: 20_000)

    static let light2026 = TermAppearance(
        name: "2026 Light", foreground: 0x3B3B3B, background: 0xFAFAFD, cursor: 0x202020, selection: 0x2600_69CC,
        ansi: [0x000000, 0xCD3131, 0x107C10, 0x949800, 0x0451A5, 0xBC05BC, 0x0598BC, 0x555555,
               0x666666, 0xCD3131, 0x14CE14, 0xB5BA00, 0x0451A5, 0xBC05BC, 0x0598BC, 0xA5A5A5],
        fontSize: 14, font: .jetBrainsMono, scrollback: 20_000)

    private static func fontFamily(_ kind: TermFontKind) -> String {
        switch kind {
        case .jetBrainsMono: "\"JetBrains Mono\", \(systemMonoStack)"
        case .googleSansCode: "\"Google Sans Code\", \(systemMonoStack)"
        case .zedMono: "\"Zed Mono\", \(systemMonoStack)"
        case .mono: systemMonoStack
        }
    }

    private static func fontKind(_ family: String) -> TermFontKind {
        let lower = family.lowercased()
        if lower.contains("jetbrains") { return .jetBrainsMono }
        if lower.contains("google sans code") || lower.contains("googlesans") { return .googleSansCode }
        if lower.contains("zed") { return .zedMono }
        return .mono
    }

    private static func hex(_ rgb: Int) -> String { String(format: "#%06X", rgb & 0xFFFFFF) }

    private static func hexCSS(_ argb: UInt32) -> String {
        let alpha = (argb >> 24) & 0xFF
        let rgb = Int(argb & 0xFFFFFF)
        return alpha == 0xFF || alpha == 0 ? hex(rgb) : String(format: "#%06X%02X", rgb, alpha)
    }

    static func parseRGB(_ raw: String?) -> Int? { parseCSS(raw).map { Int($0 & 0xFFFFFF) } }

    /// CSS 顺序:#RGB、#RGBA、#RRGGBB、#RRGGBBAA。返回 AARRGGBB。
    static func parseCSS(_ raw: String?) -> UInt32? {
        guard var body = raw?.trimmingCharacters(in: .whitespacesAndNewlines), !body.isEmpty else { return nil }
        if body.hasPrefix("#") { body.removeFirst() }
        guard body.allSatisfy(\.isHexDigit) else { return nil }
        let expanded: String
        switch body.count {
        case 3, 4: expanded = body.map { "\($0)\($0)" }.joined()
        case 6, 8: expanded = body
        default: return nil
        }
        guard let rgb = UInt32(expanded.prefix(6), radix: 16) else { return nil }
        if expanded.count == 6 { return 0xFF00_0000 | rgb }
        guard let alpha = UInt32(expanded.suffix(2), radix: 16) else { return nil }
        return alpha << 24 | rgb
    }
}

/// Same foreground/background blends and accent contrast rule as `clients/web/src/appearance.ts`.
struct UiPalette: Equatable {
    let light: Bool
    let background: Int
    let foreground: Int
    let panel: Int
    let panel2: Int
    let line: Int
    let dim: Int
    let accent: Int
    let danger: Int
}

extension TermAppearance {
    var isLight: Bool {
        (0.2126 * Double(channel(background, 16)) + 0.7152 * Double(channel(background, 8)) + 0.0722 * Double(channel(background, 0))) / 255 > 0.5
    }

    var palette: UiPalette {
        let light = isLight
        return UiPalette(
            light: light, background: background, foreground: foreground,
            panel: mixRGB(foreground, background, light ? 0.05 : 0.07),
            panel2: mixRGB(foreground, background, light ? 0.11 : 0.14),
            line: mixRGB(foreground, background, light ? 0.2 : 0.22),
            dim: mixRGB(foreground, background, 0.55),
            accent: readableAccent(ansi[4], ansi[12], background),
            danger: readableAccent(ansi[1], ansi[9], background))
    }
}

private func channel(_ rgb: Int, _ shift: Int) -> Int { (rgb >> shift) & 0xFF }

func mixRGB(_ foreground: Int, _ background: Int, _ amount: Double) -> Int {
    [16, 8, 0].reduce(0) { rgb, shift in
        let bg = Double(channel(background, shift))
        return rgb | (Int((bg + (Double(channel(foreground, shift)) - bg) * amount).rounded()) << shift)
    }
}

func contrast(_ a: Int, _ b: Int) -> Double {
    func luminance(_ rgb: Int) -> Double {
        func linear(_ shift: Int) -> Double {
            let v = Double(channel(rgb, shift)) / 255
            return v <= 0.03928 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0)
    }
    let la = luminance(a), lb = luminance(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
}

private func readableAccent(_ normal: Int, _ bright: Int, _ background: Int) -> Int {
    if contrast(normal, background) >= 3 { return normal }
    return contrast(bright, background) > contrast(normal, background) ? bright : normal
}
