import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const result = await build({
  entryPoints: [fileURLToPath(new URL("../src/client.ts", import.meta.url))],
  bundle: true, format: "esm", write: false,
});
const { GateClient } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`);
const encoder = new TextEncoder();
const decoder = new TextDecoder();
const tick = () => new Promise(resolve => setImmediate(resolve));
const colors = { fg: "#ffffff", bg: "#000000" };
function frame(channel, body) {
  const payload = encoder.encode(channel ? body : JSON.stringify(body));
  const data = new Uint8Array(5 + payload.length);
  new DataView(data.buffer).setUint32(0, channel, true);
  data.set(payload, 5);
  return data.buffer;
}
function setup() {
  let socket;
  const sockets = [];
  globalThis.WebSocket = class {
    sent = [];
    constructor() { socket = this; sockets.push(this); }
    send(data) { this.sent.push(JSON.parse(decoder.decode(new Uint8Array(data).slice(5)))); }
    emit(message, channel = 0) { this.onmessage({ data: frame(channel, message) }); }
    close() {}
  };
  const client = new GateClient();
  client.connect("ws://test", "token");
  client.attach(10, 1, 80, 24, colors);
  socket.emit({ type: "attached", channel: 1, mode: "resync", seq: 0 });
  return { client, socket, sockets };
}
afterEach(() => { delete globalThis.WebSocket; });

test("resize, reset and resync anchors wait for the preceding write to apply", async () => {
  const { client, socket } = setup();
  let apply;
  const events = [];
  client.onChannelData = () => new Promise(resolve => { apply = () => { events.push("write"); resolve(); }; });
  client.onControl = msg => events.push(msg.type);
  await tick();
  events.length = 0;
  socket.emit("old output", 1);
  socket.emit({ type: "resized", channel: 1, cols: 40, rows: 12 });
  socket.emit({ type: "resync-begin", channel: 1 });
  socket.emit({ type: "resync-end", channel: 1, seq: 100 });
  await tick();
  assert.deepEqual(events, []);
  apply();
  await tick();
  assert.deepEqual(events, ["write", "resized", "resync-begin", "resync-end"]);
});

test("reconnect advertises only applied bytes and waits for a pending write", async () => {
  const { client, socket } = setup();
  socket.emit({ type: "resync-end", channel: 1, seq: 100 });
  await tick();
  let apply;
  client.onChannelData = () => new Promise(resolve => { apply = resolve; });
  socket.emit("abc", 1);
  socket.emit({ type: "hello-ok", proto: 1 });
  await tick();
  assert.equal(socket.sent.length, 1);
  apply();
  await tick();
  assert.equal(socket.sent.at(-1).last_seq, 103);
});

test("snapshot bytes never advance the live sequence before its resync anchor", async () => {
  const { client, socket } = setup();
  socket.emit({ type: "resync-end", channel: 1, seq: 100 });
  socket.emit({ type: "resync-begin", channel: 1 });
  socket.emit("synthetic snapshot", 1);
  socket.emit({ type: "hello-ok", proto: 1 });
  await tick();
  assert.equal(socket.sent.at(-1).last_seq, null);
});

test("an asynchronous control cannot anchor a replacement attachment on the same channel", async () => {
  const { client, socket } = setup();
  await tick();
  let apply;
  client.onControl = msg => msg.type === "resync-end"
    ? new Promise(resolve => { apply = resolve; }) : undefined;
  socket.emit({ type: "resync-end", channel: 1, seq: 200 });
  await tick();
  client.detach(1);
  client.attach(20, 1, 80, 24, colors);
  apply();
  await tick();
  socket.emit({ type: "hello-ok", proto: 1 });
  await tick();
  assert.equal(socket.sent.at(-1).last_seq, null);
});

test("detached channels cannot deliver stale output or channel controls", async () => {
  const { client, socket } = setup();
  await tick();
  const events = [];
  client.onChannelData = () => events.push("data");
  client.onControl = msg => events.push(msg.type);
  client.detach(1);
  client.attach(20, 2, 80, 24, colors);
  socket.emit("old", 1);
  socket.emit({ type: "resized", channel: 1, cols: 1, rows: 1 });
  socket.emit({ type: "resync-begin", channel: 1 });
  socket.emit({ type: "session-exited", channel: 1, id: 10 });
  socket.emit({ type: "error", channel: 1, message: "old attach failed" });
  socket.emit({ type: "attached", channel: 2, mode: "resync", seq: 0 });
  socket.emit("new", 2);
  await tick();
  assert.deepEqual(events, ["attached", "data"]);
});

test("reconnect drains applied output but ignores an obsolete queued handshake", async context => {
  const { client, socket, sockets } = setup();
  socket.emit({ type: "resync-end", channel: 1, seq: 100 });
  await tick();
  let apply;
  client.onChannelData = () => new Promise(resolve => { apply = resolve; });
  socket.emit("abc", 1);
  await tick();
  socket.emit({ type: "hello-ok", proto: 1 });
  let reconnect;
  context.mock.method(globalThis, "setTimeout", callback => { reconnect = callback; });
  socket.onclose();
  reconnect();
  const replacement = sockets[1];
  replacement.emit({ type: "hello-ok", proto: 1 });
  socket.emit("stale", 1);
  await tick();
  assert.deepEqual(replacement.sent, []);
  apply();
  await tick();
  assert.equal(replacement.sent.length, 1);
  assert.equal(replacement.sent[0].last_seq, 103);
});

test("invalid credentials stop reconnecting while an old terminal write is pending", async context => {
  const { client, socket, sockets } = setup();
  socket.emit({ type: "hello-ok", proto: 1 });
  await tick();
  let apply;
  client.onChannelData = () => new Promise(resolve => { apply = resolve; });
  socket.emit("pending output", 1);
  await tick();
  const retries = [];
  const fatal = [];
  const removed = [];
  context.mock.method(globalThis, "setTimeout", callback => { retries.push(callback); });
  globalThis.localStorage = { removeItem: key => removed.push(key) };
  context.after(() => { delete globalThis.localStorage; });
  client.onFatal = message => fatal.push(message);
  socket.onclose();
  retries.shift()();
  sockets[1].emit({ type: "error", message: "invalid token" });
  sockets[1].onclose();
  assert.deepEqual(fatal, ["invalid token"]);
  assert.deepEqual(removed, ["gateterm-token"]);
  assert.equal(retries.length, 0);
  apply();
  await tick();
  assert.equal(retries.length, 0);
});

test("queued errors from an old socket cannot poison a new authentication attempt", async context => {
  const { client, socket, sockets } = setup();
  socket.emit({ type: "hello-ok", proto: 1 });
  await tick();
  let apply;
  client.onChannelData = () => new Promise(resolve => { apply = resolve; });
  socket.emit("pending output", 1);
  await tick();
  socket.emit({ type: "error", message: "old request failed" });
  const retries = [];
  const fatal = [];
  context.mock.method(globalThis, "setTimeout", callback => { retries.push(callback); });
  client.onFatal = message => fatal.push(message);
  socket.onclose();
  retries.shift()();
  apply();
  await tick();
  sockets[1].onclose();
  assert.deepEqual(fatal, []);
  assert.equal(retries.length, 1);
});

test("a received hello prevents ordinary errors being mistaken for authentication failure", async context => {
  const { client, socket, sockets } = setup();
  socket.emit({ type: "hello-ok", proto: 1 });
  await tick();
  let apply;
  client.onChannelData = () => new Promise(resolve => { apply = resolve; });
  socket.emit("pending output", 1);
  await tick();
  const retries = [];
  const fatal = [];
  context.mock.method(globalThis, "setTimeout", callback => { retries.push(callback); });
  client.onFatal = message => fatal.push(message);
  socket.onclose();
  retries.shift()();
  sockets[1].emit({ type: "hello-ok", proto: 1 });
  sockets[1].emit({ type: "error", message: "request failed" });
  sockets[1].onclose();
  assert.deepEqual(fatal, []);
  assert.equal(retries.length, 1);
  apply();
  await tick();
  assert.deepEqual(sockets[1].sent, []);
});
