import CoreGraphics
import Foundation

/// Terminal touch interaction preset.
///
/// - `touch` (default): one-finger swipe always scrolls (including TUI/agent wheel and alt-screen arrows),
///   tap prioritizes mouse click without opening the keyboard when mouse tracking is active (opens keyboard otherwise),
///   and long-press enters local text selection with handles and Copy/Paste menu.
/// - `mouse`: when an app enables mouse tracking, one-finger drag forwards left-button mouse drag directly
///   (e.g. agent drag-to-copy, pane resizing); two-finger swipe scrolls and long-press selects locally.
enum TerminalTouchMode: String, CaseIterable, Identifiable {
    case touch
    case mouse

    var id: String { rawValue }
    static let defaultMode: TerminalTouchMode = .touch

    static func parse(_ raw: String?) -> TerminalTouchMode {
        guard let raw, let parsed = TerminalTouchMode(rawValue: raw) else { return .defaultMode }
        return parsed
    }
}

enum SingleFingerDragAction: Equatable {
    case scroll
    case mouseDrag
}

enum TapAction: Equatable {
    case revealTopBar
    case dismissSelection
    case mouseClick
    case focusOnly
}

enum TwoFingerMode: Equatable {
    case scroll
    case pinch
}

enum SelectionHandle: Equatable {
    case start
    case end
}

enum TerminalTouchPolicy {
    static let pinchScaleThreshold: CGFloat = 0.12

    static func singleFingerDragAction(
        mode: TerminalTouchMode,
        mouseTracking: Bool,
        isIndirectPointer: Bool = false
    ) -> SingleFingerDragAction {
        if mouseTracking && (mode == .mouse || isIndirectPointer) {
            return .mouseDrag
        }
        return .scroll
    }

    static func tapAction(
        inTopRevealZone: Bool,
        hasSelection: Bool,
        mouseTracking: Bool
    ) -> TapAction {
        if inTopRevealZone { return .revealTopBar }
        if hasSelection { return .dismissSelection }
        if mouseTracking { return .mouseClick }
        return .focusOnly
    }

    static func decideTwoFingerGesture(
        scaleDelta: CGFloat,
        panDistance: CGFloat,
        touchSlop: CGFloat
    ) -> TwoFingerMode? {
        if abs(scaleDelta - 1) >= pinchScaleThreshold { return .pinch }
        if panDistance >= touchSlop { return .scroll }
        return nil
    }

    static func clampFontSize(baseSize: Double, scale: Double) -> Double {
        min(48, max(6, (baseSize * scale).rounded()))
    }
}

enum TerminalSelectionHandles {
    static let dropDistance: CGFloat = 10
    static let knobRadius: CGFloat = 7
    static let hitRadius: CGFloat = 28

    static func handlePositions(
        selection: TermSelection,
        origin: CGPoint,
        cellWidth: CGFloat,
        cellHeight: CGFloat,
        drop: CGFloat = dropDistance
    ) -> (start: CGPoint, end: CGPoint) {
        let s = selection.normalized
        let start = CGPoint(
            x: origin.x + CGFloat(s.x0) * cellWidth,
            y: origin.y + CGFloat(s.y0 + 1) * cellHeight + drop
        )
        let end = CGPoint(
            x: origin.x + CGFloat(s.x1 + 1) * cellWidth,
            y: origin.y + CGFloat(s.y1 + 1) * cellHeight + drop
        )
        return (start, end)
    }

    static func hitTestHandle(
        point: CGPoint,
        selection: TermSelection,
        origin: CGPoint,
        cellWidth: CGFloat,
        cellHeight: CGFloat,
        drop: CGFloat = dropDistance,
        hitRadius: CGFloat = hitRadius
    ) -> SelectionHandle? {
        let (start, end) = handlePositions(
            selection: selection,
            origin: origin,
            cellWidth: cellWidth,
            cellHeight: cellHeight,
            drop: drop
        )
        let dStart = hypot(point.x - start.x, point.y - start.y)
        let dEnd = hypot(point.x - end.x, point.y - end.y)
        if dStart <= hitRadius && dEnd <= hitRadius {
            return dStart <= dEnd ? .start : .end
        }
        if dStart <= hitRadius { return .start }
        if dEnd <= hitRadius { return .end }
        return nil
    }

    static func updateHandle(
        selection: TermSelection,
        handle: SelectionHandle,
        col: Int,
        row: Int
    ) -> (selection: TermSelection, handle: SelectionHandle) {
        let s = selection.normalized
        switch handle {
        case .start:
            if compareCell(col, row, s.x1, s.y1) > 0 {
                return (TermSelection(x0: s.x1, y0: s.y1, x1: col, y1: row), .end)
            }
            return (TermSelection(x0: col, y0: row, x1: s.x1, y1: s.y1), .start)
        case .end:
            if compareCell(col, row, s.x0, s.y0) < 0 {
                return (TermSelection(x0: col, y0: row, x1: s.x0, y1: s.y0), .start)
            }
            return (TermSelection(x0: s.x0, y0: s.y0, x1: col, y1: row), .end)
        }
    }

    static func extendWordSelection(wordAnchor: TermSelection, col: Int, row: Int) -> TermSelection {
        let w = wordAnchor.normalized
        if compareCell(col, row, w.x0, w.y0) < 0 {
            return TermSelection(x0: col, y0: row, x1: w.x1, y1: w.y1)
        }
        if compareCell(col, row, w.x1, w.y1) > 0 {
            return TermSelection(x0: w.x0, y0: w.y0, x1: col, y1: row)
        }
        return w
    }

    private static func compareCell(_ colA: Int, _ rowA: Int, _ colB: Int, _ rowB: Int) -> Int {
        if rowA != rowB { return rowA < rowB ? -1 : 1 }
        if colA != colB { return colA < colB ? -1 : 1 }
        return 0
    }
}
