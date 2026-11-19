import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const result = await build({
  entryPoints: [fileURLToPath(new URL("../src/clipboard.ts", import.meta.url))],
  bundle: true,
  format: "esm",
  write: false,
});
const { writeClipboard } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`
);

function installDom({ isSecureContext, writeText, allowExecCopy = true }) {
  const listeners = new Map();
  let copiedViaExec = null;
  globalThis.window = { isSecureContext };
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: writeText ? { clipboard: { writeText } } : {},
  });
  globalThis.document = {
    addEventListener(type, fn) {
      listeners.set(type, fn);
    },
    removeEventListener(type, fn) {
      if (listeners.get(type) === fn) listeners.delete(type);
    },
    execCommand(cmd) {
      assert.equal(cmd, "copy");
      if (!allowExecCopy) return false;
      const handler = listeners.get("copy");
      if (handler) {
        handler({
          clipboardData: {
            setData(mime, val) {
              assert.equal(mime, "text/plain");
              copiedViaExec = val;
            },
          },
          preventDefault() {},
        });
      }
      return true;
    },
  };
  return { getExecCopied: () => copiedViaExec, hasListeners: () => listeners.size > 0 };
}

test("writeClipboard uses navigator.clipboard in secure contexts", async () => {
  let written = null;
  const dom = installDom({
    isSecureContext: true,
    writeText: async (text) => {
      written = text;
    },
  });
  assert.equal(await writeClipboard("hello secure"), true);
  assert.equal(written, "hello secure");
  assert.equal(dom.getExecCopied(), null);
});

test("writeClipboard falls back to execCommand('copy') on plain HTTP LAN origins or rejected Clipboard API", async () => {
  const httpDom = installDom({ isSecureContext: false });
  assert.equal(await writeClipboard("lan copy"), true);
  assert.equal(httpDom.getExecCopied(), "lan copy");
  assert.equal(httpDom.hasListeners(), false);

  const rejectedDom = installDom({
    isSecureContext: true,
    writeText: async () => {
      throw new Error("NotAllowedError");
    },
  });
  assert.equal(await writeClipboard("fallback copy"), true);
  assert.equal(rejectedDom.getExecCopied(), "fallback copy");
  assert.equal(rejectedDom.hasListeners(), false);
});
