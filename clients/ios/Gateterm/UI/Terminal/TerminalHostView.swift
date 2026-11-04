import CoreText
import UIKit

/// Draws a committed `TermFrame` with Core Graphics and turns touches into scrolling,
/// selection and mouse reports. Keyboard input goes through `TerminalInputProxy`, a hidden
/// text view, so IME composition (e.g. Chinese pinyin) works like any other text field.
final class TerminalHostView: UIView, UIGestureRecognizerDelegate, UIEditMenuInteractionDelegate {
    // MARK: Inputs from SwiftUI

    static let topBarRevealZone: CGFloat = 48

    weak var term: AlacrittyTerm?
    var frameState: TermFrame = .empty { didSet { if frameState != oldValue { setNeedsDisplay(); placeProxy() } } }
    var appearance: TermAppearance = AppearanceCodec.default() {
        didSet {
            guard appearance != oldValue else { return }
            backgroundColor = UIColor(rgb: appearance.background)
            proxy.keyboardAppearance = appearance.isLight ? .light : .dark
            remeasure()
        }
    }
    var selection: TermSelection? { didSet { if selection != oldValue { setNeedsDisplay() } } }
    var mouseTracking = false
    var inputEnabled = true
    var applicationCursor = false
    var modifyOtherKeys = 0
    var kittyKeyboardFlags = 0
    var topBarVisible = true
    var keyboard = TerminalKeyboard(serverOS: "") {
        didSet { if keyboard !== oldValue { emit(oldValue.releaseAll()) } }
    }
    var softModifiers = KeyModifiers()

    var onInput: (Data) -> Void = { _ in }
    var onProposeSize: (Int, Int) -> Void = { _, _ in }
    var onScroll: (Int) -> Void = { _ in }
    var onSelection: (TermSelection?) -> Void = { _ in }
    var onMouse: (Int, Int, Bool, Bool) -> Void = { _, _, _, _ in }
    var onCopy: () -> Void = {}
    var onPaste: () -> Void = {}
    var onModifiersConsumed: () -> Void = {}
    var onHideTopBar: () -> Void = {}
    var onRevealTopBar: () -> Void = {}
    var onFontSize: (Double) -> Void = { _ in }

    // MARK: State

    private(set) var metrics: TermMetrics
    private let proxy = TerminalInputProxy()
    private var glyphCache: [UInt32: (CTFont, CGGlyph)] = [:]
    private var lastProposed: (Int, Int) = (0, 0)
    private var scrollAccum: CGFloat = 0
    private var mouseDownCell: (Int, Int)?
    private var selectionAnchor: (Int, Int)?
    private var pinchBase: Double = 14
    private var inertia: CADisplayLink?
    private var inertiaVelocity: CGFloat = 0
    private lazy var editMenu = UIEditMenuInteraction(delegate: self)

    override init(frame: CGRect) {
        metrics = TermMetrics(appearance: AppearanceCodec.default(), scale: UIScreen.main.scale)
        super.init(frame: frame)
        isOpaque = true
        contentMode = .redraw
        backgroundColor = UIColor(rgb: appearance.background)
        isAccessibilityElement = true
        accessibilityTraits = .allowsDirectInteraction
        proxy.host = self
        addSubview(proxy)
        addInteraction(editMenu)

        let tap = UITapGestureRecognizer(target: self, action: #selector(onTap(_:)))
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan(_:)))
        pan.maximumNumberOfTouches = 1
        let twoFinger = UIPanGestureRecognizer(target: self, action: #selector(onTwoFingerPan(_:)))
        twoFinger.minimumNumberOfTouches = 2
        twoFinger.allowedScrollTypesMask = .all
        let longPress = UILongPressGestureRecognizer(target: self, action: #selector(onLongPress(_:)))
        longPress.minimumPressDuration = 0.38
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch(_:)))
        for recognizer in [tap, pan, twoFinger, longPress, pinch] as [UIGestureRecognizer] {
            recognizer.delegate = self
            addGestureRecognizer(recognizer)
        }
        tap.require(toFail: longPress)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    // MARK: Layout and sizing

    private func remeasure() {
        let next = TermMetrics(appearance: appearance, scale: window?.screen.scale ?? UIScreen.main.scale)
        if next != metrics {
            metrics = next
            glyphCache.removeAll()
            lastProposed = (0, 0)
            proposeSize()
        }
        setNeedsDisplay()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil { remeasure() }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        proposeSize()
        placeProxy()
    }

    private func proposeSize() {
        let (cols, rows) = metrics.gridSize(for: bounds.size)
        guard cols >= 20, rows >= 5, (cols, rows) != lastProposed else { return }
        lastProposed = (cols, rows)
        onProposeSize(cols, rows)
    }

