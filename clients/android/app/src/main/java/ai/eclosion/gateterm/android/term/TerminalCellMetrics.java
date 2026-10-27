package ai.eclosion.gateterm.android.term;

/** Pixel-grid rules matching the pinned xterm WebGL renderer. No Android dependency. */
public final class TerminalCellMetrics {
    public final int width;
    public final int height;
    public final int charHeight;
    public final int charLeft;
    public final int charTop;

    public TerminalCellMetrics(double logicalAdvance, double logicalAscent, double logicalDescent,
                               double density, double lineHeight, double letterSpacing) {
        // xterm's OptionsService rejects lineHeight < 1; don't permit overlapping rows.
        lineHeight = Math.max(1.0, lineHeight);
        // Blink shapes advances using 16.16 fixed-point precision. Preserve that
        // precision before xterm's floor, including values just below an integer.
        double advance = Math.floor(logicalAdvance * 65536.0) / 65536.0;
        width = Math.max(1, (int) Math.floor(advance * density) + (int) Math.floor(letterSpacing + 0.5));
        // Canvas fontBoundingBox metrics round ascent/descent in logical pixels
        // first. Rounding only after multiplying by density produces taller rows.
        double logicalHeight = Math.floor(logicalAscent + 0.5) + Math.floor(logicalDescent + 0.5);
        charHeight = Math.max(1, (int) Math.ceil(logicalHeight * density));
        height = Math.max(1, (int) Math.floor(charHeight * lineHeight));
        charLeft = (int) Math.floor(letterSpacing / 2.0);
        charTop = lineHeight == 1.0 ? 0 : (int) Math.floor((height - charHeight) / 2.0 + 0.5);
    }
}
