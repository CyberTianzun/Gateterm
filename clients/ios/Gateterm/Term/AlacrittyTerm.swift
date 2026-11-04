import Foundation

struct Cell: Equatable {
    var codepoint: UInt32
    var fg: UInt32
    var bg: UInt32
    var attrs: UInt16
    var width: UInt8
    /// -1 默认色,-2 真彩色(用 fg/bg 里的 RGB),0–255 调色板下标。
    var fgIndex: Int16
    var bgIndex: Int16

    static let blank = Cell(codepoint: 32, fg: TermColor.defaultFG, bg: TermColor.defaultBG, attrs: 0, width: 1,
                            fgIndex: TermColor.indexDefault, bgIndex: TermColor.indexDefault)
}

enum Attr {
    static let bold: UInt16 = 1
    static let dim: UInt16 = 2
    static let italic: UInt16 = 4
    static let underline: UInt16 = 8
    static let inverse: UInt16 = 16
    static let hidden: UInt16 = 32
    static let strike: UInt16 = 64
    static let doubleUnderline: UInt16 = 128
    static let undercurl: UInt16 = 256
    static let dottedUnderline: UInt16 = 512
    static let dashedUnderline: UInt16 = 1024
    static let hyperlink: UInt16 = 2048
}

enum TermColor {
    static let indexDefault: Int16 = -1
    static let indexRGB: Int16 = -2
    static let defaultFG: UInt32 = 0xC0CAF5
    static let defaultBG: UInt32 = 0x1A1B26

    static func indexed(_ index: Int, ansi: [Int]) -> Int {
        let i = max(0, min(255, index))
        if i < 16 { return ansi[i] }
        if i < 232 {
            let v = i - 16
            func level(_ n: Int) -> Int { n == 0 ? 0 : 55 + n * 40 }
            return level(v / 36) << 16 | level((v % 36) / 6) << 8 | level(v % 6)
        }
        let g = 8 + (i - 232) * 10
        return g << 16 | g << 8 | g
    }
}

/// 一次已提交的终端画面。和终端状态机解耦,reset/resize 不能改正在显示的帧。
struct TermFrame: Equatable {
    var cells: [Cell]
    var cols: Int
    var rows: Int
    var cursorX: Int
    var cursorY: Int
    var cursorVisible: Bool
    var scrollOffset: Int
    var images: [FfiViewportImage] = []

    static let empty = TermFrame(cells: [], cols: 0, rows: 0, cursorX: 0, cursorY: 0, cursorVisible: false, scrollOffset: 0, images: [])

    func cell(_ x: Int, _ y: Int) -> Cell {
        let index = y * cols + x
        return index >= 0 && index < cells.count ? cells[index] : .blank
    }

    func hasContent() -> Bool {
        guard cols > 0, rows > 0 else { return false }
        if !images.isEmpty { return true }
        return cells.contains { ($0.codepoint != 32 && $0.codepoint != 0) || $0.bg != TermColor.defaultBG }
    }
}

/// 基于 Rust `alacritty_terminal` 的客户端终端(通过 UniFFI `TermEngine` 驱动)。主线程使用。
final class AlacrittyTerm {
    private(set) var cols: Int
    private(set) var rows: Int
    private(set) var cursorX = 0
    private(set) var cursorY = 0
    private(set) var cursorVisible = true
    private(set) var applicationCursor = false
    private(set) var bracketedPaste = false
    /// XTMODKEYS(`CSI > 4 ; Pv m`)。TerminalKeyboard 据此编码 Shift+Enter 等组合。
    private(set) var modifyOtherKeys = 0
    /// Kitty 键盘协议标志位(`CSI > flags u`)。
    private(set) var kittyKeyboardFlags = 0
    /// Kitty 剪贴板粘贴事件模式(`DECSET 5522`)。
    private(set) var kittyClipboardPasteMode = false
    /// 0 表示贴着最新一行。正数是向上翻了多少行。alt-screen 没有回滚。
    private(set) var scrollOffset = 0
    private(set) var mouseTracking = false
    private(set) var generation: UInt64 = 0
    private let engine: TermEngine

    init(cols: Int = 80, rows: Int = 24, maxScrollback: Int = 20_000) {
        self.cols = max(1, cols)
        self.rows = max(1, rows)
        engine = TermEngine(cols: UInt32(self.cols), rows: UInt32(self.rows), maxScrollback: UInt32(max(0, min(200_000, maxScrollback))))
        apply(engine.state())
    }

    private func apply(_ st: TermState) {
        cols = Int(st.cols)
        rows = Int(st.rows)
        cursorX = Int(st.cursorX)
        cursorY = Int(st.cursorY)
        cursorVisible = st.cursorVisible
        applicationCursor = st.applicationCursor
        bracketedPaste = st.bracketedPaste
        modifyOtherKeys = Int(st.modifyOtherKeys)
        kittyKeyboardFlags = Int(st.kittyKeyboardFlags)
        kittyClipboardPasteMode = st.kittyClipboardPasteMode
        scrollOffset = Int(st.scrollOffset)
        mouseTracking = st.mouseTracking
    }

    func hyperlinkAt(col: Int, row: Int) -> String? {
        guard (0..<cols).contains(col), (0..<rows).contains(row) else { return nil }
        return engine.hyperlinkAt(col: UInt32(col), row: UInt32(row))
    }

    func write(_ bytes: Data) { apply(engine.write(bytes: bytes)); generation &+= 1 }
    func reset() { apply(engine.reset()); generation &+= 1 }

    func resize(cols newCols: Int, rows newRows: Int) {
        let c = max(1, newCols), r = max(1, newRows)
        guard c != cols || r != rows else { return }
        apply(engine.resize(cols: UInt32(c), rows: UInt32(r)))
        generation &+= 1
    }

