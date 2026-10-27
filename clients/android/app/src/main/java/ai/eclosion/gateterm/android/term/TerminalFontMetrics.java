package ai.eclosion.gateterm.android.term;

import android.graphics.Paint;
import android.graphics.Typeface;

/** Shared by the terminal renderer and the standalone on-device metrics probe. */
public final class TerminalFontMetrics {
    public final Paint text;
    public final TerminalCellMetrics cell;
    public final float baseline;
    public final double logicalAdvance;
    public final float logicalAscent;
    public final float logicalDescent;

    public TerminalFontMetrics(float fontSize, float density, Typeface typeface, double advanceEm,
                               double lineHeight, double letterSpacing) {
        Paint measure = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.SUBPIXEL_TEXT_FLAG | Paint.LINEAR_TEXT_FLAG);
        measure.setTypeface(typeface);
        measure.setTextSize(fontSize);
        Paint.FontMetrics logical = measure.getFontMetrics();
        // Paint.measureText rounds the sum up even with subpixel positioning.
        // getTextWidths exposes the unrounded glyph advance that Canvas uses.
        float[] widths = new float[1];
        measure.getTextWidths("W", widths);
        // Bundled fonts use their exact OpenType advance, avoiding Android's
        // intermediate float/Minikin rounding. System fallback uses Paint.
        logicalAdvance = Double.isFinite(advanceEm) && advanceEm > 0 ? advanceEm * fontSize : widths[0];
        logicalAscent = -logical.ascent;
        logicalDescent = logical.descent;
        cell = new TerminalCellMetrics(logicalAdvance, logicalAscent, logicalDescent, density, lineHeight, letterSpacing);
        text = new Paint(measure);
        text.setTextSize(fontSize * density);
        // xterm places the ideographic baseline at the bottom of the char box,
        // then centers that box in the row when lineHeight differs from 1.
        baseline = cell.charTop + cell.charHeight - Math.round(text.getFontMetrics().descent);
    }
}
