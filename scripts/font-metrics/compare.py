"""Join real Android Paint and browser xterm measurements at equal density/size."""
import csv
import json
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[2]
output = root / "target/font-metrics"
android = json.loads((output / "android.json").read_text(encoding="utf-8-sig"))
web = json.loads((output / "web.json").read_text(encoding="utf-8"))
rows = []
for native in android["rows"]:
    for browser in web["rows"]:
        if (native["font"], native["size"], native["dpr"], native.get("lineHeight", 1), native.get("letterSpacing", 0)) != (
            browser["font"], browser["size"], browser["dpr"], browser.get("lineHeight", 1), browser.get("letterSpacing", 0)
        ):
            continue
        aw, ah = native["cellWidthDevicePx"], native["cellHeightDevicePx"]
        ww, wh = browser["cellWidthDevicePx"], browser["cellHeightDevicePx"]
        rows.append(dict(
            scenario=native["scenario"], font=native["font"], size=native["size"],
            dpr=native["dpr"], renderer=browser["renderer"],
            font_scale=native["fontScale"], line_height=native.get("lineHeight", 1), letter_spacing=native.get("letterSpacing", 0),
            android_width_px=aw, android_height_px=ah,
            web_width_px=ww, web_height_px=wh,
            width_delta_px=aw-ww, height_delta_px=ah-wh,
            width_delta_percent=100*(aw/ww-1), height_delta_percent=100*(ah/wh-1),
            android_grid_width_px=80*aw, android_grid_height_px=24*ah,
            web_grid_width_px=80*ww, web_grid_height_px=24*wh,
            grid_width_delta_px=80*(aw-ww), grid_height_delta_px=24*(ah-wh),
            grid_width_delta_logical_px=80*(aw-ww)/native["dpr"],
            grid_height_delta_logical_px=24*(ah-wh)/native["dpr"],
        ))
if not rows:
    raise RuntimeError("No matching measurements")
(output / "comparison.json").write_text(json.dumps(rows, indent=2), encoding="utf-8")
with (output / "comparison.csv").open("w", encoding="utf-8-sig", newline="") as f:
    writer = csv.DictWriter(f, fieldnames=rows[0].keys())
    writer.writeheader()
    writer.writerows(rows)
print(f"Android {android['sdk']} / {android['model']}, {android['densityDpi']} dpi, font scale {android['fontScale']}")
print(web["browser"]["product"])
print("font / size | Android cell px | WebGL cell px | 80x24 delta px")
for row in rows:
    if row["scenario"] == "device" and row["renderer"] == "webgl" and row["line_height"] == 1 and row["letter_spacing"] == 0:
        print(f"{row['font']:15} / {row['size']} | "
              f"{row['android_width_px']:.3f} x {row['android_height_px']:.3f} | "
              f"{row['web_width_px']:.3f} x {row['web_height_px']:.3f} | "
              f"{row['grid_width_delta_px']:+.3f} x {row['grid_height_delta_px']:+.3f}")
print(f"Saved {len(rows)} paired cases to {output / 'comparison.csv'}")
if "--assert-webgl" in sys.argv:
    mismatches = [r for r in rows if r["renderer"] == "webgl" and (r["width_delta_px"] != 0 or r["height_delta_px"] != 0)]
    print(f"WebGL mismatches: {len(mismatches)}")
    if mismatches:
        print(json.dumps(mismatches[:8], indent=2))
        raise SystemExit(1)
if "--write-fixture" in sys.argv:
    cases = []
    for native in android["rows"]:
        if native["scenario"] != "controlled" or abs(native["fontScale"] - 1) > 0.0001:
            continue
        if native["size"] not in [6, 12, 14, 16, 20, 48]:
            continue
        if native["lineHeight"] != 1 and native["size"] != 14:
            continue
        browser = next(r for r in web["rows"] if r["renderer"] == "webgl" and all(
            r[key] == native[key] for key in ["font", "size", "dpr", "lineHeight", "letterSpacing"]))
        case = {key: native[key] for key in ["font", "size", "dpr", "lineHeight", "letterSpacing",
                                           "logicalAdvance", "logicalAscent", "logicalDescent"]}
        for key in ["cellWidthDevicePx", "cellHeightDevicePx", "charLeftDevicePx", "charTopDevicePx"]:
            case[key] = browser[key]
        cases.append(case)
    fixture = root / "clients/android/app/src/test/resources/terminal-webgl-metrics.json"
    fixture.parent.mkdir(parents=True, exist_ok=True)
    fixture.write_text('{\n  "reference": "' + web["browser"]["product"] +
                       '; @xterm/xterm 5.5.0; @xterm/addon-webgl 0.18.0",\n  "cases": [\n' +
                       ',\n'.join('    ' + json.dumps(case) for case in cases) + '\n  ]\n}\n', encoding="utf-8")
    print(f"Saved {len(cases)} regression fixtures: {fixture}")