    /// Keep the hidden text view at the cursor so the iPad IME candidate window appears there.
    private func placeProxy() {
        let o = metrics.origin(in: bounds.size, cols: frameState.cols, rows: frameState.rows)
        proxy.frame = CGRect(x: o.x + CGFloat(frameState.cursorX) * metrics.cellWidth,
                             y: o.y + CGFloat(frameState.cursorY) * metrics.cellHeight,
                             width: metrics.cellWidth, height: metrics.cellHeight)
    }

    // MARK: Focus

    var isEditing: Bool { proxy.isFirstResponder }

    func focus() {
        guard !proxy.isFirstResponder else { return }
        _ = proxy.becomeFirstResponder()
        setNeedsDisplay()
    }

    func blur() {
        releaseKeys()
        _ = proxy.resignFirstResponder()
        setNeedsDisplay()
    }

    func releaseKeys() {
        if inputEnabled { emit(keyboard.releaseAll()) } else { keyboard.reset() }
        consumeModifiers()
    }

    fileprivate func focusChanged() { setNeedsDisplay() }

    // MARK: Input from the proxy

    fileprivate func emit(_ bytes: Data) { if !bytes.isEmpty { onInput(bytes) } }

    fileprivate func consumeModifiers() {
        guard softModifiers.active else { return }
        softModifiers = KeyModifiers()
        onModifiersConsumed()
    }

    /// Text committed by the keyboard or IME. Soft Ctrl/Alt/Shift apply to one ASCII key.
    fileprivate func commitText(_ text: String, composing: Bool) {
        guard inputEnabled, !text.isEmpty else { return }
        let ascii = text.count == 1 && (text.first?.asciiValue).map { (32...126).contains($0) } == true
        let modifiers = composing && !ascii ? KeyModifiers() : softModifiers
        emit(keyboard.committedText(text, modifiers: modifiers, modifyOtherKeys: modifyOtherKeys, kittyKeyboardFlags: kittyKeyboardFlags))
        if (!composing || ascii), text.count == 1, let c = text.first, c.isASCII, TerminalKey.fromASCII(c) != nil {
            consumeModifiers()
        }
    }

    fileprivate func softBackspace() {
        guard inputEnabled else { return }
        emit(keyboard.tap(KeyStroke(.backspace, modifiers: softModifiers), modifyOtherKeys: modifyOtherKeys, kittyKeyboardFlags: kittyKeyboardFlags))
        consumeModifiers()
    }

    /// Hardware key down. Returns false to let the text system handle it (plain typing, IME, ⌘ shortcuts).
    fileprivate func hardwareKeyDown(_ key: UIKey) -> Bool {
        guard let mapped = HIDKeyMap.key(key.keyCode) else { return false }
        let flags = key.modifierFlags
        if flags.contains(.command) { return false }
        let win32 = keyboard.serverOS == "windows" && keyboard.win32InputMode
        if TerminalKey.modifierKeys.contains(mapped) && !win32 { return false }
        let special = mapped.character == 0 || [.enter, .numpadEnter, .tab, .escape, .backspace].contains(mapped)
        let shortcut = flags.contains(.control) || flags.contains(.alternate)
        let kittyIntercept = (kittyKeyboardFlags & 10) != 0
        guard special || shortcut || kittyIntercept || TerminalKey.modifierKeys.contains(mapped) else { return false }
        guard inputEnabled else { return true }
        var modifiers = KeyModifiers(shift: flags.contains(.shift), ctrl: flags.contains(.control), alt: flags.contains(.alternate),
                                     capsLock: flags.contains(.alphaShift))
        modifiers = modifiers + softModifiers
        var cp = mapped.character
        if let scalar = key.charactersIgnoringModifiers.unicodeScalars.first, scalar.isASCII, scalar.value >= 32, mapped.character != 0,
           !TerminalKey.numpadDigits.contains(mapped) {
            cp = Int(scalar.value)
            if modifiers.shift, (97...122).contains(cp) { cp -= 32 }
        }
        emit(keyboard.keyDown(id: key.keyCode.rawValue, KeyStroke(mapped, codePoint: cp, modifiers: modifiers),
                              applicationCursor: applicationCursor, modifyOtherKeys: modifyOtherKeys,
                              kittyKeyboardFlags: kittyKeyboardFlags))
        if !TerminalKey.modifierKeys.contains(mapped) { consumeModifiers() }
        return true
    }

    fileprivate func hardwareKeyUp(_ key: UIKey) -> Bool {
        let flags = key.modifierFlags
        let modifiers = KeyModifiers(shift: flags.contains(.shift), ctrl: flags.contains(.control), alt: flags.contains(.alternate))
        guard let bytes = keyboard.keyUp(id: key.keyCode.rawValue, modifiers: modifiers, kittyKeyboardFlags: kittyKeyboardFlags) else { return false }
        if inputEnabled { emit(bytes) }
        return true
    }

