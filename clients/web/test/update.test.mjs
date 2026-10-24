import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const result = await build({ entryPoints: [fileURLToPath(new URL("../src/update.ts", import.meta.url))],
  bundle: true, format: "esm", write: false });
const { fetchUpdateStatus } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`);
const release = { version: "0.3.0", url: "https://github.com/CyberTianzun/Octoterm/releases/tag/release-v0.3.0" };
const ok = (update) => ({ ok: true, status: 200, json: async () => ({ current_version: "0.2.3", update }) });

test("version status reads the authenticated local cache, never GitHub directly", async (t) => {
  const original = globalThis.fetch;
  t.after(() => { globalThis.fetch = original; });
  let seen;
  const controller = new AbortController();
  globalThis.fetch = async (url, init) => { seen = { url, init }; return ok(release); };
  assert.deepEqual(await fetchUpdateStatus("local-token", controller.signal), release);
  assert.equal(seen.url, "/api/update");
  assert.equal(seen.init.headers.Authorization, "Bearer local-token");
  assert.equal(seen.init.cache, "no-store");
  assert.equal(seen.init.signal, controller.signal);
});

test("no update and older server endpoints return no offer", async (t) => {
  const original = globalThis.fetch;
  t.after(() => { globalThis.fetch = original; });
  globalThis.fetch = async () => ok(null);
  assert.equal(await fetchUpdateStatus("t"), null);
  globalThis.fetch = async () => ({ ok: false, status: 404 });
  assert.equal(await fetchUpdateStatus("t"), null);
  globalThis.fetch = async () => ({ ok: false, status: 503 });
  await assert.rejects(fetchUpdateStatus("t"));
});

test("only this repository's HTTPS Release pages are accepted", async (t) => {
  const original = globalThis.fetch;
  t.after(() => { globalThis.fetch = original; });
  for (const url of ["javascript:alert(1)", "http://github.com/CyberTianzun/Octoterm/releases/latest",
    "https://github.com.evil.example/CyberTianzun/Octoterm/releases/latest",
    "https://github.com/other/project/releases/latest", "https://user@github.com/CyberTianzun/Octoterm/releases/latest"]) {
    globalThis.fetch = async () => ok({ ...release, url });
    await assert.rejects(fetchUpdateStatus("t"), url);
  }
  globalThis.fetch = async () => ok({ ...release, version: "" });
  await assert.rejects(fetchUpdateStatus("t"));
});
