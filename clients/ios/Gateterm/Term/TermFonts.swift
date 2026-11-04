import CoreText
import UIKit

/// Bundled terminal fonts and the pixel-grid cell metrics shared by drawing and hit testing.
enum TermFonts {
    private static var postScriptNames: [TermFontKind: String] = [:]

    /// Registers the bundled TTFs for this process (no Info.plist `UIAppFonts` needed).
    static func registerBundledFonts() {
        for kind in TermFontKind.allCases {
            guard let resource = kind.resource,
                  let url = Bundle.main.url(forResource: resource, withExtension: "ttf") else { continue }
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
            if let descriptors = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as? [CTFontDescriptor],
               let name = descriptors.first.flatMap({ CTFontDescriptorCopyAttribute($0, kCTFontNameAttribute) as? String }) {
                postScriptNames[kind] = name
            }
        }
    }

    static func font(_ kind: TermFontKind, size: CGFloat) -> CTFont {
        if let name = postScriptNames[kind] { return CTFontCreateWithName(name as CFString, size, nil) }
        return UIFont.monospacedSystemFont(ofSize: size, weight: .regular) as CTFont
    }
}

/// Cell geometry following the pinned xterm WebGL renderer, like Android's `TerminalCellMetrics`:
/// integer device pixels per cell, so backgrounds and block glyphs never leave seams.
struct TermMetrics: Equatable {
    let font: CTFont
    let scale: CGFloat
    /// Points (device pixels / scale).
    let cellWidth: CGFloat
    let cellHeight: CGFloat
    let baseline: CGFloat
    let charLeft: CGFloat

    init(appearance: TermAppearance, scale: CGFloat) {
        let size = CGFloat(appearance.fontSize)
        let font = TermFonts.font(appearance.font, size: size)
        self.font = font
        self.scale = scale
        var glyph = CGGlyph(0)
        var unichar: UniChar = 87 // "W"
        CTFontGetGlyphsForCharacters(font, &unichar, &glyph, 1)
        var advance = CGSize.zero
        CTFontGetAdvancesForGlyphs(font, .horizontal, &glyph, &advance, 1)
        let logicalAdvance = floor(Double(advance.width) * 65536) / 65536
        let lineHeight = max(1.0, appearance.lineHeight)
        let letterSpacing = appearance.letterSpacing
        let widthPx = max(1, Int(floor(logicalAdvance * Double(scale))) + Int(floor(letterSpacing + 0.5)))
        let ascent = Double(CTFontGetAscent(font)), descent = Double(CTFontGetDescent(font))
        let logicalHeight = floor(ascent + 0.5) + floor(descent + 0.5)
        let charHeightPx = max(1, Int(ceil(logicalHeight * Double(scale))))
        let heightPx = max(1, Int(floor(Double(charHeightPx) * lineHeight)))
        let charLeftPx = Int(floor(letterSpacing / 2))
        let charTopPx = lineHeight == 1 ? 0 : Int(floor(Double(heightPx - charHeightPx) / 2 + 0.5))
        let baselinePx = charTopPx + charHeightPx - Int((descent * Double(scale)).rounded())
        cellWidth = CGFloat(widthPx) / scale
        cellHeight = CGFloat(heightPx) / scale
        baseline = CGFloat(baselinePx) / scale
        charLeft = CGFloat(charLeftPx) / scale
    }

    static func == (a: TermMetrics, b: TermMetrics) -> Bool {
        a.font == b.font && a.scale == b.scale && a.cellWidth == b.cellWidth && a.cellHeight == b.cellHeight && a.baseline == b.baseline
    }

    /// Centered grid origin snapped to device pixels; drawing and hit testing share it.
    func origin(in size: CGSize, cols: Int, rows: Int) -> CGPoint {
        guard cols > 0, rows > 0 else { return .zero }
        let ox = floor(max(0, (size.width - CGFloat(cols) * cellWidth) / 2) * scale) / scale
        let oy = floor(max(0, (size.height - CGFloat(rows) * cellHeight) / 2) * scale) / scale
        return CGPoint(x: ox, y: oy)
    }

    func cell(at point: CGPoint, in size: CGSize, cols: Int, rows: Int, clamp: Bool = false) -> (col: Int, row: Int)? {
        let o = origin(in: size, cols: cols, rows: rows)
        var col = Int(floor((point.x - o.x) / cellWidth)), row = Int(floor((point.y - o.y) / cellHeight))
        if clamp {
            col = min(cols - 1, max(0, col))
            row = min(rows - 1, max(0, row))
        }
        guard (0..<cols).contains(col), (0..<rows).contains(row) else { return nil }
        return (col, row)
    }

    func gridSize(for size: CGSize) -> (cols: Int, rows: Int) {
        (Int(size.width / cellWidth), Int(size.height / cellHeight))
    }
}
