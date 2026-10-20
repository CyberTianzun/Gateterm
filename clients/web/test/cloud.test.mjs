import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildSync } from 'esbuild';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const outfile = resolve(here, '../../../target/web-cloud-test.mjs');
buildSync({ stdin: { contents: 'export * from "./cloud"; export {setLocale} from "./i18n";', resolveDir: resolve(here, '../src'), loader: 'ts' }, bundle: true, format: 'esm', outfile });
const { fetchCloudStatus, cloudStatusText, mountCloudStatus, setLocale } = await import(pathToFileURL(outfile));
const ok = body => ({ ok: true, json: async () => body });

test('Cloud status uses bearer auth and bypasses caches', async t => {
  const original = globalThis.fetch;
  t.after(() => { globalThis.fetch = original; });
  let seen;
  globalThis.fetch = async (url, init) => { seen = { url, init }; return ok({ connected: true, email: 'a@example.com', token: 'not-for-the-ui' }); };
  assert.deepEqual(await fetchCloudStatus('local-token'), { state: 'connected', email: 'a@example.com' });
  assert.equal(seen.url, '/api/cloud/status');
  assert.equal(seen.init.headers.Authorization, 'Bearer local-token');
  assert.equal(seen.init.cache, 'no-store');
});

test('Logout is distinct from a network failure or an invalid response', async t => {
  const original = globalThis.fetch;
  t.after(() => { globalThis.fetch = original; });
  globalThis.fetch = async () => ok({ connected: false, email: null });
  assert.deepEqual(await fetchCloudStatus('tok'), { state: 'disconnected' });
  for (const body of [{ connected: true }, { connected: true, email: '' }, {}, '<html>']) {
    globalThis.fetch = async () => ok(body);
    await assert.rejects(fetchCloudStatus('tok'));
  }
  globalThis.fetch = async () => ({ ok: false, status: 401 });
  await assert.rejects(fetchCloudStatus('tok'));
  globalThis.fetch = async () => { throw new TypeError('offline'); };
  await assert.rejects(fetchCloudStatus('tok'));
});

test('Sidebar ignores stale results and refreshes account, locale and logout on focus', async () => {
  const original = { fetch: globalThis.fetch, window: globalThis.window, document: globalThis.document };
  globalThis.window = new EventTarget();
  globalThis.document = Object.assign(new EventTarget(), { visibilityState: 'visible' });
  const element = { textContent: '', dataset: {} };
  let release;
  globalThis.fetch = () => new Promise(resolve => { release = resolve; });
  setLocale('en');
  const ui = mountCloudStatus(element, () => 'tok');
  const tick = () => new Promise(resolve => setImmediate(resolve));
  try {
    ui.setOnline(true);
    ui.setOnline(false);
    release(ok({ connected: true, email: 'stale@example.com' }));
    await tick();
    assert.equal(element.textContent, cloudStatusText({ state: 'unavailable' }));
    globalThis.fetch = async () => ok({ connected: true, email: 'current@example.com' });
    ui.setOnline(true);
    await tick();
    assert.equal(element.textContent, 'current@example.com connected to Octoterm Cloud');
    setLocale('zh-CN');
    assert.equal(element.textContent, 'current@example.com 已连接到 Octoterm Cloud');
    globalThis.fetch = async () => ok({ connected: false, email: null });
    window.dispatchEvent(new Event('focus'));
    await tick();
    assert.equal(element.textContent, '没有连接到 Octoterm Cloud');
    assert.equal(element.dataset.connected, 'false');
  } finally {
    ui.dispose();
    Object.assign(globalThis, original);
  }
});
