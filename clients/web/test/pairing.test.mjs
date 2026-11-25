import { test, after, afterEach } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { execSync } from "node:child_process";
import { rmSync } from "node:fs";
import { build } from "esbuild";

const here = dirname(fileURLToPath(import.meta.url));
const buildOutfile = join(here, ".pairing.build.mjs");

after(() => {
  rmSync(buildOutfile, { force: true });
});

afterEach(() => {
  delete globalThis.document;
});

execSync("npx esbuild src/pairing.ts --bundle --format=esm --outfile=test/.pairing.build.mjs", {
  cwd: join(here, ".."),
});

const {
  buildPairingTarget,
  buildQrSvgModel,
  renderQrSvg,
  createQrSvgElement,
} = await import("./.pairing.build.mjs");

const mcpBundle = await build({
  entryPoints: [fileURLToPath(new URL("../src/mcp.ts", import.meta.url))],
  bundle: true,
  format: "esm",
  write: false,
});
const { extractTitlePrefix } = await import(
  `data:text/javascript;base64,${Buffer.from(mcpBundle.outputFiles[0].text).toString("base64")}`
);

test("buildPairingTarget accepts LAN IPv4, IPv6, HTTPS domain, and reverse-proxy subpaths", () => {
  assert.deepEqual(buildPairingTarget("http://192.168.1.20:7683/", "tok123"), {
    ok: true,
    url: "http://192.168.1.20:7683/#token=tok123",
    displayUrl: "http://192.168.1.20:7683",
  });

  assert.deepEqual(buildPairingTarget("http://[fd00::2]:7683/", "tok123"), {
    ok: true,
    url: "http://[fd00::2]:7683/#token=tok123",
    displayUrl: "http://[fd00::2]:7683",
  });

  assert.deepEqual(buildPairingTarget("https://term.example.com/", "tok123"), {
    ok: true,
    url: "https://term.example.com/#token=tok123",
    displayUrl: "https://term.example.com",
  });

  assert.deepEqual(
    buildPairingTarget("https://term.example.com/gateterm/index.html?x=1#old", "tok123"),
    {
      ok: true,
      url: "https://term.example.com/gateterm/#token=tok123",
      displayUrl: "https://term.example.com/gateterm",
    },
  );
});

test("buildPairingTarget round-trips special and Unicode tokens via URLSearchParams", () => {
  const token = "a+b &=#密码";
  const res = buildPairingTarget("https://term.example.com/gateterm/", token);
  assert.equal(res.ok, true);
  assert.equal(res.displayUrl, "https://term.example.com/gateterm");

  const parsed = new URL(res.url);
  assert.equal(parsed.origin + parsed.pathname, "https://term.example.com/gateterm/");
  const fragmentParams = new URLSearchParams(parsed.hash.slice(1));
  assert.equal(fragmentParams.get("token"), token);
});

test("buildPairingTarget omits title when undefined, null, empty, or whitespace/newlines only", () => {
  for (const title of [undefined, null, "", "  \r\n  "]) {
    assert.deepEqual(
      buildPairingTarget("https://term.example.com/gateterm/", "tok123", title),
      {
        ok: true,
        url: "https://term.example.com/gateterm/#token=tok123",
        displayUrl: "https://term.example.com/gateterm",
      },
    );
  }
});

test("buildPairingTarget normalizes and round-trips special/Unicode titles via URLSearchParams", () => {
  const res = buildPairingTarget(
    "https://term.example.com/gateterm/",
    "a+b &=#密码",
    "  研发\r\n1号+a&b=#测试  ",
  );
  assert.equal(res.ok, true);
  assert.equal(res.displayUrl, "https://term.example.com/gateterm");

  const fragmentParams = new URLSearchParams(new URL(res.url).hash.slice(1));
  assert.equal(fragmentParams.get("token"), "a+b &=#密码");
  assert.equal(fragmentParams.get("title"), "研发 1号+a&b=#测试");
});

test("extractTitlePrefix + buildPairingTarget integration includes configured title and omits default titles", () => {
  assert.deepEqual(
    buildPairingTarget(
      "https://term.example.com/",
      "tok",
      extractTitlePrefix("(3) prod-01 - gateterm"),
    ),
    {
      ok: true,
      url: "https://term.example.com/#token=tok&title=prod-01",
      displayUrl: "https://term.example.com",
    },
  );

  for (const docTitle of ["(2) gateterm", "gateterm"]) {
    assert.deepEqual(
      buildPairingTarget("https://term.example.com/", "tok", extractTitlePrefix(docTitle)),
      {
        ok: true,
        url: "https://term.example.com/#token=tok",
        displayUrl: "https://term.example.com",
      },
    );
  }
});

test("buildPairingTarget rejects loopback and wildcard hosts", () => {
  const cases = [
    "http://localhost:7683/",
    "http://LOCALHOST:7683/",
    "http://127.0.0.1:7683/",
    "http://127.4.3.2:7683/",
    "http://[::1]:7683/",
    "http://[::ffff:127.0.0.1]:7683/",
    "http://0.0.0.0:7683/",
    "http://[::]:7683/",
  ];

  for (const href of cases) {
    const res = buildPairingTarget(href, "tok123");
    assert.equal(res.ok, false, `expected ${href} to be rejected`);
    assert.equal(res.reason, "loopback", `expected loopback reason for ${href}`);
    assert.equal(typeof res.host, "string");
    assert.ok(res.host.length > 0, `expected non-empty host for ${href}`);
  }
});