    // MARK: Gestures

    private func cell(_ point: CGPoint, clamp: Bool = false) -> (Int, Int)? {
        metrics.cell(at: point, in: bounds.size, cols: frameState.cols, rows: frameState.rows, clamp: clamp).map { ($0.col, $0.row) }
    }

    func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        (g is UIPinchGestureRecognizer && other is UIPanGestureRecognizer) || (other is UIPinchGestureRecognizer && g is UIPanGestureRecognizer)
    }

    @objc private func onTap(_ g: UITapGestureRecognizer) {
        stopInertia()
        let point = g.location(in: self)
        if !topBarVisible && point.y <= Self.topBarRevealZone {
            onRevealTopBar()
            return
        }
        if topBarVisible {
            onHideTopBar()
        }
        if selection != nil { onSelection(nil) }
        if mouseTracking, let c = cell(point) {
            onMouse(c.0, c.1, true, false)
            onMouse(c.0, c.1, false, false)
        }
        focus()
    }

    @objc private func onPan(_ g: UIPanGestureRecognizer) {
        let point = g.location(in: self)
        switch g.state {
        case .began:
            stopInertia()
            scrollAccum = 0
            let start = point.applying(.init(translationX: -g.translation(in: self).x, y: -g.translation(in: self).y))
            let inTopRevealZone = !topBarVisible && start.y <= Self.topBarRevealZone
            if topBarVisible {
                onHideTopBar()
            }
            if mouseTracking, !inTopRevealZone, let c = cell(start) {
                mouseDownCell = c
                onMouse(c.0, c.1, true, false)
            }
        case .changed:
            if mouseDownCell != nil {
                if let c = cell(point, clamp: true) { onMouse(c.0, c.1, true, true) }
            } else {
                scrollLines(g.translation(in: self).y)
                g.setTranslation(.zero, in: self)
            }
        default:
            if let down = mouseDownCell {
                let c = cell(point, clamp: true) ?? down
                onMouse(c.0, c.1, false, false)
                mouseDownCell = nil
            } else if g.state == .ended {
                startInertia(g.velocity(in: self).y)
            }
        }
    }

    @objc private func onTwoFingerPan(_ g: UIPanGestureRecognizer) {
        switch g.state {
        case .began: stopInertia(); scrollAccum = 0
        case .changed:
            scrollLines(g.translation(in: self).y)
            g.setTranslation(.zero, in: self)
        case .ended: startInertia(g.velocity(in: self).y)
        default: break
        }
    }

    /// Finger moving down (positive dy) reveals older lines, like Android/web.
    private func scrollLines(_ dy: CGFloat) {
        guard metrics.cellHeight > 0 else { return }
        scrollAccum += dy
        let lines = Int(scrollAccum / metrics.cellHeight)
        if lines != 0 {
            scrollAccum -= CGFloat(lines) * metrics.cellHeight
            onScroll(lines)
        }
    }

    private func startInertia(_ velocity: CGFloat) {
        guard abs(velocity) > 200 else { return }
        inertiaVelocity = velocity
        let link = CADisplayLink(target: self, selector: #selector(stepInertia(_:)))
        link.add(to: .main, forMode: .common)
        inertia = link
    }

    @objc private func stepInertia(_ link: CADisplayLink) {
        let dt = CGFloat(link.targetTimestamp - link.timestamp)
        scrollLines(inertiaVelocity * dt)
        inertiaVelocity *= pow(0.94, dt * 60)
        if abs(inertiaVelocity) < 40 { stopInertia() }
    }

    private func stopInertia() {
        inertia?.invalidate()
        inertia = nil
    }

    @objc private func onLongPress(_ g: UILongPressGestureRecognizer) {
        let point = g.location(in: self)
        switch g.state {
        case .began:
            stopInertia()
            guard let c = cell(point, clamp: true) else { return }
            selectionAnchor = c
            onSelection(TermSelection(x0: c.0, y0: c.1, x1: c.0, y1: c.1))
            UIImpactFeedbackGenerator(style: .light).impactOccurred()
        case .changed:
            guard let a = selectionAnchor, let c = cell(point, clamp: true) else { return }
            onSelection(TermSelection(x0: a.0, y0: a.1, x1: c.0, y1: c.1))
        case .ended:
            selectionAnchor = nil
            editMenu.presentEditMenu(with: UIEditMenuConfiguration(identifier: nil, sourcePoint: point))
        default:
            selectionAnchor = nil
        }
    }

    @objc private func onPinch(_ g: UIPinchGestureRecognizer) {
        switch g.state {
        case .began: pinchBase = appearance.fontSize
        case .changed:
            let next = min(48, max(6, (pinchBase * Double(g.scale)).rounded()))
            if next != appearance.fontSize { onFontSize(next) }
        default: break
        }
    }

    func editMenuInteraction(_ interaction: UIEditMenuInteraction, menuFor configuration: UIEditMenuConfiguration,
                             suggestedActions: [UIMenuElement]) -> UIMenu? {
        var actions: [UIAction] = []
        if selection != nil {
            actions.append(UIAction(title: L.t("ios_selection_copy"), image: UIImage(systemName: "doc.on.doc")) { [weak self] _ in
                self?.onCopy()
                self?.onSelection(nil)
            })
        }
        if UIPasteboard.general.hasStrings && inputEnabled {
            actions.append(UIAction(title: L.t("ios_selection_paste"), image: UIImage(systemName: "doc.on.clipboard")) { [weak self] _ in
                self?.onPaste()
            })
        }
        return actions.isEmpty ? nil : UIMenu(children: actions)
    }

    func showEditMenu(at point: CGPoint) {
        editMenu.presentEditMenu(with: UIEditMenuConfiguration(identifier: nil, sourcePoint: point))
    }

    // MARK: Drawing

    override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext() else { return }
        let a = appearance
        let defaultBG = a.background
        ctx.setFillColor(UIColor(rgb: defaultBG).cgColor)
        ctx.fill(bounds)
        let f = frameState
        guard f.cols > 0, f.rows > 0 else { return }
        let m = metrics
        let o = m.origin(in: bounds.size, cols: f.cols, rows: f.rows)
        let sel = selection
        let selectionColor = UIColor(argb: a.selection).cgColor

        func colors(_ c: Cell) -> (fg: Int, bg: Int) {
            var fg = a.color(index: c.fgIndex, stored: c.fg, foreground: true)
            var bg = a.color(index: c.bgIndex, stored: c.bg, foreground: false)
            if c.attrs & Attr.inverse != 0 { swap(&fg, &bg) }
            return (fg, bg)
        }

        // 1. Backgrounds, run-length merged; selection paints over them.
        for y in 0..<f.rows {
            let top = o.y + CGFloat(y) * m.cellHeight
            var x = 0
            while x < f.cols {
                let c = f.cell(x, y)
                let width = max(1, Int(c.width))
                if c.width == 0 { x += 1; continue }
                let bg = colors(c).bg
                guard bg != defaultBG else { x += width; continue }
                let start = x
                x += width
                while x < f.cols {
                    let n = f.cell(x, y)
                    if n.width != 0 && colors(n).bg != bg { break }
                    x += max(1, Int(n.width))
                }
                ctx.setFillColor(UIColor(rgb: bg).cgColor)
                ctx.fill(CGRect(x: o.x + CGFloat(start) * m.cellWidth, y: top, width: CGFloat(x - start) * m.cellWidth, height: m.cellHeight))
            }
            if let sel {
                let s = sel.normalized
                if y >= s.y0 && y <= s.y1 {
                    let x0 = y == s.y0 ? s.x0 : 0, x1 = y == s.y1 ? s.x1 : f.cols - 1
                    ctx.setFillColor(selectionColor)
                    ctx.fill(CGRect(x: o.x + CGFloat(x0) * m.cellWidth, y: top, width: CGFloat(x1 - x0 + 1) * m.cellWidth, height: m.cellHeight))
                }
            }
        }

        // 1b. Kitty graphics placements with zIndex < 0 (below text, above cell backgrounds).
        if !f.images.isEmpty {
            Self.drawViewportImages(in: ctx, images: f.images, zFilter: { $0 < 0 }, origin: o, cellW: m.cellWidth, cellH: m.cellHeight)
        }

        // 2. Glyphs in a y-up text space, batched per font/colour/style.
        ctx.saveGState()
        ctx.translateBy(x: 0, y: bounds.height)
        ctx.scaleBy(x: 1, y: -1)
        let height = bounds.height
        var glyphs: [CGGlyph] = []
        var positions: [CGPoint] = []
        var batchFont: CTFont?
        var batchKey: (Int, UInt16) = (-1, 0)
        let boldWidth = max(0.5, CGFloat(a.fontSize) * 0.035)
        func flush() {
            guard let font = batchFont, !glyphs.isEmpty else { glyphs.removeAll(keepingCapacity: true); positions.removeAll(keepingCapacity: true); return }
            let (fg, attrs) = batchKey
            let alpha: CGFloat = attrs & Attr.dim != 0 ? 0.6 : 1
            let color = UIColor(rgb: fg).withAlphaComponent(alpha).cgColor
            ctx.setFillColor(color)
            ctx.setStrokeColor(color)
            if attrs & Attr.bold != 0 { ctx.setTextDrawingMode(.fillStroke); ctx.setLineWidth(boldWidth) } else { ctx.setTextDrawingMode(.fill) }
            ctx.textMatrix = attrs & Attr.italic != 0 ? CGAffineTransform(a: 1, b: 0, c: 0.2, d: 1, tx: 0, ty: 0) : .identity
            CTFontDrawGlyphs(font, glyphs, positions, glyphs.count, ctx)
            glyphs.removeAll(keepingCapacity: true)
            positions.removeAll(keepingCapacity: true)
        }
        var decorations: [(CGRect, Int)] = []
        var blocks: [(CGRect, Int)] = []
        var fancyUnderlines: [(attrs: UInt16, left: CGFloat, top: CGFloat, width: CGFloat, fg: Int, alpha: CGFloat)] = []
        for y in 0..<f.rows {
            let top = o.y + CGFloat(y) * m.cellHeight
            let baselineY = height - (top + m.baseline)
            for x in 0..<f.cols {
                let c = f.cell(x, y)
                guard c.width != 0 else { continue }
                var (fg, bg) = colors(c)
                if c.attrs & Attr.hidden != 0 { fg = bg }
                let left = o.x + CGFloat(x) * m.cellWidth
                let cellW = m.cellWidth * CGFloat(max(1, Int(c.width)))
                let alpha: CGFloat = (c.attrs & Attr.dim) != 0 ? 0.6 : 1
                if Self.hasFancyUnderline(c.attrs) {
                    fancyUnderlines.append((c.attrs, left, top, cellW, fg, alpha))
                }
                guard c.codepoint != 32, c.codepoint != 0 else { continue }
                let plainUnderline = (c.attrs & Attr.underline) != 0 &&
                    (c.attrs & (Attr.doubleUnderline | Attr.undercurl | Attr.dottedUnderline | Attr.dashedUnderline)) == 0
                if plainUnderline {
                    decorations.append((CGRect(x: left, y: top + m.baseline + 1 / m.scale, width: cellW, height: max(1 / m.scale, 1)), fg))
                }
                if c.attrs & Attr.strike != 0 {
                    decorations.append((CGRect(x: left, y: top + m.baseline - m.cellHeight * 0.3, width: cellW, height: max(1 / m.scale, 1)), fg))
                }
                if appendBlock(c.codepoint, left: left, top: top, width: cellW, height: m.cellHeight, color: fg, into: &blocks) { continue }
                guard let (font, glyph) = glyph(for: c.codepoint) else { continue }
                let style = c.attrs & (Attr.bold | Attr.italic | Attr.dim)
                if batchFont !== font || batchKey.0 != fg || batchKey.1 != style {
                    flush()
                    batchFont = font
                    batchKey = (fg, style)
                }
                glyphs.append(glyph)
                positions.append(CGPoint(x: left + m.charLeft, y: baselineY))
            }
        }
        flush()
        ctx.restoreGState()
        ctx.setTextDrawingMode(.fill)
        for (rect, color) in blocks + decorations {
            ctx.setFillColor(UIColor(rgb: color).cgColor)
            ctx.fill(rect)
        }
        for item in fancyUnderlines {
            Self.drawFancyUnderline(
                in: ctx,
                attrs: item.attrs,
                left: item.left,
                top: item.top,
                width: item.width,
                cellH: m.cellHeight,
                color: UIColor(rgb: item.fg).withAlphaComponent(item.alpha).cgColor
            )
        }

        // 2b. Kitty graphics placements with zIndex >= 0 (above text, below cursor).
        if !f.images.isEmpty {
            Self.drawViewportImages(in: ctx, images: f.images, zFilter: { $0 >= 0 }, origin: o, cellW: m.cellWidth, cellH: m.cellHeight)
        }

        // 3. Cursor: solid while the keyboard is focused, hollow otherwise.
        if f.cursorVisible && sel == nil {
            let r = CGRect(x: o.x + CGFloat(f.cursorX) * m.cellWidth, y: o.y + CGFloat(f.cursorY) * m.cellHeight,
                           width: m.cellWidth, height: m.cellHeight)
            let color = UIColor(rgb: a.cursor)
            if proxy.isFirstResponder {
                ctx.setFillColor(color.withAlphaComponent(0.63).cgColor)
                ctx.fill(r)
            } else {
                ctx.setStrokeColor(color.cgColor)
                ctx.setLineWidth(1)
                ctx.stroke(r.insetBy(dx: 0.5, dy: 0.5))
            }
        }
    }

    private func appendBlock(_ cp: UInt32, left: CGFloat, top: CGFloat, width: CGFloat, height: CGFloat, color: Int,
                             into out: inout [(CGRect, Int)]) -> Bool {
        let s = metrics.scale
        func snap(_ v: CGFloat) -> CGFloat { (v * s).rounded() / s }
        if let rects = TerminalBlockGlyphs.rectangles(cp) {
            for i in stride(from: 0, to: rects.count, by: 4) {
                let x0 = snap(left + CGFloat(rects[i]) * width / 8), y0 = snap(top + CGFloat(rects[i + 1]) * height / 8)
                let x1 = snap(left + CGFloat(rects[i + 2]) * width / 8), y1 = snap(top + CGFloat(rects[i + 3]) * height / 8)
                out.append((CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0), color))
            }
            return true
        }
        if let box = TerminalBlockGlyphs.boxSegments(cp) {
            for i in stride(from: 0, to: box.count, by: 4) {
                let x0 = snap(left + CGFloat(box[i]) * width / 16), y0 = snap(top + CGFloat(box[i + 1]) * height / 16)
                let x1 = max(x0 + 1 / s, snap(left + CGFloat(box[i + 2]) * width / 16))
                let y1 = max(y0 + 1 / s, snap(top + CGFloat(box[i + 3]) * height / 16))
                out.append((CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0), color))
            }
            return true
        }
        return false
    }

    /// Primary-font glyph, or a Core Text fallback font for CJK, emoji and symbols.
    private func glyph(for cp: UInt32) -> (CTFont, CGGlyph)? {
        if let cached = glyphCache[cp] { return cached.1 == 0 ? nil : cached }
        guard let scalar = Unicode.Scalar(cp) else { return nil }
        var units = Array(String(Character(scalar)).utf16)
        var glyphs = [CGGlyph](repeating: 0, count: units.count)
        var result: (CTFont, CGGlyph) = (metrics.font, 0)
        if CTFontGetGlyphsForCharacters(metrics.font, &units, &glyphs, units.count), glyphs[0] != 0 {
            result = (metrics.font, glyphs[0])
        } else {
            let string = String(Character(scalar)) as CFString
            let fallback = CTFontCreateForString(metrics.font, string, CFRange(location: 0, length: CFStringGetLength(string)))
            if CTFontGetGlyphsForCharacters(fallback, &units, &glyphs, units.count), glyphs[0] != 0 { result = (fallback, glyphs[0]) }
        }
        glyphCache[cp] = result
        return result.1 == 0 ? nil : result
    }

    // MARK: Kitty graphics, fancy underlines, clipboard & hyperlinks

    private static let imageCache: NSCache<NSString, CGImage> = {
        let cache = NSCache<NSString, CGImage>()
        cache.countLimit = 64
        return cache
    }()

    static func hasFancyUnderline(_ attrs: UInt16) -> Bool {
        (attrs & (Attr.doubleUnderline | Attr.undercurl | Attr.dottedUnderline | Attr.dashedUnderline | Attr.hyperlink)) != 0
    }

    static func drawViewportImages(
        in ctx: CGContext,
        images: [FfiViewportImage],
        zFilter: (Int32) -> Bool,
        origin: CGPoint,
        cellW: CGFloat,
        cellH: CGFloat
    ) {
        for img in images where zFilter(img.zIndex) {
            let w = Int(img.widthPx)
            let h = Int(img.heightPx)
            guard w > 0, h > 0, img.rgba.count >= w * h * 4 else { continue }
            let key = "\(img.imageId):\(img.widthPx)x\(img.heightPx):\(img.rgba.hashValue)" as NSString
            let cgImg: CGImage
            if let cached = imageCache.object(forKey: key) {
                cgImg = cached
            } else {
                let colorSpace = CGColorSpaceCreateDeviceRGB()
                let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
                guard let provider = CGDataProvider(data: img.rgba as CFData),
                      let created = CGImage(
                          width: w,
                          height: h,
                          bitsPerComponent: 8,
                          bitsPerPixel: 32,
                          bytesPerRow: w * 4,
                          space: colorSpace,
                          bitmapInfo: bitmapInfo,
                          provider: provider,
                          decode: nil,
                          shouldInterpolate: true,
                          intent: .defaultIntent
                      ) else { continue }
                imageCache.setObject(created, forKey: key)
                cgImg = created
            }
            let left = origin.x + CGFloat(img.col) * cellW
            let top = origin.y + CGFloat(img.row) * cellH
            let width = CGFloat(max(1, img.cols)) * cellW
            let height = CGFloat(max(1, img.rows)) * cellH
            let targetRect = CGRect(x: left, y: top, width: width, height: height)
            ctx.saveGState()
            ctx.translateBy(x: targetRect.minX, y: targetRect.maxY)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(cgImg, in: CGRect(origin: .zero, size: targetRect.size))
            ctx.restoreGState()
        }
    }

    static func drawFancyUnderline(
        in ctx: CGContext,
        attrs: UInt16,
        left: CGFloat,
        top: CGFloat,
        width: CGFloat,
        cellH: CGFloat,
        color: CGColor
    ) {
        let stroke: CGFloat = 1
        let bottom = top + cellH
        ctx.setFillColor(color)
        ctx.setStrokeColor(color)
        ctx.setLineWidth(stroke)
        if (attrs & Attr.doubleUnderline) != 0 {
            ctx.fill(CGRect(x: left, y: bottom - stroke * 3, width: width, height: stroke))
            ctx.fill(CGRect(x: left, y: bottom - stroke, width: width, height: stroke))
        } else if (attrs & Attr.undercurl) != 0 {
            let waveY = bottom - stroke * 2
            let step = max(3, stroke * 3)
            let path = CGMutablePath()
            path.move(to: CGPoint(x: left, y: waveY))
            var x = left
            var up = true
            while x < left + width {
                let nextX = min(left + width, x + step)
                let midX = (x + nextX) * 0.5
                let ctrlY = up ? waveY - stroke * 1.5 : waveY + stroke * 1.5
                path.addQuadCurve(to: CGPoint(x: nextX, y: waveY), control: CGPoint(x: midX, y: ctrlY))
                x = nextX
                up.toggle()
            }
            ctx.addPath(path)
            ctx.strokePath()
        } else if (attrs & Attr.dottedUnderline) != 0 {
            let dot = max(2, stroke * 2)
            let y0 = bottom - stroke * 2
            var x = left
            while x < left + width {
                let x1 = min(left + width, x + dot)
                ctx.fill(CGRect(x: x, y: y0, width: x1 - x, height: stroke))
                x += dot * 2
            }
        } else if (attrs & Attr.dashedUnderline) != 0 {
            let dash = max(4, stroke * 4)
            let gap = max(2, stroke * 2)
            let y0 = bottom - stroke * 2
            var x = left
            while x < left + width {
                let x1 = min(left + width, x + dash)
                ctx.fill(CGRect(x: x, y: y0, width: x1 - x, height: stroke))
                x += dash + gap
            }
        } else if (attrs & Attr.hyperlink) != 0 && (attrs & Attr.underline) == 0 {
            let dot = max(2, stroke * 2)
            let y0 = bottom - stroke * 1.5
            var x = left
            while x < left + width {
                let x1 = min(left + width, x + dot)
                ctx.fill(CGRect(x: x, y: y0, width: x1 - x, height: stroke))
                x += dot * 2
            }
        }
    }

    static func drainClipboardToSystem(_ term: AlacrittyTerm) {
        let events = term.drainClipboardEvents()
        guard !events.isEmpty else { return }
        for ev in events {
            switch ev {
            case let .write(mime, data):
                if mime == "text/plain" || mime.hasPrefix("text/plain;") {
                    UIPasteboard.general.string = String(decoding: data, as: UTF8.self)
                }
            case .readRequest:
                break
            }
        }
    }

    @MainActor
    static func openHyperlinkAt(term: AlacrittyTerm, col: Int, row: Int) -> Bool {
        guard let urlStr = term.hyperlinkAt(col: col, row: row),
              let url = URL(string: urlStr) else { return false }
        UIApplication.shared.open(url)
        return true
    }
}

