package ai.eclosion.gateterm.android.term;

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

    /**
     * Normalized box-drawing segments in 16ths of a cell: left, top, right, bottom (0..16).
     * Covers Unicode Box-Drawing U+2500..U+257F so TUI borders never show 1px font seams.
     */
    public static int[] boxSegments(int codepoint) {
        if (codepoint < 0x2500 || codepoint > 0x257f) return null;
        int[] arms = armWeights(codepoint);
        if (arms != null) {
            // arms = {up, down, left, right} where 0=none, 1=light (7..9), 2=heavy (6..10)
            int u = arms[0], d = arms[1], l = arms[2], r = arms[3];
            int maxW = Math.max(Math.max(u, d), Math.max(l, r));
            int halfMax = maxW; // light=1 -> [7..9], heavy=2 -> [6..10]
            int count = (u > 0 ? 1 : 0) + (d > 0 ? 1 : 0) + (l > 0 ? 1 : 0) + (r > 0 ? 1 : 0);
            int[] out = new int[count * 4];
            int idx = 0;
            if (u > 0) {
                out[idx++] = 8 - u; out[idx++] = 0; out[idx++] = 8 + u; out[idx++] = 8 + halfMax;
            }
            if (d > 0) {
                out[idx++] = 8 - d; out[idx++] = 8 - halfMax; out[idx++] = 8 + d; out[idx++] = 16;
            }
            if (l > 0) {
                out[idx++] = 0; out[idx++] = 8 - l; out[idx++] = 8 + halfMax; out[idx++] = 8 + l;
            }
            if (r > 0) {
                out[idx++] = 8 - halfMax; out[idx++] = 8 - r; out[idx++] = 16; out[idx++] = 8 + r;
            }
            return out;
        }
        return specialBoxSegments(codepoint);
    }

    private static int[] armWeights(int cp) {
        switch (cp) {
            case '─': case '╭': case '╮': case '╯': case '╰':
                if (cp == '─') return new int[] {0, 0, 1, 1};
                if (cp == '╭') return new int[] {0, 1, 0, 1};
                if (cp == '╮') return new int[] {0, 1, 1, 0};
                if (cp == '╯') return new int[] {1, 0, 1, 0};
                return new int[] {1, 0, 0, 1};
            case '━': return new int[] {0, 0, 2, 2};
            case '│': return new int[] {1, 1, 0, 0};
            case '┃': return new int[] {2, 2, 0, 0};
            case '┌': return new int[] {0, 1, 0, 1};
            case '┍': return new int[] {0, 1, 0, 2};
            case '┎': return new int[] {0, 2, 0, 1};
            case '┏': return new int[] {0, 2, 0, 2};
            case '┐': return new int[] {0, 1, 1, 0};
            case '┑': return new int[] {0, 1, 2, 0};
            case '┒': return new int[] {0, 2, 1, 0};
            case '┓': return new int[] {0, 2, 2, 0};
            case '└': return new int[] {1, 0, 0, 1};
            case '┕': return new int[] {1, 0, 0, 2};
            case '┖': return new int[] {2, 0, 0, 1};
            case '┗': return new int[] {2, 0, 0, 2};
            case '┘': return new int[] {1, 0, 1, 0};
            case '┙': return new int[] {1, 0, 2, 0};
            case '┚': return new int[] {2, 0, 1, 0};
            case '┛': return new int[] {2, 0, 2, 0};
            case '├': return new int[] {1, 1, 0, 1};
            case '┝': return new int[] {1, 1, 0, 2};
            case '┞': return new int[] {2, 1, 0, 1};
            case '┟': return new int[] {1, 2, 0, 1};
            case '┠': return new int[] {2, 2, 0, 1};
            case '┡': return new int[] {2, 1, 0, 2};
            case '┢': return new int[] {1, 2, 0, 2};
            case '┣': return new int[] {2, 2, 0, 2};
            case '┤': return new int[] {1, 1, 1, 0};
            case '┥': return new int[] {1, 1, 2, 0};
            case '┦': return new int[] {2, 1, 1, 0};
            case '┧': return new int[] {1, 2, 1, 0};
            case '┨': return new int[] {2, 2, 1, 0};
            case '┩': return new int[] {2, 1, 2, 0};
            case '┪': return new int[] {1, 2, 2, 0};
            case '┫': return new int[] {2, 2, 2, 0};
            case '┬': return new int[] {0, 1, 1, 1};
            case '┭': return new int[] {0, 1, 2, 1};
            case '┮': return new int[] {0, 1, 1, 2};
            case '┯': return new int[] {0, 1, 2, 2};
            case '┰': return new int[] {0, 2, 1, 1};
            case '┱': return new int[] {0, 2, 2, 1};
            case '┲': return new int[] {0, 2, 1, 2};
            case '┳': return new int[] {0, 2, 2, 2};
            case '┴': return new int[] {1, 0, 1, 1};
            case '┵': return new int[] {1, 0, 2, 1};
            case '┶': return new int[] {1, 0, 1, 2};
            case '┷': return new int[] {1, 0, 2, 2};
            case '┸': return new int[] {2, 0, 1, 1};
            case '┹': return new int[] {2, 0, 2, 1};
            case '┺': return new int[] {2, 0, 1, 2};
            case '┻': return new int[] {2, 0, 2, 2};
            case '┼': return new int[] {1, 1, 1, 1};
            case '┽': return new int[] {1, 1, 2, 1};
            case '┾': return new int[] {1, 1, 1, 2};
            case '┿': return new int[] {1, 1, 2, 2};
            case '╀': return new int[] {2, 1, 1, 1};
            case '╁': return new int[] {1, 2, 1, 1};
            case '╂': return new int[] {2, 2, 1, 1};
            case '╃': return new int[] {2, 1, 2, 1};
            case '╄': return new int[] {2, 1, 1, 2};
            case '╅': return new int[] {1, 2, 2, 1};
            case '╆': return new int[] {1, 2, 1, 2};
            case '╇': return new int[] {2, 1, 2, 2};
            case '╈': return new int[] {1, 2, 2, 2};
            case '╉': return new int[] {2, 2, 2, 1};
            case '╊': return new int[] {2, 2, 1, 2};
            case '╋': return new int[] {2, 2, 2, 2};
            case '╴': return new int[] {0, 0, 1, 0};
            case '╵': return new int[] {1, 0, 0, 0};
            case '╶': return new int[] {0, 0, 0, 1};
            case '╷': return new int[] {0, 1, 0, 0};
            case '╸': return new int[] {0, 0, 2, 0};
            case '╹': return new int[] {2, 0, 0, 0};
            case '╺': return new int[] {0, 0, 0, 2};
            case '╻': return new int[] {0, 2, 0, 0};
            case '╼': return new int[] {0, 0, 1, 2};
            case '╽': return new int[] {1, 2, 0, 0};
            case '╾': return new int[] {0, 0, 2, 1};
            case '╿': return new int[] {2, 1, 0, 0};
            default: return null;
        }
    }

    private static int[] specialBoxSegments(int cp) {
        switch (cp) {
            case '═': return new int[] {0, 5, 16, 7, 0, 9, 16, 11};
            case '║': return new int[] {5, 0, 7, 16, 9, 0, 11, 16};
            case '╒': return new int[] {7, 5, 16, 7, 7, 9, 16, 11, 7, 5, 9, 16};
            case '╓': return new int[] {5, 7, 16, 9, 5, 7, 7, 16, 9, 7, 11, 16};
            case '╔': return new int[] {5, 5, 16, 7, 9, 9, 16, 11, 5, 5, 7, 16, 9, 9, 11, 16};
            case '╕': return new int[] {0, 5, 9, 7, 0, 9, 9, 11, 7, 5, 9, 16};
            case '╖': return new int[] {0, 7, 11, 9, 5, 7, 7, 16, 9, 7, 11, 16};
            case '╗': return new int[] {0, 5, 11, 7, 0, 9, 7, 11, 9, 5, 11, 16, 5, 9, 7, 16};
            case '╘': return new int[] {7, 0, 9, 11, 7, 5, 16, 7, 7, 9, 16, 11};
            case '╙': return new int[] {5, 0, 7, 9, 9, 0, 11, 9, 5, 7, 16, 9};
            case '╚': return new int[] {5, 0, 7, 11, 9, 0, 11, 7, 5, 9, 16, 11, 9, 5, 16, 7};
            case '╛': return new int[] {7, 0, 9, 11, 0, 5, 9, 7, 0, 9, 9, 11};
            case '╜': return new int[] {5, 0, 7, 9, 9, 0, 11, 9, 0, 7, 11, 9};
            case '╝': return new int[] {9, 0, 11, 11, 5, 0, 7, 7, 0, 9, 11, 11, 0, 5, 7, 7};
            case '╞': return new int[] {7, 0, 9, 16, 7, 5, 16, 7, 7, 9, 16, 11};
            case '╟': return new int[] {5, 0, 7, 16, 9, 0, 11, 16, 9, 7, 16, 9};
            case '╠': return new int[] {5, 0, 7, 16, 9, 0, 11, 7, 9, 9, 11, 16, 9, 5, 16, 7, 9, 9, 16, 11};
            case '╡': return new int[] {7, 0, 9, 16, 0, 5, 9, 7, 0, 9, 9, 11};
            case '╢': return new int[] {5, 0, 7, 16, 9, 0, 11, 16, 0, 7, 7, 9};
            case '╣': return new int[] {9, 0, 11, 16, 5, 0, 7, 7, 5, 9, 7, 16, 0, 5, 7, 7, 0, 9, 7, 11};
            case '╤': return new int[] {0, 5, 16, 7, 0, 9, 16, 11, 7, 9, 9, 16};
            case '╥': return new int[] {0, 7, 16, 9, 5, 7, 7, 16, 9, 7, 11, 16};
            case '╦': return new int[] {0, 5, 16, 7, 0, 9, 7, 11, 9, 9, 16, 11, 5, 9, 7, 16, 9, 9, 11, 16};
            case '╧': return new int[] {0, 5, 16, 7, 0, 9, 16, 11, 7, 0, 9, 7};
            case '╨': return new int[] {0, 7, 16, 9, 5, 0, 7, 9, 9, 0, 11, 9};
            case '╩': return new int[] {0, 9, 16, 11, 0, 5, 7, 7, 9, 5, 16, 7, 5, 0, 7, 7, 9, 0, 11, 7};
            case '╪': return new int[] {7, 0, 9, 16, 0, 5, 16, 7, 0, 9, 16, 11};
            case '╫': return new int[] {5, 0, 7, 16, 9, 0, 11, 16, 0, 7, 16, 9};
            case '╬': return new int[] {5, 0, 7, 7, 9, 0, 11, 7, 5, 9, 7, 16, 9, 9, 11, 16, 0, 5, 7, 7, 9, 5, 16, 7, 0, 9, 7, 11, 9, 9, 16, 11};
            case '╌': case '┄': case '┈': return new int[] {1, 7, 6, 9, 10, 7, 15, 9};
            case '╍': case '┅': case '┉': return new int[] {1, 6, 6, 10, 10, 6, 15, 10};
            case '╎': case '┆': case '┊': return new int[] {7, 1, 9, 6, 7, 10, 9, 15};
            case '╏': case '┇': case '┋': return new int[] {6, 1, 10, 6, 6, 10, 10, 15};
            default: return null;
        }
    }

    public boolean draw(Canvas canvas, int codepoint, float left, float top,
                        float width, float height, int color) {
        int[] rects = rectangles(codepoint);
        if (rects != null) {
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
        int[] box = boxSegments(codepoint);
        if (box != null) {
            fill.setColor(color);
            for (int i = 0; i < box.length; i += 4) {
                float x0 = Math.round(left + box[i] * width / 16f);
                float y0 = Math.round(top + box[i + 1] * height / 16f);
                float x1 = Math.max(x0 + 1f, Math.round(left + box[i + 2] * width / 16f));
                float y1 = Math.max(y0 + 1f, Math.round(top + box[i + 3] * height / 16f));
                canvas.drawRect(x0, y0, x1, y1, fill);
            }
            return true;
        }
        return false;
    }
}
