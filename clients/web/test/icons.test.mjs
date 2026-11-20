import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../src");

async function loadModule(relPath) {
  const result = await build({
    entryPoints: [join(srcDir, relPath)],
    bundle: true,
    format: "esm",
    write: false,
  });
  return import(
    `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`
  );
}

function makeSimpleDom(withNS = true) {
  class FakeEl {
    constructor(tag, ns = null) {
      this.tagName = tag.toUpperCase();
      this.namespaceURI = ns;
      this.attributes = new Map();
      this.childNodes = [];
      this.listeners = new Map();
      this._text = "";
      this.hidden = false;
      this.href = "";
      this.target = "";
      this.rel = "";
      this.title = "";
      this.type = "";
    }
    set className(v) {
      this._className = String(v);
      this.attributes.set("class", this._className);
    }
    get className() {
      return this._className ?? "";
    }
    set textContent(v) {
      this._text = String(v);
      this.childNodes = [];
    }
    get textContent() {
      return this._text + this.childNodes.map((c) => c.textContent).join("");
    }
    setAttribute(k, v) {
      this.attributes.set(k, String(v));
    }
    getAttribute(k) {
      return this.attributes.get(k) ?? null;
    }
    appendChild(child) {
      this.childNodes.push(child);
      return child;
    }
    append(...nodes) {
      for (const n of nodes) {
        this.appendChild(typeof n === "string" ? doc.createTextNode(n) : n);
      }
    }
    replaceChildren(...nodes) {
      this.childNodes = [];
      this.append(...nodes);
    }
    addEventListener(type, fn) {
      if (!this.listeners.has(type)) this.listeners.set(type, []);
      this.listeners.get(type).push(fn);
    }
    querySelector(selector) {
      for (const c of this.childNodes) {
        if (c.tagName === selector.toUpperCase()) return c;
        if (c.querySelector) {
          const hit = c.querySelector(selector);
          if (hit) return hit;
        }
      }
      return null;
    }
  }
  const doc = {
    visibilityState: "visible",
    createElement(tag) {
      return new FakeEl(tag, null);
    },
    createTextNode(text) {
      return { tagName: "#TEXT", textContent: String(text), childNodes: [] };
    },
    addEventListener() {},
    removeEventListener() {},
  };
  if (withNS) {
    doc.createElementNS = (ns, tag) => new FakeEl(tag, ns);
  }
  return doc;
}

const STATIC_HTML_ICON_BUTTONS = [
  ["open-history", "history"],
  ["back-to-list", "grid"],
  ["open-settings", "settings"],
  ["new-session", "plus"],
  ["menu", "menu"],
  ["sidebar-show", "sidebar-show"],
];

test("UI icon buttons in index.html use inline SVG matching icons.ts instead of Unicode glyphs", async () => {
  const { iconSvg } = await loadModule("icons.ts");
  const html = readFileSync(join(srcDir, "index.html"), "utf-8");

  for (const [id, iconName] of STATIC_HTML_ICON_BUTTONS) {
    const match = html.match(new RegExp(`<button[^>]*id="${id}"[^>]*>([\\s\\S]*?)</button>`));
    assert.ok(match, `missing button #${id} in index.html`);
    const inner = match[1].trim();
    assert.equal(inner, iconSvg(iconName), `button #${id} SVG drifted from icons.ts (${iconName})`);
  }
});

test("no Unicode symbol glyphs remain in web UI icon sources or i18n strings", () => {
  const files = ["index.html", "main.ts", "modal.ts", "chat-view.ts", "settings.ts", "update.ts", "i18n.ts"];
  const forbidden = /[⊞⚙✎✕☰↗✓⚠]/;

  for (const file of files) {
    const content = readFileSync(join(srcDir, file), "utf-8");
    const lines = content.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const trimmed = lines[i].trim();
      if (trimmed.startsWith("//") || trimmed.startsWith("*") || trimmed.startsWith("/*")) continue;
      const code = trimmed.replace(/(?<!:)\/\/.*$/, "");
      assert.equal(
        forbidden.test(code),
        false,
        `found OS-font-dependent Unicode UI symbol in ${file}:${i + 1}: ${code}`,
      );
    }
  }
});

