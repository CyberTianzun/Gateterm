import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const result = await build({
  entryPoints: [fileURLToPath(new URL("../src/auth.ts", import.meta.url))],
  bundle: true, format: "esm", write: false,
});
const { readToken } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`);

afterEach(() => {
  for (const key of ["location", "history", "localStorage", "prompt"]) delete globalThis[key];
});

function browser(url, savedToken) {
  const storage = new Map(savedToken === undefined ? [] : [["octoterm-token", savedToken]]);
  const replacements = [];
  const prompts = [];
  globalThis.location = new URL(url);
  globalThis.localStorage = {
    getItem: key => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  };
  globalThis.history = {
    state: { session: 7 },
    replaceState(state, title, url) {
      replacements.push({ state, title, url });
      globalThis.location = new URL(url, location);
    },
  };
  globalThis.prompt = message => { prompts.push(message); return "manual-token"; };
  return { storage, replacements, prompts };
}

test("a shared token is saved and removed from the current URL; refresh still authenticates", () => {
  const { storage, replacements, prompts } = browser("https://example.com/#token=new%2Bsecret", "old-token");
  assert.equal(readToken("Token:"), "new+secret");
  assert.equal(storage.get("octoterm-token"), "new+secret");
  assert.equal(location.href, "https://example.com/");
  assert.deepEqual(replacements, [{ state: history.state, title: "", url: "/" }]);
  assert.equal(readToken("Token:"), "new+secret");
  assert.equal(replacements.length, 1);
  assert.deepEqual(prompts, []);
});

test("removing every token preserves the path, query, other fragment parameters, and history state", () => {
  const { replacements } = browser("https://example.com/web/?lang=zh#view=chat&token=secret&session=7&token=other");
  const state = history.state;
  assert.equal(readToken("Token:"), "secret");
  assert.equal(location.href, "https://example.com/web/?lang=zh#view=chat&session=7");
  assert.equal(replacements[0].state, state);
});

test("an empty URL token is removed without overwriting the saved credential", () => {
  browser("https://example.com/#token=", "saved-token");
  assert.equal(readToken("Token:"), "saved-token");
  assert.equal(location.hash, "");
});

test("URLs without a token stay unchanged and use the saved credential", () => {
  const url = "https://example.com/web/?lang=en#terminal";
  const { replacements, prompts } = browser(url, "saved-token");
  assert.equal(readToken("Token:"), "saved-token");
  assert.equal(location.href, url);
  assert.deepEqual(replacements, []);
  assert.deepEqual(prompts, []);
});

test("manual credentials are saved and reused without another prompt", () => {
  const { prompts } = browser("https://example.com/");
  assert.equal(readToken("Enter token:"), "manual-token");
  assert.equal(readToken("Enter token:"), "manual-token");
  assert.equal(localStorage.getItem("octoterm-token"), "manual-token");
  assert.deepEqual(prompts, ["Enter token:"]);
});

test("cancellation and whitespace do not save an empty credential", () => {
  const { storage } = browser("https://example.com/");
  globalThis.prompt = () => null;
  assert.equal(readToken("Enter token:"), "");
  globalThis.prompt = () => "   ";
  assert.equal(readToken("Enter token:"), "");
  assert.equal(storage.has("octoterm-token"), false);
});

test("blocked storage does not discard a shared or manually entered token", () => {
  const { prompts } = browser("https://example.com/#token=shared-secret");
  globalThis.localStorage = {
    getItem() { throw new Error("blocked"); },
    setItem() { throw new Error("blocked"); },
  };
  assert.equal(readToken("Token:"), "shared-secret");
  assert.equal(location.hash, "");
  assert.deepEqual(prompts, []);
  assert.equal(readToken("Token:"), "manual-token");
  assert.deepEqual(prompts, ["Token:"]);
});

test("an empty saved token prompts instead of authenticating with an empty string", () => {
  const { prompts } = browser("https://example.com/", "");
  assert.equal(readToken("Token:"), "manual-token");
  assert.deepEqual(prompts, ["Token:"]);
});
