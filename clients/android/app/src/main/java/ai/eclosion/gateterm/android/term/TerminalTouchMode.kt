package ai.eclosion.gateterm.android.term

import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.roundToInt

/**
 * 终端触控交互方案。
 *
 * - [Touch]：手指优先（默认）——单指滑动永远滚动（含 Agent 滚轮与全屏程序方向键），
 *   程序启用鼠标时轻点优先发鼠标单击（不弹软键盘），未开鼠标时轻点呼出键盘，长按进入文字选择（带拖拽手柄与复制/粘贴菜单）。
 * - [Mouse]：鼠标兼容——终端程序开启鼠标上报时，单指拖动直接作为鼠标拖拽发给程序
 *   （如 Agent 拖选自动复制、分屏拖拽），双指滑动滚动，长按仍可本地选字。
 */
enum class TerminalTouchMode(val key: String) {
    Touch("touch"),
    Mouse("mouse"),
    ;

    companion object {
        val Default: TerminalTouchMode = Touch

        fun parse(raw: String?): TerminalTouchMode =
            entries.firstOrNull { it.key == raw } ?: Default
    }
}

enum class SingleFingerDragAction {
    Scroll,
    MouseDrag,
}

enum class TapAction {
    RevealTopBar,
    DismissSelection,
    MouseClick,
    FocusOnly,
}

enum class TwoFingerMode {
    Scroll,
    Pinch,
}

enum class SelectionHandle {
    Start,
    End,
}

object TerminalTouchPolicy {
    private const val PINCH_SCALE_THRESHOLD = 0.12f

    fun singleFingerDragAction(
        mode: TerminalTouchMode,
        mouseTracking: Boolean,
        isHardwareMouse: Boolean = false,
    ): SingleFingerDragAction {
        if (mouseTracking && (mode == TerminalTouchMode.Mouse || isHardwareMouse)) {
            return SingleFingerDragAction.MouseDrag
        }
        return SingleFingerDragAction.Scroll
    }

    fun tapAction(
        inTopRevealZone: Boolean,
        hasSelection: Boolean,
        mouseTracking: Boolean,
    ): TapAction = when {
        inTopRevealZone -> TapAction.RevealTopBar
        hasSelection -> TapAction.DismissSelection
        mouseTracking -> TapAction.MouseClick
        else -> TapAction.FocusOnly
    }

    fun decideTwoFingerGesture(
        scaleDelta: Float,
        panDistancePx: Float,
        touchSlopPx: Float,
    ): TwoFingerMode? = when {
        abs(scaleDelta - 1f) >= PINCH_SCALE_THRESHOLD -> TwoFingerMode.Pinch
        panDistancePx >= touchSlopPx -> TwoFingerMode.Scroll
        else -> null
    }

    fun clampFontSize(baseSize: Float, scale: Float): Float =
        (baseSize * scale).roundToInt().coerceIn(6, 48).toFloat()
}

object TerminalSelectionHandles {
    fun normalize(sel: TermSelection): TermSelection = sel.normalized()

    fun handlePositions(
        sel: TermSelection,
        ox: Float,
        oy: Float,
        cellW: Float,
        cellH: Float,
        dropPx: Float,
    ): Pair<Pair<Float, Float>, Pair<Float, Float>> {
        val s = sel.normalized()
        val start = (ox + s.x0 * cellW) to (oy + (s.y0 + 1) * cellH + dropPx)
        val end = (ox + (s.x1 + 1) * cellW) to (oy + (s.y1 + 1) * cellH + dropPx)
        return start to end
    }

    fun hitTestHandle(
        x: Float,
        y: Float,
        sel: TermSelection,
        ox: Float,
        oy: Float,
        cellW: Float,
        cellH: Float,
        dropPx: Float,
        hitRadiusPx: Float,
    ): SelectionHandle? {
        val (start, end) = handlePositions(sel, ox, oy, cellW, cellH, dropPx)
        val dStart = hypot(x - start.first, y - start.second)
        val dEnd = hypot(x - end.first, y - end.second)
        return when {
            dStart <= hitRadiusPx && dEnd <= hitRadiusPx ->
                if (dStart <= dEnd) SelectionHandle.Start else SelectionHandle.End
            dStart <= hitRadiusPx -> SelectionHandle.Start
            dEnd <= hitRadiusPx -> SelectionHandle.End
            else -> null
        }
    }

    fun updateHandle(
        sel: TermSelection,
        handle: SelectionHandle,
        col: Int,
        row: Int,
    ): Pair<TermSelection, SelectionHandle> {
        val s = sel.normalized()
        return when (handle) {
            SelectionHandle.Start -> {
                if (compareCell(col, row, s.x1, s.y1) > 0) {
                    TermSelection(s.x1, s.y1, col, row) to SelectionHandle.End
                } else {
                    TermSelection(col, row, s.x1, s.y1) to SelectionHandle.Start
                }
            }
            SelectionHandle.End -> {
                if (compareCell(col, row, s.x0, s.y0) < 0) {
                    TermSelection(col, row, s.x0, s.y0) to SelectionHandle.Start
                } else {
                    TermSelection(s.x0, s.y0, col, row) to SelectionHandle.End
                }
            }
        }
    }

    fun extendWordSelection(wordAnchor: TermSelection, col: Int, row: Int): TermSelection {
        val w = wordAnchor.normalized()
        return when {
            compareCell(col, row, w.x0, w.y0) < 0 -> TermSelection(col, row, w.x1, w.y1)
            compareCell(col, row, w.x1, w.y1) > 0 -> TermSelection(w.x0, w.y0, col, row)
            else -> w
        }
    }

    fun menuAnchor(
        sel: TermSelection,
        ox: Float,
        oy: Float,
        cellW: Float,
        cellH: Float,
        viewportW: Float,
        viewportH: Float,
        menuW: Float,
        menuH: Float,
        gapPx: Float,
        handleClearancePx: Float,
        edgeMarginPx: Float = 8f,
    ): Pair<Float, Float> {
        val s = sel.normalized()
        val firstRowRight = if (s.y0 == s.y1) ox + (s.x1 + 1) * cellW else viewportW - ox
        val centerX = (ox + s.x0 * cellW + firstRowRight) * 0.5f
        val maxX = (viewportW - menuW - edgeMarginPx).coerceAtLeast(edgeMarginPx)
        val x = (centerX - menuW * 0.5f).coerceIn(edgeMarginPx, maxX)

        val selTop = oy + s.y0 * cellH
        val selBottom = oy + (s.y1 + 1) * cellH
        val aboveY = selTop - menuH - gapPx
        val maxY = (viewportH - menuH - edgeMarginPx).coerceAtLeast(edgeMarginPx)
        val y = if (aboveY >= edgeMarginPx) {
            aboveY
        } else {
            (selBottom + handleClearancePx + gapPx).coerceIn(edgeMarginPx, maxY)
        }
        return x to y
    }

    private fun compareCell(colA: Int, rowA: Int, colB: Int, rowB: Int): Int {
        if (rowA != rowB) return rowA.compareTo(rowB)
        return colA.compareTo(colB)
    }
}
