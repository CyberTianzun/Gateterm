/* Uses the installed xterm renderer, not a reimplementation of its rounding. */
window.measureTerminalFonts = async function measureTerminalFonts() {
  const report = { platform: "web", userAgent: navigator.userAgent, dpr: devicePixelRatio, rows: [] };
  const host = document.getElementById("host");
  for (const [font, family] of [["JetBrainsMono", "JetBrains Mono"], ["GoogleSansCode", "Google Sans Code"], ["ZedMono", "Zed Mono"]]) {
    const face = await new FontFace(family, `url(../../clients/web/src/fonts/${font}-Regular.woff2)`, { weight: "400" }).load();
    document.fonts.add(face);
    await document.fonts.ready;
    for (const size of [6, 8, 12, 14, 16, 20, 32, 48]) {
     for (const [lineHeight, letterSpacing] of [[1, 0], [1.2, 1.5], [0.8, -0.5], [1.15, 0.5]]) {
      for (const renderer of ["webgl", "dom"]) {
        const terminal = new Terminal({ cols: 20, rows: 5, fontFamily: `"${family}", monospace`, fontSize: size,
          fontWeight: "normal", fontWeightBold: "bold", lineHeight, letterSpacing, customGlyphs: true });
        let addon;
        try {
          terminal.open(host);
          if (renderer === "webgl") {
            addon = new WebglAddon.WebglAddon();
            terminal.loadAddon(addon);
          }
          await new Promise(resolve => terminal.write("MW0123456789 \u4e2d\u6587", resolve));
          await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
          const metrics = terminal._core._charSizeService;
          const dimensions = terminal._core._renderService.dimensions;
          const ctx = new OffscreenCanvas(100, 100).getContext("2d");
          ctx.font = `${size}px "${family}", monospace`;
          const samples = {};
          for (const sample of ["M", "W", "MW0123456789", "\u4e2d\u6587"]) {
            const m = ctx.measureText(sample);
            samples[sample] = { advanceCssPx: m.width,
              fontAscentCssPx: m.fontBoundingBoxAscent, fontDescentCssPx: m.fontBoundingBoxDescent,
              inkBoundsCssPx: [-m.actualBoundingBoxLeft, -m.actualBoundingBoxAscent, m.actualBoundingBoxRight, m.actualBoundingBoxDescent] };
          }
          const row = { font, size, renderer, lineHeight, letterSpacing, dpr: devicePixelRatio,
            measuredWidthCssPx: metrics.width, measuredHeightCssPx: metrics.height,
            cellWidthDevicePx: dimensions.device.cell.width, cellHeightDevicePx: dimensions.device.cell.height,
            charLeftDevicePx: dimensions.device.char.left, charTopDevicePx: dimensions.device.char.top,
            cellWidthCssPx: dimensions.css.cell.width, cellHeightCssPx: dimensions.css.cell.height,
            grid80x24DevicePx: [80 * dimensions.device.cell.width, 24 * dimensions.device.cell.height], samples };
          report.rows.push(row);
          console.log("GatetermFontMetrics", JSON.stringify(row));
        } finally {
          terminal.dispose();
          host.replaceChildren();
        }
      }
     }
    }
    document.fonts.delete(face);
  }
  return report;
};

// Capture the actual WebGL output for the block-character crab, with/without
// xterm's custom glyph path. Preserve the drawing buffer only in this probe.
window.renderBlockGlyphSample = async function renderBlockGlyphSample() {
  const images = [];
  const rows = [];
  const host = document.getElementById("host");
  const crab = [" \u2590\u259b\u2588\u2588\u2588\u259c\u258c ",
    "\u259d\u259c\u2588\u2588\u2588\u2588\u2588\u259b\u2598", "  \u2598\u2598 \u259d\u259d  "];
  for (const [font, family] of [["JetBrainsMono", "JetBrains Mono"], ["GoogleSansCode", "Google Sans Code"], ["ZedMono", "Zed Mono"]]) {
    const face = await new FontFace(family, `url(../../clients/web/src/fonts/${font}-Regular.woff2)`, { weight: "400" }).load();
    document.fonts.add(face);
    for (const customGlyphs of [false, true]) {
      const terminal = new Terminal({ cols: 9, rows: 3, fontFamily: `"${family}", monospace`, fontSize: 14,
        lineHeight: 1, letterSpacing: 0, customGlyphs, theme: { foreground: "#d97757", background: "#191a1b" } });
      try {
        terminal.open(host);
        const addon = new WebglAddon.WebglAddon(true);
        terminal.loadAddon(addon);
        await new Promise(resolve => terminal.write("\x1b[?25l" + crab.join("\r\n"), resolve));
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        images.push({ font, customGlyphs, png: addon._renderer._canvas.toDataURL("image/png") });
        rows.push({ font, customGlyphs, dpr: devicePixelRatio, dimensions: terminal._core._renderService.dimensions.device });
      } finally { terminal.dispose(); host.replaceChildren(); }
    }
    document.fonts.delete(face);
  }
  return { rows, images };
};
