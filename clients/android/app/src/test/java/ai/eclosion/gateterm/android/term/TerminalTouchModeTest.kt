package ai.eclosion.gateterm.android.term

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalTouchModeTest {
    @Test
    fun parseDefaultsToTouchAndRoundTripsKnownValues() {
        assertEquals(TerminalTouchMode.Touch, TerminalTouchMode.Default)
        assertEquals(TerminalTouchMode.Touch, TerminalTouchMode.parse(null))
        assertEquals(TerminalTouchMode.Touch, TerminalTouchMode.parse("unknown"))
        for (mode in TerminalTouchMode.entries) {
            assertEquals(mode, TerminalTouchMode.parse(mode.key))
        }
    }

    @Test
    fun singleFingerDragActionRespectsModeMouseTrackingAndHardwareMouse() {
        assertEquals(
            SingleFingerDragAction.Scroll,
            TerminalTouchPolicy.singleFingerDragAction(TerminalTouchMode.Touch, mouseTracking = true),
        )
        assertEquals(
            SingleFingerDragAction.Scroll,
            TerminalTouchPolicy.singleFingerDragAction(TerminalTouchMode.Touch, mouseTracking = false),
        )
        assertEquals(
            SingleFingerDragAction.MouseDrag,
            TerminalTouchPolicy.singleFingerDragAction(
                TerminalTouchMode.Touch,
                mouseTracking = true,
                isHardwareMouse = true,
            ),
        )
        assertEquals(
            SingleFingerDragAction.MouseDrag,
            TerminalTouchPolicy.singleFingerDragAction(TerminalTouchMode.Mouse, mouseTracking = true),
        )
        assertEquals(
            SingleFingerDragAction.Scroll,
            TerminalTouchPolicy.singleFingerDragAction(TerminalTouchMode.Mouse, mouseTracking = false),
        )
    }

    @Test
    fun tapActionPrioritizesTopRevealAndSelectionDismissalOverMouseClick() {
        assertEquals(
            TapAction.RevealTopBar,
            TerminalTouchPolicy.tapAction(inTopRevealZone = true, hasSelection = true, mouseTracking = true),
        )
        assertEquals(
            TapAction.DismissSelection,
            TerminalTouchPolicy.tapAction(inTopRevealZone = false, hasSelection = true, mouseTracking = true),
        )
        assertEquals(
            TapAction.MouseClick,
            TerminalTouchPolicy.tapAction(inTopRevealZone = false, hasSelection = false, mouseTracking = true),
        )
        assertEquals(
            TapAction.FocusOnly,
            TerminalTouchPolicy.tapAction(inTopRevealZone = false, hasSelection = false, mouseTracking = false),
        )
    }

    @Test
    fun twoFingerDisambiguationAndFontClamp() {
        assertNull(TerminalTouchPolicy.decideTwoFingerGesture(scaleDelta = 1.05f, panDistancePx = 4f, touchSlopPx = 12f))
        assertEquals(
            TwoFingerMode.Pinch,
            TerminalTouchPolicy.decideTwoFingerGesture(scaleDelta = 1.15f, panDistancePx = 2f, touchSlopPx = 12f),
        )
        assertEquals(
            TwoFingerMode.Pinch,
            TerminalTouchPolicy.decideTwoFingerGesture(scaleDelta = 0.85f, panDistancePx = 20f, touchSlopPx = 12f),
        )
        assertEquals(
            TwoFingerMode.Scroll,
            TerminalTouchPolicy.decideTwoFingerGesture(scaleDelta = 1.03f, panDistancePx = 14f, touchSlopPx = 12f),
        )
        assertEquals(6f, TerminalTouchPolicy.clampFontSize(10f, 0.2f), 0f)
        assertEquals(48f, TerminalTouchPolicy.clampFontSize(30f, 2.5f), 0f)
        assertEquals(18f, TerminalTouchPolicy.clampFontSize(14f, 1.28f), 0f)
    }

    @Test
    fun handleHitTestingPicksClosestWithinRadius() {
        val sel = TermSelection(2, 1, 5, 1)
        val (startPt, endPt) = TerminalSelectionHandles.handlePositions(
            sel, ox = 10f, oy = 20f, cellW = 10f, cellH = 20f, dropPx = 12f,
        )
        // start is at left edge of col 2 (10 + 20 = 30), bottom of row 1 (20 + 40 + 12 = 72)
        assertEquals(30f, startPt.first, 0.01f)
        assertEquals(72f, startPt.second, 0.01f)
        // end is at right edge of col 5 (10 + 60 = 70), bottom of row 1 (72)
        assertEquals(70f, endPt.first, 0.01f)
        assertEquals(72f, endPt.second, 0.01f)

        assertEquals(
            SelectionHandle.Start,
            TerminalSelectionHandles.hitTestHandle(32f, 70f, sel, 10f, 20f, 10f, 20f, 12f, 30f),
        )
        assertEquals(
            SelectionHandle.End,
            TerminalSelectionHandles.hitTestHandle(68f, 74f, sel, 10f, 20f, 10f, 20f, 12f, 30f),
        )
        assertNull(
            TerminalSelectionHandles.hitTestHandle(200f, 200f, sel, 10f, 20f, 10f, 20f, 12f, 30f),
        )
    }

    @Test
    fun handleUpdateSwapsCleanlyWhenCrossingOtherEndpoint() {
        val initial = TermSelection(3, 2, 7, 2)
        val (movedStart, handle1) = TerminalSelectionHandles.updateHandle(initial, SelectionHandle.Start, col = 1, row = 2)
        assertEquals(TermSelection(1, 2, 7, 2), movedStart)
        assertEquals(SelectionHandle.Start, handle1)

        // Drag Start past End -> becomes new End and swaps handle
        val (crossed, handle2) = TerminalSelectionHandles.updateHandle(initial, SelectionHandle.Start, col = 9, row = 3)
        assertEquals(TermSelection(7, 2, 9, 3), crossed)
        assertEquals(SelectionHandle.End, handle2)

        // Drag End before Start -> becomes new Start and swaps handle
        val (crossedBack, handle3) = TerminalSelectionHandles.updateHandle(initial, SelectionHandle.End, col = 1, row = 1)
        assertEquals(TermSelection(1, 1, 3, 2), crossedBack)
        assertEquals(SelectionHandle.Start, handle3)
    }

    @Test
    fun extendWordSelectionPreservesOriginalWordInBothDirections() {
        val word = TermSelection(4, 2, 8, 2)
        assertEquals(word, TerminalSelectionHandles.extendWordSelection(word, col = 6, row = 2))
        assertEquals(TermSelection(4, 2, 12, 2), TerminalSelectionHandles.extendWordSelection(word, col = 12, row = 2))
        assertEquals(TermSelection(1, 2, 8, 2), TerminalSelectionHandles.extendWordSelection(word, col = 1, row = 2))
        assertEquals(TermSelection(2, 1, 8, 2), TerminalSelectionHandles.extendWordSelection(word, col = 2, row = 1))
        assertEquals(TermSelection(4, 2, 2, 3), TerminalSelectionHandles.extendWordSelection(word, col = 2, row = 3))
    }

    @Test
    fun menuAnchorPrefersAboveSelectionAndFlipsBelowNearTopEdge() {
        // Middle of viewport: menu placed above selection
        val (xMid, yMid) = TerminalSelectionHandles.menuAnchor(
            sel = TermSelection(2, 5, 6, 5),
            ox = 0f, oy = 0f, cellW = 10f, cellH = 20f,
            viewportW = 400f, viewportH = 600f,
            menuW = 180f, menuH = 44f, gapPx = 8f, handleClearancePx = 28f,
        )
        assertTrue(yMid < 5 * 20f)
        assertTrue(xMid >= 8f && xMid + 180f <= 392f)

        // Top row of viewport: no room above, flips below handles
        val (_, yTop) = TerminalSelectionHandles.menuAnchor(
            sel = TermSelection(0, 0, 4, 0),
            ox = 0f, oy = 0f, cellW = 10f, cellH = 20f,
            viewportW = 400f, viewportH = 600f,
            menuW = 180f, menuH = 44f, gapPx = 8f, handleClearancePx = 28f,
        )
        assertTrue(yTop >= 20f + 28f)
    }
}