test("icons.ts generates consistent accessible SVG markup and DOM nodes", async (t) => {
  const { UI_ICON_NAMES, iconSvg, createIcon } = await loadModule("icons.ts");
  assert.ok(UI_ICON_NAMES.length >= 16);

  const prevDoc = globalThis.document;
  t.after(() => {
    globalThis.document = prevDoc;
  });

  for (const withNS of [true, false]) {
    globalThis.document = makeSimpleDom(withNS);
    for (const name of UI_ICON_NAMES) {
      const markup = iconSvg(name);
      assert.match(markup, /^<svg\b[^>]*viewBox="0 0 24 24"[^>]*aria-hidden="true"[^>]*>.*<\/svg>$/);
      assert.ok(markup.includes('stroke="currentColor"'), `${name} should inherit currentColor`);

      const el = createIcon(name, "inline-icon");
      assert.equal(el.tagName, "SVG");
      assert.equal(el.namespaceURI, withNS ? "http://www.w3.org/2000/svg" : null);
      assert.equal(el.getAttribute("viewBox"), "0 0 24 24");
      assert.equal(el.getAttribute("aria-hidden"), "true");
      assert.equal(el.getAttribute("focusable"), "false");
      assert.equal(el.getAttribute("class"), "inline-icon");
      assert.ok(el.childNodes.length > 0, `${name} should have SVG child shapes`);
    }
  }
});

test("createModal renders an SVG close button with accessible label and no Unicode X", async (t) => {
  const { createModal } = await loadModule("modal.ts");
  const prevDoc = globalThis.document;
  globalThis.document = makeSimpleDom(true);
  t.after(() => {
    globalThis.document = prevDoc;
  });

  const { header } = createModal("test-dialog", "Settings", "Close settings");
  const closeBtn = header.querySelector("button");
  assert.ok(closeBtn, "modal header should contain close button");
  assert.equal(closeBtn.getAttribute("aria-label"), "Close settings");
  assert.equal(closeBtn.title, "Close settings");
  assert.equal(closeBtn.textContent, "");
  const svg = closeBtn.querySelector("svg");
  assert.ok(svg, "close button should render an SVG icon");
  assert.equal(svg.getAttribute("aria-hidden"), "true");
});

test("mountUpdateStatus renders an external-link SVG icon and skips redundant DOM replacement", async (t) => {
  const { mountUpdateStatus } = await loadModule("update.ts");
  const prevDoc = globalThis.document;
  const prevWin = globalThis.window;
  const prevFetch = globalThis.fetch;
  const doc = makeSimpleDom(true);
  globalThis.document = doc;
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    json: async () => ({
      current_version: "0.2.3",
      checked_at: 100,
      update: {
        version: "0.3.0",
        url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v0.3.0",
      },
    }),
  });
  t.after(() => {
    globalThis.document = prevDoc;
    globalThis.window = prevWin;
    globalThis.fetch = prevFetch;
  });

  const host = doc.createElement("div");
  const ctrl = mountUpdateStatus(host, () => "tok");
  try {
    ctrl.setOnline(true);
    await new Promise((r) => setTimeout(r, 20));
    assert.equal(host.hidden, false);
    const link = host.querySelector("a");
    assert.ok(link, "update link should be rendered");
    assert.equal(link.textContent.includes("↗"), false);
    assert.ok(link.textContent.includes("v0.3.0"));
    const svg = link.querySelector("svg");
    assert.ok(svg, "update link should append an inline SVG icon");
    assert.equal(svg.getAttribute("class"), "inline-icon");
  } finally {
    ctrl.dispose();
  }
});
