package ai.eclosion.gateterm.diagnostics.fontmetrics;

import android.content.Context;
import android.content.res.Configuration;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Typeface;
import ai.eclosion.gateterm.android.term.FontAdvance;
import ai.eclosion.gateterm.android.term.TerminalFontMetrics;
import ai.eclosion.gateterm.android.term.TerminalBlockGlyphs;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import org.json.JSONArray;
import org.json.JSONObject;

/** Pixel coverage checks using the same block renderer as TerminalCanvas. */
public final class BlockGlyphProbe {
    private static final int BG = 0xff191a1b;
    private static final int FG = 0xffd97757;
    private static final String[] CRAB = {
        " \u2590\u259b\u2588\u2588\u2588\u259c\u258c ",
        "\u259d\u259c\u2588\u2588\u2588\u2588\u2588\u259b\u2598",
        "  \u2598\u2598 \u259d\u259d  "
    };
    private static final String[] SOLID = {"\u2588\u2588\u2588\u2588", "\u2588\u2588\u2588\u2588", "\u2588\u2588\u2588\u2588"};

    public static JSONObject run(Context device) throws Exception {
        JSONObject report = new JSONObject();
        report.put("model", android.os.Build.MODEL);
        report.put("sdk", android.os.Build.VERSION.SDK_INT);
        int deviceDpi = device.getResources().getDisplayMetrics().densityDpi;
        report.put("densityDpi", deviceDpi);
        JSONArray rows = new JSONArray();
        JSONArray images = new JSONArray();
        int reproduced = 0;
        for (int dpi : new int[] {160, 320, deviceDpi}) {
            Configuration config = new Configuration(device.getResources().getConfiguration());
            config.densityDpi = dpi;
            Context context = device.createConfigurationContext(config);
            float density = context.getResources().getDisplayMetrics().density;
            for (String font : new String[] {"JetBrainsMono", "GoogleSansCode", "ZedMono"}) {
                String path = "fonts/" + font + "-Regular.ttf";
                Typeface face = Typeface.createFromAsset(context.getAssets(), path);
                double advance;
                try (InputStream input = context.getAssets().open(path)) { advance = FontAdvance.readEm(input); }
                for (int size : new int[] {12, 14, 16}) {
                    for (double[] spacing : new double[][] {{1, 0}, {1.2, 1.5}}) {
                        TerminalFontMetrics metrics = new TerminalFontMetrics(size, density, face, advance, spacing[0], spacing[1]);
                        for (int origin : new int[] {0, 1}) {
                            for (int color : new int[] {FG, 0x99d97757, 0xff7aa2f7}) {
                                Bitmap before = render(CRAB, metrics, origin, color, false);
                                Bitmap after = render(CRAB, metrics, origin, color, true);
                                Bitmap solid = render(SOLID, metrics, origin, color, true);
                                int oldGaps = missingCrabJoin(before, metrics, origin);
                                int newGaps = missingCrabJoin(after, metrics, origin);
                                int solidGaps = missingSolid(solid, metrics, origin);
                                if (oldGaps > 0) reproduced++;
                                if (newGaps != 0 || solidGaps != 0) throw new AssertionError(
                                    font + " " + size + " dpi=" + dpi + " gaps=" + newGaps + "/" + solidGaps);
                                rows.put(new JSONObject().put("font", font).put("size", size).put("dpi", dpi)
                                    .put("lineHeight", spacing[0]).put("letterSpacing", spacing[1]).put("origin", origin)
                                    .put("color", Integer.toHexString(color)).put("oldCrabJoinMissingPixels", oldGaps)
                                    .put("newCrabJoinMissingPixels", newGaps).put("solidMissingPixels", solidGaps));
                                if (dpi == deviceDpi && size == 14 && spacing[0] == 1 && origin == 0 && color == FG) {
                                    images.put(save(device, before, "crab-" + font + "-before.png"));
                                    images.put(save(device, after, "crab-" + font + "-after.png"));
                                }
                                before.recycle(); after.recycle(); solid.recycle();
                            }
                        }
                    }
                }
            }
        }
        if (reproduced == 0) throw new AssertionError("The old drawing path did not reproduce any seams");
        return report.put("cases", rows).put("reproducedOldSeams", reproduced).put("images", images);
    }

    private static Bitmap render(String[] lines, TerminalFontMetrics metrics, int origin, int color, boolean custom) {
        int width = metrics.cell.width, height = metrics.cell.height;
        Bitmap bitmap = Bitmap.createBitmap(lines[0].length() * width + 2 * origin, lines.length * height + 2 * origin, Bitmap.Config.ARGB_8888);
        Canvas canvas = new Canvas(bitmap);
        canvas.drawColor(BG);
        Paint background = new Paint(); background.setColor(BG);
        Paint text = new Paint(metrics.text); text.setColor(color);
        TerminalBlockGlyphs blocks = new TerminalBlockGlyphs();
        for (int y = 0; y < lines.length; y++) {
            for (int x = 0; x < lines[y].length(); x++) {
                float left = origin + x * width, top = origin + y * height;
                canvas.drawRect(left, top, left + width, top + height, background);
                char glyph = lines[y].charAt(x);
                if (!custom || !blocks.draw(canvas, glyph, left, top, width, height, color)) {
                    canvas.drawText(String.valueOf(glyph), left + metrics.cell.charLeft, top + metrics.baseline, text);
                }
            }
        }
        return bitmap;
    }

    private static int missingCrabJoin(Bitmap bitmap, TerminalFontMetrics metrics, int origin) {
        int missing = 0;
        // The middle three body columns must remain solid through the first row join.
        for (int x = origin + 3 * metrics.cell.width; x < origin + 6 * metrics.cell.width; x++) {
            for (int y = origin + metrics.cell.height - 1; y <= origin + metrics.cell.height; y++) {
                if (bitmap.getPixel(x, y) == BG) missing++;
            }
        }
        // Each leg's upper quadrant touches the body's lower edge at the second join.
        for (double column : new double[] {2.25, 3.25, 5.75, 6.75}) {
            int x = origin + (int) (column * metrics.cell.width);
            for (int y = origin + 2 * metrics.cell.height - 1; y <= origin + 2 * metrics.cell.height; y++) {
                if (bitmap.getPixel(x, y) == BG) missing++;
            }
        }
        return missing;
    }

    private static int missingSolid(Bitmap bitmap, TerminalFontMetrics metrics, int origin) {
        int missing = 0;
        int[] pixels = new int[bitmap.getWidth() * bitmap.getHeight()];
        bitmap.getPixels(pixels, 0, bitmap.getWidth(), 0, 0, bitmap.getWidth(), bitmap.getHeight());
        for (int y = origin; y < origin + 3 * metrics.cell.height; y++) {
            for (int x = origin; x < origin + 4 * metrics.cell.width; x++) {
                if (pixels[y * bitmap.getWidth() + x] == BG) missing++;
            }
        }
        return missing;
    }

    private static String save(Context context, Bitmap bitmap, String name) throws Exception {
        File file = new File(context.getExternalFilesDir(null), name);
        try (FileOutputStream out = new FileOutputStream(file)) { bitmap.compress(Bitmap.CompressFormat.PNG, 100, out); }
        return file.getAbsolutePath();
    }
}
