package ai.eclosion.octoterm.android.term;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/** Reads the W advance in em units from bundled OpenType fonts, without rasterizer rounding. */
public final class FontAdvance {
    private FontAdvance() {}

    public static double readEm(InputStream input) throws IOException {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        int count;
        while ((count = input.read(buffer)) != -1) bytes.write(buffer, 0, count);
        ByteBuffer font = ByteBuffer.wrap(bytes.toByteArray()).order(ByteOrder.BIG_ENDIAN);
        int head = table(font, 0x68656164);
        int hhea = table(font, 0x68686561);
        int hmtx = table(font, 0x686d7478);
        int cmap = table(font, 0x636d6170);
        int units = u16(font, head + 18);
        int metrics = u16(font, hhea + 34);
        int glyph = 0;
        for (int i = 0; i < u16(font, cmap + 2); i++) {
            int record = cmap + 4 + i * 8;
            int platform = u16(font, record);
            int encoding = u16(font, record + 2);
            if (platform != 0 && !(platform == 3 && (encoding == 1 || encoding == 10))) continue;
            int subtable = cmap + font.getInt(record + 4);
            int format = u16(font, subtable);
            if (format == 4) glyph = glyph4(font, subtable, 'W');
            if (format == 12) glyph = glyph12(font, subtable, 'W');
            if (glyph != 0) break;
        }
        if (glyph == 0 || units == 0 || metrics == 0) throw new IOException("Font has no W advance");
        int width = u16(font, hmtx + Math.min(glyph, metrics - 1) * 4);
        if (width == 0) throw new IOException("Font has zero W advance");
        return (double) width / units;
    }

    private static int table(ByteBuffer font, int tag) throws IOException {
        for (int i = 0; i < u16(font, 4); i++) {
            int record = 12 + i * 16;
            if (font.getInt(record) == tag) return font.getInt(record + 8);
        }
        throw new IOException("Required font table is missing");
    }

    private static int glyph4(ByteBuffer font, int start, int code) {
        int segments = u16(font, start + 6) / 2;
        for (int i = 0; i < segments; i++) {
            int end = u16(font, start + 14 + i * 2);
            int first = u16(font, start + 16 + segments * 2 + i * 2);
            if (code < first || code > end) continue;
            int delta = font.getShort(start + 16 + segments * 4 + i * 2);
            int rangeAddress = start + 16 + segments * 6 + i * 2;
            int range = u16(font, rangeAddress);
            if (range == 0) return (code + delta) & 0xffff;
            int glyph = u16(font, rangeAddress + range + (code - first) * 2);
            return glyph == 0 ? 0 : (glyph + delta) & 0xffff;
        }
        return 0;
    }

    private static int glyph12(ByteBuffer font, int start, int code) {
        int groups = font.getInt(start + 12);
        for (int i = 0; i < groups; i++) {
            int group = start + 16 + i * 12;
            int first = font.getInt(group);
            if (code >= first && code <= font.getInt(group + 4)) return font.getInt(group + 8) + code - first;
        }
        return 0;
    }

    private static int u16(ByteBuffer font, int offset) { return font.getShort(offset) & 0xffff; }
}
