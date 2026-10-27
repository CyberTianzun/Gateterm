// Node 22+; uses a separate headless Chrome profile and the actual installed xterm.
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const out = path.join(root, "target/font-metrics");
await mkdir(out, { recursive: true });
const profile = await mkdtemp(path.join(out, "chrome-"));
const chrome = process.env.CHROME_PATH || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const blockMode = process.argv.includes("--blocks");
const url = process.argv.slice(2).find(arg => !arg.startsWith("--")) || "http://127.0.0.1:8766/scripts/font-metrics/probe.html";
const android = JSON.parse((await readFile(path.join(out, "android.json"), "utf8")).replace(/^\uFEFF/, ""));
const dprs = blockMode ? [android.density] : [...new Set([1, 2, android.density])];
const child = spawn(chrome, ["--headless=new", "--no-first-run", "--no-default-browser-check",
  "--enable-unsafe-swiftshader", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"],
  { windowsHide: true, stdio: ["ignore", "ignore", "pipe"] });
let ws;
let send;
try {
  const endpoint = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Chrome startup timed out")), 30000);
    let stderr = "";
    child.once("error", reject);
    child.once("exit", code => { clearTimeout(timer); reject(new Error(`Chrome exited: ${code}`)); });
    child.stderr.on("data", data => {
      stderr += data.toString();
      const match = stderr.match(/DevTools listening on (ws:\/\/\S+)/);
      if (match) { clearTimeout(timer); resolve(match[1]); }
    });
  });
  ws = new WebSocket(endpoint);
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  let nextId = 0;
  const pending = new Map();
  const logs = [];
  ws.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject, timer } = pending.get(message.id);
      pending.delete(message.id); clearTimeout(timer);
      message.error ? reject(new Error(JSON.stringify(message.error))) : resolve(message.result);
    }
    if (message.method === "Runtime.consoleAPICalled") {
      logs.push(message.params.args.map(arg => arg.value ?? arg.description).join(" "));
    }
  });
  send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 60000);
    pending.set(id, { resolve, reject, timer });
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  const browser = await send("Browser.getVersion");
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const page = (method, params) => send(method, params, sessionId);
  await page("Runtime.enable");
  await page("Page.enable");
  const rows = [];
  for (const dpr of dprs) {
    await page("Emulation.setDeviceMetricsOverride", { width: 1200, height: 900, deviceScaleFactor: dpr, mobile: false });
    await page("Page.navigate", { url });
    for (let attempt = 0; ; attempt++) {
      const ready = await page("Runtime.evaluate", { expression: "typeof window.measureTerminalFonts === 'function'", returnByValue: true });
      if (ready.result.value) break;
      if (attempt === 100) throw new Error("Probe page did not load");
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    const result = await page("Runtime.evaluate", { expression: blockMode ? "window.renderBlockGlyphSample()" : "window.measureTerminalFonts()", awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    rows.push(...result.result.value.rows);
    for (const image of result.result.value.images || []) {
      const file = path.join(out, `crab-web-${image.font}-${image.customGlyphs ? "custom" : "font"}.png`);
      await writeFile(file, Buffer.from(image.png.split(",")[1], "base64"));
    }
    console.log(`DPR ${dpr}: measured ${result.result.value.rows.length} WebGL/DOM cases`);
  }
  const stem = blockMode ? "web-blocks" : "web";
  await writeFile(path.join(out, `${stem}.json`), JSON.stringify({ platform: "web", browser, rows }, null, 2));
  await writeFile(path.join(out, `${stem}.log`), logs.join("\n") + "\n");
  console.log(`Results: ${out}/${stem}.json`);
} finally {
  if (send && ws?.readyState === WebSocket.OPEN) await send("Browser.close").catch(() => {});
  ws?.close();
  child.kill();
}
