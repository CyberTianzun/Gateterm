package ai.eclosion.gateterm.android.ui.term

import android.graphics.Typeface
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntSize
import ai.eclosion.gateterm.android.appearance.TermAppearance
import ai.eclosion.gateterm.android.term.Attr
import ai.eclosion.gateterm.android.term.TermFrame
import ai.eclosion.gateterm.android.term.TermGeometry
import ai.eclosion.gateterm.android.term.TermSelection
import ai.eclosion.gateterm.android.term.TerminalFontMetrics
import ai.eclosion.gateterm.android.term.TerminalBlockGlyphs

class TermPaint(
    val text: android.graphics.Paint,
    val fill: android.graphics.Paint,
    val cellW: Float,
    val cellH: Float,
    val baseline: Float,
    val charLeft: Float,
)

fun measureTerm(fontSize: Float, density: Float, typeface: Typeface, advanceEm: Double, lineHeight: Double, letterSpacing: Double): TermPaint {
    val metrics = TerminalFontMetrics(fontSize, density, typeface, advanceEm, lineHeight, letterSpacing)
    return TermPaint(
        text = metrics.text,
        fill = android.graphics.Paint(),
        cellW = metrics.cell.width.toFloat(),
        cellH = metrics.cell.height.toFloat(),
        baseline = metrics.baseline,
        charLeft = metrics.cell.charLeft.toFloat(),
    )
}

@Composable
fun TerminalCanvas(
    frame: TermFrame,
    appearance: TermAppearance,
    selection: TermSelection?,
    paint: TermPaint,
    onProposeSize: (Int, Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val textPaint = remember(paint) { paint.text }
    val bgPaint = remember(paint) { paint.fill }
    val blockGlyphs = remember { TerminalBlockGlyphs() }
    val cellW = paint.cellW
    val cellH = paint.cellH
    val baseline = paint.baseline
    val shown = frame
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    val proposeSize by rememberUpdatedState(onProposeSize)
    // A spacing/font change must resize the PTY even if the viewport did not change.
    LaunchedEffect(viewport, cellW, cellH) {
        val cols = (viewport.width / cellW).toInt()
        val rows = (viewport.height / cellH).toInt()
        if (cols >= 20 && rows >= 5) proposeSize(cols, rows)
    }

    Canvas(
        modifier.onSizeChanged { viewport = it },
    ) {
        drawRect(androidx.compose.ui.graphics.Color(appearance.background or 0xFF000000.toInt()))
        val cols = shown.cols
        val rows = shown.rows
        if (cols <= 0 || rows <= 0) return@Canvas
        val (ox, oy) = TermGeometry.origin(size.width, size.height, cols, rows, cellW, cellH)
        val sel = selection?.let { normalizeSelection(it) }
        drawIntoCanvas { canvas ->
            val native = canvas.nativeCanvas
            for (y in 0 until rows) {
                val row = shown.cells.getOrNull(y) ?: continue
                for (x in 0 until cols) {
                    val cell = row.getOrNull(x) ?: continue
                    if (cell.width == 0) continue
                    var fg = appearance.colorFor(cell.fgIndex, cell.fg, foreground = true)
                    var bg = appearance.colorFor(cell.bgIndex, cell.bg, foreground = false)
                    if (cell.attrs and Attr.INVERSE != 0) {
                        val tmp = fg
                        fg = bg
                        bg = tmp
                    }
                    if (cell.attrs and Attr.HIDDEN != 0) fg = bg
                    val selected = sel != null && inSelection(sel, x, y)
                    val left = ox + x * cellW
                    val top = oy + y * cellH
                    bgPaint.color = if (selected) appearance.selection else (bg or 0xFF000000.toInt())
                    native.drawRect(left, top, left + cellW * cell.width, top + cellH, bgPaint)
                    if (cell.codepoint != ' '.code && cell.codepoint != 0) {
                        val alpha = if (cell.attrs and Attr.DIM != 0) 0x99000000.toInt() else 0xFF000000.toInt()
                        textPaint.color = (fg and 0x00FFFFFF) or alpha
                        textPaint.isFakeBoldText = cell.attrs and Attr.BOLD != 0
                        textPaint.textSkewX = if (cell.attrs and Attr.ITALIC != 0) -0.2f else 0f
                        textPaint.isUnderlineText = cell.attrs and Attr.UNDERLINE != 0
                        textPaint.isStrikeThruText = cell.attrs and Attr.STRIKE != 0
                        val glyph = cell.glyphText()
                        val blockDrawn = glyph.length == 1 && blockGlyphs.draw(
                            native, cell.codepoint, left, top, cellW * cell.width, cellH, textPaint.color,
                        )
                        if (!blockDrawn) native.drawText(glyph, left + paint.charLeft, top + baseline, textPaint)
                    }
                }
            }
            if (shown.cursorVisible && (sel == null)) {
                bgPaint.color = (appearance.cursor or 0xFF000000.toInt()) and 0x00FFFFFF or 0xA0000000.toInt()
                val left = ox + shown.cursorX * cellW
                val top = oy + shown.cursorY * cellH
                native.drawRect(left, top, left + cellW, top + cellH, bgPaint)
            }
        }
    }
}

private fun normalizeSelection(sel: TermSelection): TermSelection {
    return if (sel.y0 < sel.y1 || (sel.y0 == sel.y1 && sel.x0 <= sel.x1)) {
        sel
    } else {
        TermSelection(sel.x1, sel.y1, sel.x0, sel.y0)
    }
}

private fun inSelection(sel: TermSelection, x: Int, y: Int): Boolean {
    if (y < sel.y0 || y > sel.y1) return false
    if (sel.y0 == sel.y1) return x in sel.x0..sel.x1
    if (y == sel.y0) return x >= sel.x0
    if (y == sel.y1) return x <= sel.x1
    return true
}
