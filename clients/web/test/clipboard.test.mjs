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

function installDom({
  isSecureContext,
  writeText,
  allowExecCopy = true,
  requireSelectionForExecCopy = false,
  throwOnExecCopy = false,
}) {
  const listeners = new Map();
  let copiedViaExec = null;
  let copiedViaSelection = null;
  const createdTextareas = [];
  const bodyChildren = [];
  const focusRestoreCalls = [];
  const previousActiveElement = {
    focus(opts) {
      focusRestoreCalls.push(opts ?? null);
      globalThis.document.activeElement = previousActiveElement;
    },
  };

  globalThis.window = { isSecureContext };
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: writeText ? { clipboard: { writeText } } : {},
  });
  globalThis.document = {
    activeElement: previousActiveElement,
    body: {
      appendChild(node) {
        bodyChildren.push(node);
        node.parentNode = globalThis.document.body;
        return node;
      },
      removeChild(node) {
        const idx = bodyChildren.indexOf(node);
        if (idx >= 0) bodyChildren.splice(idx, 1);
        node.parentNode = null;
      },
    },
    createElement(tag) {
      assert.equal(tag, "textarea");
      const attrs = new Map();
      const el = {
        tagName: "TEXTAREA",
        value: "",
        readOnly: false,
        style: {},
        parentNode: null,
        selected: false,
        focusCalls: [],
        selectCalls: 0,
        selectionRange: null,
        removed: false,
        setAttribute(k, v) {
          attrs.set(k, String(v));
          if (k === "readonly") el.readOnly = true;
        },
        getAttribute(k) {
          return attrs.get(k) ?? null;
        },
        focus(opts) {
          el.focusCalls.push(opts ?? null);
          globalThis.document.activeElement = el;
        },
        select() {
          el.selectCalls += 1;
          el.selected = true;
        },
        setSelectionRange(start, end) {
          el.selectionRange = [start, end];
        },
        remove() {
          el.removed = true;
          const idx = bodyChildren.indexOf(el);
          if (idx >= 0) bodyChildren.splice(idx, 1);
          el.parentNode = null;
        },
      };
      createdTextareas.push(el);
      return el;
    },
    addEventListener(type, fn) {
      listeners.set(type, fn);
    },
    removeEventListener(type, fn) {
      if (listeners.get(type) === fn) listeners.delete(type);
    },
    execCommand(cmd) {
      assert.equal(cmd, "copy");
      if (throwOnExecCopy) throw new Error("execCommand failed");
      if (!allowExecCopy) return false;
      if (requireSelectionForExecCopy) {
        const selected = bodyChildren.find((el) => el.selected);
        if (!selected) return false;
        copiedViaSelection = selected.value;
        return true;
      }
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
  return {
    getExecCopied: () => copiedViaExec,
    getSelectionCopied: () => copiedViaSelection,
    getCreatedTextareas: () => createdTextareas,
    getBodyChildCount: () => bodyChildren.length,
    getFocusRestoreCalls: () => focusRestoreCalls,
    hasListeners: () => listeners.size > 0,
  };
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
  assert.equal(dom.getCreatedTextareas().length, 0);
});

test("writeClipboard falls back to execCommand('copy') on plain HTTP LAN origins or rejected Clipboard API", async () => {
  const httpDom = installDom({ isSecureContext: false });
  assert.equal(await writeClipboard("lan copy"), true);
  assert.equal(httpDom.getExecCopied(), "lan copy");
  assert.equal(httpDom.hasListeners(), false);
  assert.equal(httpDom.getCreatedTextareas().length, 0);

  const rejectedDom = installDom({
    isSecureContext: true,
    writeText: async () => {
      throw new Error("NotAllowedError");
    },
  });
  assert.equal(await writeClipboard("fallback copy"), true);
  assert.equal(rejectedDom.getExecCopied(), "fallback copy");
  assert.equal(rejectedDom.hasListeners(), false);
  assert.equal(rejectedDom.getCreatedTextareas().length, 0);
});

test("writeClipboard falls back to offscreen textarea selection and restores focus when direct execCommand fails", async () => {
  const dom = installDom({
    isSecureContext: false,
    requireSelectionForExecCopy: true,
  });
  assert.equal(await writeClipboard("selection copy"), true);
  assert.equal(dom.getExecCopied(), null);
  assert.equal(dom.getSelectionCopied(), "selection copy");
  assert.equal(dom.getCreatedTextareas().length, 1);
  const ta = dom.getCreatedTextareas()[0];
  assert.equal(ta.readOnly, true);
  assert.equal(ta.getAttribute("readonly"), "");
  assert.equal(ta.style.position, "fixed");
  assert.equal(ta.style.left, "-9999px");
  assert.equal(ta.style.top, "0");
  assert.equal(ta.style.opacity, "0");
  assert.equal(ta.value, "selection copy");
  assert.equal(ta.selectCalls, 1);
  assert.deepEqual(ta.selectionRange, [0, "selection copy".length]);
  assert.equal(ta.removed, true);
  assert.equal(dom.getBodyChildCount(), 0);
  assert.equal(dom.getFocusRestoreCalls().length, 1);
  assert.deepEqual(dom.getFocusRestoreCalls()[0], { preventScroll: true });
  assert.equal(dom.hasListeners(), false);
});

test("writeClipboard returns false without throwing when both stages are blocked", async () => {
  const blockedDom = installDom({
    isSecureContext: false,
    allowExecCopy: false,
  });
  assert.equal(await writeClipboard("blocked copy"), false);
  assert.equal(blockedDom.getExecCopied(), null);
  assert.equal(blockedDom.getSelectionCopied(), null);
  assert.equal(blockedDom.getBodyChildCount(), 0);
  assert.equal(blockedDom.getFocusRestoreCalls().length, 1);
  assert.equal(blockedDom.hasListeners(), false);

  const throwingDom = installDom({
    isSecureContext: false,
    throwOnExecCopy: true,
  });
  assert.equal(await writeClipboard("throwing copy"), false);
  assert.equal(throwingDom.getBodyChildCount(), 0);
  assert.equal(throwingDom.getFocusRestoreCalls().length, 1);
  assert.equal(throwingDom.hasListeners(), false);

  const noBodyDom = installDom({
    isSecureContext: false,
    allowExecCopy: false,
  });
  globalThis.document.body = null;
  assert.equal(await writeClipboard("no body copy"), false);
  assert.equal(noBodyDom.getCreatedTextareas().length, 0);
  assert.equal(noBodyDom.hasListeners(), false);
});