test("buildPairingTarget rejects unsupported schemes, userinfo, blank tokens, and malformed URLs", () => {
  assert.deepEqual(buildPairingTarget("file:///tmp/index.html", "tok123"), {
    ok: false,
    reason: "unsupported-scheme",
  });
  assert.deepEqual(buildPairingTarget("ftp://192.168.1.2/file", "tok123"), {
    ok: false,
    reason: "unsupported-scheme",
  });

  assert.deepEqual(buildPairingTarget("http://user:pass@192.168.1.2:7683/", "tok123"), {
    ok: false,
    reason: "userinfo-not-allowed",
  });
  assert.deepEqual(buildPairingTarget("http://user@192.168.1.2:7683/", "tok123"), {
    ok: false,
    reason: "userinfo-not-allowed",
  });

  assert.deepEqual(buildPairingTarget("http://192.168.1.2:7683/", ""), {
    ok: false,
    reason: "missing-token",
  });
  assert.deepEqual(buildPairingTarget("http://192.168.1.2:7683/", "   \t\n "), {
    ok: false,
    reason: "missing-token",
  });

  assert.deepEqual(buildPairingTarget("not a url", "tok123"), {
    ok: false,
    reason: "invalid-url",
  });
  assert.deepEqual(buildPairingTarget("", "tok123"), {
    ok: false,
    reason: "invalid-url",
  });
});

test("buildQrSvgModel & renderQrSvg enforce 4-module quiet zone and valid SVG structure", () => {
  const model = buildQrSvgModel("https://term.example.com/#token=abc");
  assert.equal(model.ok, true);
  assert.equal(model.quietZone, 4);
  assert.equal(model.viewBoxSize, model.moduleCount + 8);
  assert.ok(model.pathData.length > 0);

  const coords = [...model.pathData.matchAll(/M(\d+) (\d+)h1v1h-1z/g)];
  assert.ok(coords.length > 0);
  for (const [, xStr, yStr] of coords) {
    const x = Number(xStr);
    const y = Number(yStr);
    assert.ok(x >= 4 && x < model.moduleCount + 4, `x=${x} out of quiet-zone bounds`);
    assert.ok(y >= 4 && y < model.moduleCount + 4, `y=${y} out of quiet-zone bounds`);
  }

  const svgRes = renderQrSvg("https://term.example.com/#token=abc");
  assert.equal(svgRes.ok, true);
  assert.ok(svgRes.svg.includes(`<svg `));
  assert.ok(svgRes.svg.includes(`viewBox="0 0 ${model.viewBoxSize} ${model.viewBoxSize}"`));
  assert.ok(svgRes.svg.includes(`fill="#ffffff"`));
  assert.ok(svgRes.svg.includes(`fill="#000000"`));
  assert.ok(svgRes.svg.includes(model.pathData));
});

test("buildQrSvgModel encodes UTF-8 multi-byte characters accurately without Latin-1 truncation", () => {
  const unicodeQr = buildQrSvgModel("密码");
  const truncatedQr = buildQrSvgModel("\u00c6\u0001");
  assert.equal(unicodeQr.ok, true);
  assert.equal(truncatedQr.ok, true);
  assert.notEqual(unicodeQr.pathData, truncatedQr.pathData);

  const mixedQr = buildQrSvgModel("https://term.example.com/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81");
  assert.equal(mixedQr.ok, true);
});

test("buildQrSvgModel & renderQrSvg return too-long for >8192 chars and oversize UTF-8 payloads", () => {
  const over8192 = "a".repeat(8193);
  assert.deepEqual(buildQrSvgModel(over8192), { ok: false, reason: "too-long" });
  assert.deepEqual(renderQrSvg(over8192), { ok: false, reason: "too-long" });

  // 1000 CJK characters = 3000 UTF-8 bytes, which exceeds QR v40 EC-M capacity (2331 bytes)
  // even though string.length (1000) is well under 8192.
  const oversizeUtf8 = "密".repeat(1000);
  assert.deepEqual(buildQrSvgModel(oversizeUtf8), { ok: false, reason: "too-long" });
  assert.deepEqual(renderQrSvg(oversizeUtf8), { ok: false, reason: "too-long" });
});

test("createQrSvgElement constructs SVG DOM safely with createElementNS", () => {
  const created = [];
  const makeNode = (ns, tag) => {
    const attrs = new Map();
    const children = [];
    const node = {
      ns,
      tag,
      attrs,
      children,
      setAttribute(k, v) {
        attrs.set(k, String(v));
      },
      append(...nodes) {
        children.push(...nodes);
      },
    };
    created.push(node);
    return node;
  };

  globalThis.document = {
    createElementNS(ns, tag) {
      return makeNode(ns, tag);
    },
  };

  const model = buildQrSvgModel("https://term.example.com/#token=abc");
  assert.equal(model.ok, true);

  const svg = createQrSvgElement(model, "Pairing QR code");
  assert.equal(svg.ns, "http://www.w3.org/2000/svg");
  assert.equal(svg.tag, "svg");
  assert.equal(svg.attrs.get("viewBox"), `0 0 ${model.viewBoxSize} ${model.viewBoxSize}`);
  assert.equal(svg.attrs.get("role"), "img");
  assert.equal(svg.attrs.get("aria-label"), "Pairing QR code");
  assert.equal(svg.attrs.get("shape-rendering"), "crispEdges");

  assert.equal(svg.children.length, 2);
  const [rect, path] = svg.children;
  assert.equal(rect.ns, "http://www.w3.org/2000/svg");
  assert.equal(rect.tag, "rect");
  assert.equal(rect.attrs.get("width"), String(model.viewBoxSize));
  assert.equal(rect.attrs.get("height"), String(model.viewBoxSize));
  assert.equal(rect.attrs.get("fill"), "#ffffff");

  assert.equal(path.ns, "http://www.w3.org/2000/svg");
  assert.equal(path.tag, "path");
  assert.equal(path.attrs.get("d"), model.pathData);
  assert.equal(path.attrs.get("fill"), "#000000");
});
