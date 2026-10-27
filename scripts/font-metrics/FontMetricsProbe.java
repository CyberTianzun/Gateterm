package ai.eclosion.gateterm.diagnostics.fontmetrics;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Context;
import android.content.res.Configuration;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.os.Bundle;
import android.util.DisplayMetrics;
import android.util.Log;
import org.json.JSONArray;
import org.json.JSONObject;
import ai.eclosion.gateterm.android.term.TerminalFontMetrics;
import ai.eclosion.gateterm.android.term.FontAdvance;

/** Standalone instrumentation: does not launch or replace the Gateterm app. */
public final class FontMetricsProbe extends Instrumentation {
    private String mode = "metrics";
    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        if (arguments != null) mode = arguments.getString("mode", "metrics");
        start();
    }

    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            Context device = getTargetContext();
            if (mode.equals("blocks")) {
                result.putString("stream", "\nBLOCK_GLYPHS_JSON=" + BlockGlyphProbe.run(device).toString() + "\n");
                finish(Activity.RESULT_OK, result);
                return;
            }
            DisplayMetrics dm = device.getResources().getDisplayMetrics();
            JSONObject report = new JSONObject();
            report.put("platform", "android");
            report.put("sdk", android.os.Build.VERSION.SDK_INT);
            report.put("model", android.os.Build.MODEL);
            report.put("densityDpi", dm.densityDpi);
            report.put("density", dm.density);
            report.put("fontScale", device.getResources().getConfiguration().fontScale);
            JSONArray rows = new JSONArray();
            measure(device, "device", rows);
            for (int dpi : new int[] {160, 320, dm.densityDpi}) {
                for (float scale : new float[] {1f, 1.3f}) {
                    Configuration config = new Configuration(device.getResources().getConfiguration());
                    config.densityDpi = dpi;
                    config.fontScale = scale;
                    measure(device.createConfigurationContext(config), "controlled", rows);
                }
            }
            report.put("rows", rows);
            result.putString("stream", "\nFONT_METRICS_JSON=" + report.toString() + "\n");
            finish(Activity.RESULT_OK, result);
        } catch (Throwable error) {
            result.putString("stream", Log.getStackTraceString(error));
            finish(Activity.RESULT_CANCELED, result);
        }
    }

    private void measure(Context context, String scenario, JSONArray rows) throws Exception {
        DisplayMetrics dm = context.getResources().getDisplayMetrics();
        for (String font : new String[] {"JetBrainsMono", "GoogleSansCode", "ZedMono"}) {
            Typeface face = Typeface.createFromAsset(context.getAssets(), "fonts/" + font + "-Regular.ttf");
            double advanceEm;
            try (java.io.InputStream input = context.getAssets().open("fonts/" + font + "-Regular.ttf")) {
                advanceEm = FontAdvance.readEm(input);
            }
            for (int size : new int[] {6, 8, 12, 14, 16, 20, 32, 48}) {
              for (double[] spacing : new double[][] {{1, 0}, {1.2, 1.5}, {0.8, -0.5}, {1.15, 0.5}}) {
                float px = size * dm.density;
                // Call the actual production measurement code, not a copied formula.
                TerminalFontMetrics measured = new TerminalFontMetrics(size, dm.density, face, advanceEm, spacing[0], spacing[1]);
                Paint paint = measured.text;
                Paint.FontMetrics fm = paint.getFontMetrics();
                float cellW = measured.cell.width;
                float cellH = measured.cell.height;
                JSONObject row = new JSONObject();
                row.put("scenario", scenario);
                row.put("font", font);
                row.put("size", size);
                row.put("lineHeight", spacing[0]);
                row.put("letterSpacing", spacing[1]);
                row.put("densityDpi", dm.densityDpi);
                row.put("dpr", dm.density);
                row.put("fontScale", context.getResources().getConfiguration().fontScale);
                row.put("textSizeDevicePx", px);
                row.put("ascent", fm.ascent);
                row.put("descent", fm.descent);
                row.put("leading", fm.leading);
                row.put("MWidth", paint.measureText("M"));
                row.put("WWidth", paint.measureText("W"));
                row.put("cellWidthDevicePx", cellW);
                row.put("cellHeightDevicePx", cellH);
                row.put("baselineDevicePx", measured.baseline);
                row.put("charLeftDevicePx", measured.cell.charLeft);
                row.put("charTopDevicePx", measured.cell.charTop);
                row.put("logicalAdvance", measured.logicalAdvance);
                row.put("logicalAscent", measured.logicalAscent);
                row.put("logicalDescent", measured.logicalDescent);
                row.put("grid80x24DevicePx", new JSONArray(new double[] {80 * cellW, 24 * cellH}));
                JSONObject samples = new JSONObject();
                for (String sample : new String[] {"M", "W", "MW0123456789", "\u4e2d\u6587"}) {
                    Rect bounds = new Rect();
                    paint.getTextBounds(sample, 0, sample.length(), bounds);
                    samples.put(sample, new JSONObject()
                        .put("advance", paint.measureText(sample))
                        .put("inkBounds", new JSONArray(new int[] {bounds.left, bounds.top, bounds.right, bounds.bottom})));
                }
                row.put("samples", samples);
                Log.i("GatetermFontMetrics", row.toString());
                rows.put(row);
              }
            }
        }
    }
}