/// The first responder for keyboard input. Text and IME commits are forwarded to the host and
/// cleared; special keys and Ctrl/Alt chords from hardware keyboards are encoded directly.
final class TerminalInputProxy: UITextView, UITextViewDelegate {
    weak var host: TerminalHostView?
    private var handledPresses: Set<Int> = []

    init() {
        super.init(frame: .zero, textContainer: nil)
        delegate = self
        backgroundColor = .clear
        textColor = .clear
        tintColor = .clear
        isScrollEnabled = false
        autocorrectionType = .no
        autocapitalizationType = .none
        spellCheckingType = .no
        smartQuotesType = .no
        smartDashesType = .no
        smartInsertDeleteType = .no
        inlinePredictionType = .no
        keyboardType = .default
        returnKeyType = .default
        textContentType = .none
        isAccessibilityElement = false
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override var canBecomeFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        host?.focusChanged()
        return ok
    }

    override func resignFirstResponder() -> Bool {
        host?.releaseKeys()
        handledPresses.removeAll()
        let ok = super.resignFirstResponder()
        host?.focusChanged()
        return ok
    }

    /// True while the IME updates marked text: the first keystroke of a composition arrives
    /// before `markedTextRange` is set and must not be committed as plain text.
    private var updatingMarkedText = false
    private var composing: Bool { markedTextRange != nil || updatingMarkedText }

