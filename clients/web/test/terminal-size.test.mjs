import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const result = await build({
  entryPoints: [fileURLToPath(new URL("../src/terminal-size.ts", import.meta.url))],
  bundle: true, format: "esm", write: false,
});
const { sanitizeDimensions, createRefitScheduler } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`
);

test("sanitizeDimensions rejects invalid, zero, negative and non-finite inputs and floors valid ones", () => {
  assert.equal(sanitizeDimensions(null), null);
  assert.equal(sanitizeDimensions(undefined), null);
  assert.equal(sanitizeDimensions({ cols: 0, rows: 24 }), null);
  assert.equal(sanitizeDimensions({ cols: 80, rows: 0 }), null);
  assert.equal(sanitizeDimensions({ cols: -5, rows: 24 }), null);
  assert.equal(sanitizeDimensions({ cols: Number.NaN, rows: 24 }), null);
  assert.equal(sanitizeDimensions({ cols: Number.POSITIVE_INFINITY, rows: 24 }), null);
  assert.equal(sanitizeDimensions({ cols: 0.9, rows: 24 }), null);
  assert.deepEqual(sanitizeDimensions({ cols: 120.9, rows: 40.2 }), { cols: 120, rows: 40 });
});

test("createRefitScheduler coalesces bursty triggers into one animation frame and supports cancel", () => {
  const frames = new Map();
  let nextId = 1;
  let runs = 0;
  const sched = createRefitScheduler({
    scheduleFrame: (cb) => { const id = nextId++; frames.set(id, cb); return id; },
    cancelFrame: (id) => { frames.delete(id); },
    run: () => { runs++; },
  });
  sched.schedule();
  sched.schedule();
  sched.schedule();
  assert.equal(frames.size, 1);
  assert.equal(runs, 0);
  const [firstId, firstCb] = [...frames.entries()][0];
  frames.delete(firstId);
  firstCb();
  assert.equal(runs, 1);
  // Scheduling again after execution queues a new frame; cancel aborts it.
  sched.schedule();
  assert.equal(frames.size, 1);
  sched.cancel();
  assert.equal(frames.size, 0);
  assert.equal(runs, 1);
});

test("createRefitScheduler ignores stale callback if cancelFrame is omitted", () => {
  const callbacks = [];
  let runs = 0;
  const sched = createRefitScheduler({
    scheduleFrame: (cb) => { callbacks.push(cb); return callbacks.length; },
    run: () => { runs++; },
  });
  sched.schedule();
  sched.cancel();
  callbacks.shift()();
  assert.equal(runs, 0);
});