    func scroll(by delta: Int) {
        guard delta != 0 else { return }
        let previous = scrollOffset
        apply(engine.scrollBy(delta: Int32(clamping: delta)))
        if scrollOffset != previous { generation &+= 1 }
    }

    func scrollToBottom() {
        guard scrollOffset != 0 else { return }
        apply(engine.scrollToBottom())
        generation &+= 1
    }

    func setMaxScrollback(_ limit: Int) {
        apply(engine.setMaxScrollback(limit: UInt32(max(0, min(200_000, limit)))))
        generation &+= 1
    }

    func copyViewport(_ s: TermSelection) -> String {
        engine.copyViewport(x0: Int32(s.x0), y0: Int32(s.y0), x1: Int32(s.x1), y1: Int32(s.y1))
    }

    func encodePointer(col: Int, row: Int, pressed: Bool, moving: Bool) -> Data? {
        guard mouseTracking else { return nil }
        return engine.encodePointer(col: Int32(col), row: Int32(row), button: 0, pressed: pressed, moving: moving)
    }

    func pasteRequiresConfirmation(_ text: String) -> Bool {
        engine.pasteRequiresConfirmation(text: text)
    }

    func encodePaste(_ text: String) -> Data { engine.encodePaste(text: text) }

    func drainClipboardEvents() -> [FfiClipboardEvent] {
        engine.drainClipboardEvents()
    }

    func visibleImages() -> [FfiViewportImage] {
        engine.visibleImages()
    }

    func encodeOsc5522ReadReply(mime: String, data: Data) -> Data {
        engine.encodeOsc5522ReadReply(mime: mime, data: data)
    }

    func encodeOsc5522PasteOffer(availableMimes: [String]) -> Data {
        engine.encodeOsc5522PasteOffer(availableMimes: availableMimes)
    }

    func encodeKey(
        key: String,
        codepoint: Int,
        shift: Bool,
        alt: Bool,
        ctrl: Bool,
        isRelease: Bool,
        serverOs: String,
        win32InputMode: Bool
    ) -> Data {
        engine.encodeKey(
            key: key,
            codepoint: UInt32(max(0, codepoint)),
            shift: shift,
            alt: alt,
            ctrl: ctrl,
            isRelease: isRelease,
            serverOs: serverOs,
            win32InputMode: win32InputMode
        )
    }

    /// Packed little-endian cells: `u32 cp, u32 fg, u32 bg, i16 fgIndex, i16 bgIndex, u16 attrs, u8 width, u8 pad`.
    func capture() -> TermFrame {
        let packed = engine.snapshotPacked()
        let count = cols * rows
        var cells = [Cell](repeating: .blank, count: count)
        packed.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let available = min(count, raw.count / 20)
            for i in 0..<available {
                let o = i * 20
                cells[i] = Cell(
                    codepoint: UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: o, as: UInt32.self)),
                    fg: UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: o + 4, as: UInt32.self)),
                    bg: UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: o + 8, as: UInt32.self)),
                    attrs: UInt16(littleEndian: raw.loadUnaligned(fromByteOffset: o + 16, as: UInt16.self)),
                    width: raw[o + 18],
                    fgIndex: Int16(littleEndian: raw.loadUnaligned(fromByteOffset: o + 12, as: Int16.self)),
                    bgIndex: Int16(littleEndian: raw.loadUnaligned(fromByteOffset: o + 14, as: Int16.self)))
            }
        }
        let cy = cursorY + scrollOffset
        let visible = cursorVisible && (0..<rows).contains(cy) && (0..<cols).contains(cursorX)
        return TermFrame(cells: cells, cols: cols, rows: rows, cursorX: cursorX, cursorY: visible ? cy : cursorY,
                         cursorVisible: visible, scrollOffset: scrollOffset, images: visibleImages())
    }
}

struct TermSelection: Equatable {
    var x0: Int, y0: Int, x1: Int, y1: Int

    var normalized: TermSelection {
        y0 < y1 || (y0 == y1 && x0 <= x1) ? self : TermSelection(x0: x1, y0: y1, x1: x0, y1: y0)
    }

    func contains(_ x: Int, _ y: Int) -> Bool {
        let s = normalized
        if y < s.y0 || y > s.y1 { return false }
        if s.y0 == s.y1 { return x >= s.x0 && x <= s.x1 }
        if y == s.y0 { return x >= s.x0 }
        if y == s.y1 { return x <= s.x1 }
        return true
    }
}

/// 首帧/resync 期间的刷屏门闩。服务端顺序是 resized → resync-begin → 重绘字节 → resync-end。
/// 持有期间只改模拟器、不提交画面;客户端尺寸要等第一次 resync-end 之后再报。
struct TermPaintGate {
    private(set) var holding = false
    private(set) var acceptClientResize = false

    mutating func onSessionOpen() { holding = true; acceptClientResize = false }
    mutating func onAttached(replay: Bool) { holding = !replay; acceptClientResize = replay }
    mutating func onResyncBegin() { holding = true }
    mutating func onResyncEnd() { holding = false; acceptClientResize = true }
    mutating func onDetached() { holding = false; acceptClientResize = false }

    var shouldPublishWrite: Bool { !holding }
    var shouldSendResize: Bool { acceptClientResize && !holding }
}

/// Layout reports one-column jitter while the keyboard animates; only a real change resizes the PTY.
func significantResize(_ cols: Int, _ rows: Int, _ prevCols: Int, _ prevRows: Int) -> Bool {
    if prevCols <= 0 || prevRows <= 0 { return true }
    return abs(cols - prevCols) >= 2 || abs(rows - prevRows) >= 2
}
