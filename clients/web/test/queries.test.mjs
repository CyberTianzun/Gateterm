import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { execSync } from "node:child_process";

// queries.ts 对 xterm 只有类型依赖,打包后是零依赖的纯函数
const here = dirname(fileURLToPath(import.meta.url));
execSync("npx esbuild src/queries.ts --bundle --format=esm --outfile=test/.queries.build.mjs", {
  cwd: join(here, ".."),
});
const { swallowTerminalQueries, termColors } = await import("./.queries.build.mjs");

test("termColors normalizes theme colors to #rrggbb", () => {
  assert.deepEqual(termColors({ foreground: "#D4D4D4", background: "#191a1bcc" }), {
    fg: "#d4d4d4",
    bg: "#191a1b",
  });
  assert.deepEqual(termColors({ foreground: "#abc", background: "#1234" }), {
    fg: "#aabbcc",
    bg: "#112233",
  });
});

test("termColors falls back to xterm defaults", () => {
  assert.deepEqual(termColors(undefined), { fg: "#ffffff", bg: "#000000" });
  assert.deepEqual(termColors({ background: "rgb(1,2,3)" }), { fg: "#ffffff", bg: "#000000" });
});

function fakeTerminal() {
  const csi = [];
  const dcs = [];
  const osc = new Map();
  const parser = {
    registerCsiHandler: (id, cb) => csi.push([id, cb]),
    registerDcsHandler: (id, cb) => dcs.push([id, cb]),
    registerOscHandler: (ident, cb) => osc.set(ident, cb),
  };
  return { term: { parser }, csi, dcs, osc };
}

test("queries are swallowed so xterm never answers them", () => {
  const { term, csi, dcs, osc } = fakeTerminal();
  swallowTerminalQueries(term);
  const key = ({ prefix = "", intermediates = "", final }) => prefix + intermediates + final;
  const csiKeys = csi.map(([id]) => key(id));
  for (const k of ["c", ">c", "=c", "n", "?n", "$p", "?$p", ">q"]) assert.ok(csiKeys.includes(k), k);
  assert.ok(csi.every(([, cb]) => cb() === true));
  assert.deepEqual(dcs.map(([id]) => key(id)), ["$q"]);
  for (const ident of [4, 10, 11, 12]) assert.ok(osc.has(ident), `osc ${ident}`);
});

test("color sets still reach xterm, only color queries are swallowed", () => {
  const { term, osc } = fakeTerminal();
  swallowTerminalQueries(term);
  assert.equal(osc.get(11)("?"), true);
  assert.equal(osc.get(4)("1;?"), true);
  assert.equal(osc.get(11)("#ffffff"), false);
  assert.equal(osc.get(4)("1;rgb:ff/00/00"), false);
});