    override func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
        updatingMarkedText = true
        defer { updatingMarkedText = false }
        super.setMarkedText(markedText, selectedRange: selectedRange)
    }

    override func setAttributedMarkedText(_ markedText: NSAttributedString?, selectedRange: NSRange) {
        updatingMarkedText = true
        defer { updatingMarkedText = false }
        super.setAttributedMarkedText(markedText, selectedRange: selectedRange)
    }

    func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool {
        if composing { return true }
        if text.isEmpty { return true }
        host?.commitText(text, composing: false)
        return false
    }

    func textViewDidChange(_ textView: UITextView) {
        guard markedTextRange == nil, !updatingMarkedText, !text.isEmpty else { return }
        let committed = text ?? ""
        text = ""
        host?.commitText(committed, composing: true)
    }

    override func deleteBackward() {
        if composing || !(text ?? "").isEmpty { super.deleteBackward(); return }
        host?.softBackspace()
    }

    override func paste(_ sender: Any?) { host?.onPaste() }
    override func copy(_ sender: Any?) { host?.onCopy() }

    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        action == #selector(paste(_:)) || action == #selector(copy(_:))
    }

    // No caret, selection handles or text-interaction gestures on the invisible proxy.
    override func caretRect(for position: UITextPosition) -> CGRect { .zero }
    override func selectionRects(for range: UITextRange) -> [UITextSelectionRect] { [] }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var rest = Set<UIPress>()
        for press in presses {
            if let key = press.key, !composing, host?.hardwareKeyDown(key) == true {
                handledPresses.insert(key.keyCode.rawValue)
            } else {
                rest.insert(press)
            }
        }
        if !rest.isEmpty { super.pressesBegan(rest, with: event) }
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var rest = Set<UIPress>()
        for press in presses {
            if let key = press.key, handledPresses.remove(key.keyCode.rawValue) != nil {
                _ = host?.hardwareKeyUp(key)
            } else {
                rest.insert(press)
            }
        }
        if !rest.isEmpty { super.pressesEnded(rest, with: event) }
    }

    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        pressesEnded(presses, with: event)
    }
}

