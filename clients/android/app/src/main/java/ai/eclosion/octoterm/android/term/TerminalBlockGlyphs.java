package ai.eclosion.octoterm.android.term;

import android.graphics.Canvas;
import android.graphics.Paint;

/** Unicode block elements fill cell geometry, independently of the text font/baseline. */
public final class TerminalBlockGlyphs {
    private static final int[][] BLOCKS = new int[32][];
    private final Paint fill = new Paint(); // No antialiasing at shared cell edges.

    static {
        BLOCKS[0] = new int[] {0, 0, 8, 4}; // Upper half.
        for (int eighths = 1; eighths <= 8; eighths++) {
            BLOCKS[eighths] = new int[] {0, 8 - eighths, 8, 8};
        }
        for (int code = 0x2589; code <= 0x258f; code++) {
            BLOCKS[code - 0x2580] = new int[] {0, 0, 0x2590 - code, 8};
        }
        BLOCKS[0x10] = new int[] {4, 0, 8, 8}; // Right half.
        // U+2591..2593 are shade patterns, not solid block elements.
        BLOCKS[0x14] = new int[] {0, 0, 8, 1};
        BLOCKS[0x15] = new int[] {7, 0, 8, 8};
        // Bits: upper-left, upper-right, lower-left, lower-right.
        int[] quadrants = {4, 8, 1, 13, 9, 7, 11, 2, 6, 14};
        for (int i = 0; i < quadrants.length; i++) {
            int mask = quadrants[i];
            int[] rects = new int[Integer.bitCount(mask) * 4];
            int index = 0;
            for (int bit = 0; bit < 4; bit++) {
                if ((mask & (1 << bit)) == 0) continue;
                int x = (bit & 1) * 4;
                int y = (bit >> 1) * 4;
                rects[index++] = x; rects[index++] = y;
                rects[index++] = x + 4; rects[index++] = y + 4;
            }
            BLOCKS[0x16 + i] = rects;
        }
    }

    /** Read-only rectangles in eighths of a cell: left, top, right, bottom. */
    public static int[] rectangles(int codepoint) {
        return codepoint >= 0x2580 && codepoint <= 0x259f ? BLOCKS[codepoint - 0x2580] : null;
    }

    public boolean draw(Canvas canvas, int codepoint, float left, float top,
                        float width, float height, int color) {
        int[] rects = rectangles(codepoint);
        if (rects == null) return false;
        fill.setColor(color);
        for (int i = 0; i < rects.length; i += 4) {
            // Adjacent parts use the same rounded edge, including odd cell sizes.
            canvas.drawRect(Math.round(left + rects[i] * width / 8f),
                Math.round(top + rects[i + 1] * height / 8f),
                Math.round(left + rects[i + 2] * width / 8f),
                Math.round(top + rects[i + 3] * height / 8f), fill);
        }
        return true;
    }
}
