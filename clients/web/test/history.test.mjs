import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";

execSync("npx esbuild src/history.ts --bundle --format=esm --outfile=test/.history.build.mjs", {
  cwd: fileURLToPath(new URL("..", import.meta.url)),
});
const { fetchHistory, fetchHistoryMessages, openHistory, HistoryError, historyError } = await import("./.history.build.mjs");
const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });
const entry = { agent_id: "codex", agent_session_id: "known", cwd: "/untrusted", command: ["untrusted"] };

test("history filters and pagination are encoded; the bearer token stays in the header", async () => {
  globalThis.fetch = async (url, init) => {
    const parsed = new URL(url, "http://localhost");
    assert.equal(parsed.searchParams.get("q"), "项目 & title");
    assert.equal(parsed.searchParams.get("agent_id"), "codex");
    assert.equal(parsed.searchParams.get("offset"), "50");
    assert.equal(parsed.searchParams.get("refresh"), "true");
    assert.equal(init.headers.Authorization, "Bearer secret");
    assert.ok(!url.includes("secret"));
    return { ok: true, json: async () => ({ sessions: [], total: 0 }) };
  };
  assert.equal((await fetchHistory("secret", "项目 & title", "codex", 50, true)).total, 0);
});

test("resume submits only the server-owned identity, never a command or directory", async () => {
  globalThis.fetch = async (url, init) => {
    assert.equal(url, "/api/agents/history/open");
    assert.equal(init.method, "POST");
    assert.deepEqual(JSON.parse(init.body), { agent_id: "codex", agent_session_id: "known" });
    return { ok: true, json: async () => ({ session: { id: 7 }, reused: true }) };
  };
  assert.equal((await openHistory("secret", entry)).session.id, 7);
});

test("preview also resolves by identity and does not send a transcript path", async () => {
  globalThis.fetch = async (url) => {
    const parsed = new URL(url, "http://localhost");
    assert.equal(parsed.pathname, "/api/agents/history/messages");
    assert.deepEqual([...parsed.searchParams], [["agent_id", "codex"], ["agent_session_id", "known"]]);
    return { ok: true, json: async () => ({ source: "transcript", messages: [] }) };
  };
  assert.equal((await fetchHistoryMessages("t", entry)).source, "transcript");
});

test("old servers, disconnections and typed failures produce readable errors", async () => {
  globalThis.fetch = async () => ({ ok: false, json: async () => ({ error: "missing-directory" }) });
  await assert.rejects(openHistory("t", entry), (e) => e instanceof HistoryError && e.reason === "missing-directory");
  globalThis.fetch = async () => { throw Error("offline"); };
  await assert.rejects(fetchHistory("t"), (e) => e.reason === "unreadable");
  globalThis.fetch = async () => ({ ok: true, json: async () => ({ source: "terminal", reason: "unreadable" }) });
  await assert.rejects(fetchHistoryMessages("t", entry), (e) => e.reason === "unreadable");
  globalThis.fetch = async () => ({ ok: false, json: async () => { throw Error("old server HTML"); } });
  await assert.rejects(fetchHistory("t"), (e) => e.reason === "unreadable");
  assert.ok(historyError("new-unknown-reason").length);
  assert.ok(!historyError("missing-directory").includes("missing-directory"));
});