/// USB HID usage → logical terminal key (layout-independent identity, like Android key codes).
enum HIDKeyMap {
    static func key(_ code: UIKeyboardHIDUsage) -> TerminalKey? {
        let raw = code.rawValue
        switch raw {
        case 0x04...0x1D: return TerminalKey.letters[raw - 0x04]
        case 0x1E...0x26: return TerminalKey.digits[raw - 0x1E + 1]
        case 0x27: return .digit0
        case 0x3A...0x45: return TerminalKey.functionKeys[raw - 0x3A]
        case 0x59...0x61: return TerminalKey.numpadDigits[raw - 0x59 + 1]
        case 0x62: return .numpad0
        default: break
        }
        let table: [Int: TerminalKey] = [
            0x28: .enter, 0x29: .escape, 0x2A: .backspace, 0x2B: .tab, 0x2C: .space, 0x2D: .minus, 0x2E: .equal,
            0x2F: .bracketLeft, 0x30: .bracketRight, 0x31: .backslash, 0x33: .semicolon, 0x34: .quote, 0x35: .backquote,
            0x36: .comma, 0x37: .period, 0x38: .slash, 0x39: .capsLock, 0x46: .printScreen, 0x47: .scrollLock, 0x48: .pause,
            0x49: .insert, 0x4A: .home, 0x4B: .pageUp, 0x4C: .delete, 0x4D: .end, 0x4E: .pageDown, 0x4F: .right, 0x50: .left,
            0x51: .down, 0x52: .up, 0x53: .numLock, 0x54: .numpadDivide, 0x55: .numpadMultiply, 0x56: .numpadSubtract,
            0x57: .numpadAdd, 0x58: .numpadEnter, 0x63: .numpadDecimal, 0x65: .menu,
            0xE0: .controlLeft, 0xE1: .shiftLeft, 0xE2: .altLeft, 0xE4: .controlRight, 0xE5: .shiftRight, 0xE6: .altRight,
        ]
        return table[raw]
    }
}

extension UIColor {
    convenience init(rgb: Int) {
        self.init(red: CGFloat((rgb >> 16) & 0xFF) / 255, green: CGFloat((rgb >> 8) & 0xFF) / 255, blue: CGFloat(rgb & 0xFF) / 255, alpha: 1)
    }

    convenience init(argb: UInt32) {
        let alpha = (argb >> 24) & 0xFF
        self.init(red: CGFloat((argb >> 16) & 0xFF) / 255, green: CGFloat((argb >> 8) & 0xFF) / 255,
                  blue: CGFloat(argb & 0xFF) / 255, alpha: alpha == 0 ? 1 : CGFloat(alpha) / 255)
    }
}
