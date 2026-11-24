import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { execSync } from "node:child_process";
import { build } from "esbuild";

const here = dirname(fileURLToPath(import.meta.url));
execSync("npx esbuild src/chat.ts --bundle --format=esm --outfile=test/.chat.build.mjs", {
  cwd: join(here, ".."),
});
const {
  mergeWindow,
  fetchMessages,
  fetchMessagesWithRecovery,
  fallbackText,
  preview,
  isUserPromptMessage,
  userPromptTextBlocks,
  filterUserPromptMessages,
  MAX_AUTO_OLDER_PAGES,
  isHarnessUserText,
  hasUserPrompt,
  newlyPrependedMessages,
  loadOlderUntilUserPrompt,
} = await import("./.chat.build.mjs");

const chatViewBundle = await build({
  entryPoints: [join(here, "../src/chat-view.ts")],
  bundle: true,
  format: "esm",
  write: false,
});
const { buildChatHistoryHeader, renderMessages } = await import(
  `data:text/javascript;base64,${Buffer.from(chatViewBundle.outputFiles[0].text).toString("base64")}`
);

const msg = (id, text = "hi") => ({ id, role: "assistant", ts: null, blocks: [{ kind: "text", text }] });
const win = (messages, over = {}) => ({ source: "transcript", messages, cursor: "c", reset: false, more: false, ...over });

test("追加时按 id 去重 —— 窗口边界上那条可能被重复送来", () => {
  const prev = [msg("a"), msg("b")];
  const out = mergeWindow(prev, win([msg("b"), msg("c")]));
  assert.deepEqual(out.map((m) => m.id), ["a", "b", "c"]);
});

test("older pages prepend without moving or duplicating live messages", () => {
  const out = mergeWindow([msg("c", "latest c"), msg("d")], win([msg("a"), msg("b"), msg("c"), msg("b")]), true);
  assert.deepEqual(out.map((m) => m.id), ["a", "b", "c", "d"]);
  assert.equal(out[2].blocks[0].text, "latest c");
});

test("a reset received during backward paging replaces the old timeline", () => {
  assert.deepEqual(mergeWindow([msg("old")], win([msg("new"), msg("new")], { reset: true }), true).map((m) => m.id), ["new"]);
});

test("backward pagination passes only the opaque before cursor", async () => {
  let seen;
  globalThis.fetch = async (url) => { seen = url; return { ok: true, json: async () => win([]) }; };
  await fetchMessages("t", "codex", "s1", undefined, "opaque/+==");
  const query = new URL(seen, "http://localhost").searchParams;
  assert.equal(query.get("before"), "opaque/+==");
  assert.equal(query.has("after"), false);
});

/// reset 意味着服务端认定旧游标失效(compact 了、或换了会话)。那时旧内容和新内容
/// 不是同一条时间线,接在一起会得到一段前后不搭的对话。
test("reset 是整段替换,不是追加", () => {
  const prev = [msg("a"), msg("b")];
  const out = mergeWindow(prev, win([msg("z")], { reset: true }));
  assert.deepEqual(out.map((m) => m.id), ["z"]);
});

test("回落时不动已有消息", () => {
  const prev = [msg("a")];
  assert.deepEqual(mergeWindow(prev, { source: "terminal", reason: "disabled" }), prev);
});

test("路由缺失/报错降级为 terminal 而不是抛(协议 T12)", async () => {
  globalThis.fetch = async () => ({ ok: false, status: 404 });
  assert.equal((await fetchMessages("t", "claude-code", "s1")).source, "terminal");
  globalThis.fetch = async () => { throw new Error("offline"); };
  assert.equal((await fetchMessages("t", "claude-code", "s1")).source, "terminal");
});

test("游标作为 after 参数带上", async () => {
  let seen;
  globalThis.fetch = async (url) => { seen = url; return { ok: true, json: async () => win([]) }; };
  await fetchMessages("t", "claude-code", "s 1", "12.34");
  assert.match(seen, /agent_id=claude-code/);
  assert.match(seen, /agent_session_id=s\+1/);
  assert.match(seen, /after=12\.34/);
});

test("认不出的回落原因也要说人话,不能把机器键怼给用户", () => {
  const known = fallbackText("disabled");
  const unknown = fallbackText("something-new-from-the-future");
  assert.ok(!known.includes("disabled"));
  assert.ok(!unknown.includes("something-new"));
  assert.ok(unknown.length > 0);
});

test("preview 优先取正文,没有正文就取工具调用", () => {
  assert.equal(preview(msg("a", "正文")), "正文");
  const toolOnly = { id: "t", role: "assistant", ts: null, blocks: [{ kind: "tool-use", name: "Bash", input: "ls" }] };
  assert.equal(preview(toolOnly), "Bash ls");
});

/// 「这个 AI 读不了」比「读不了」有用得多 —— 用户至少知道是这家的问题、
/// 换个 agent 就能看,而不是以为整个功能坏了。
test("unsupported 要说清是哪一家", () => {
  const s = fallbackText("unsupported-agent", "codex");
  assert.ok(s.includes("codex"), `没说是哪一家: ${s}`);
  assert.ok(!s.includes("{agent}"), "占位符没被替换");
});

test("其它原因不受影响,也不会漏出机器键", () => {
  for (const r of ["disabled", "no-transcript-path", "unreadable", "parse-failed", "who-knows"]) {
    const s = fallbackText(r, "codex");
    assert.ok(s.length > 0);
    assert.ok(!s.includes(r), `机器键漏到界面上了: ${s}`);
  }
});

test("isUserPromptMessage 识别真正的用户发言并排除纯 tool-result / 空白 / 非用户消息", () => {
  assert.equal(isUserPromptMessage({ id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "Fix bug" }] }), true);
  assert.equal(
    isUserPromptMessage({
      id: "u2",
      role: "user",
      ts: null,
      blocks: [
        { kind: "tool-result", ok: true, text: "output" },
        { kind: "text", text: "Now add tests" },
      ],
    }),
    true,
  );
  assert.equal(isUserPromptMessage({ id: "a1", role: "assistant", ts: null, blocks: [{ kind: "text", text: "Sure" }] }), false);
  assert.equal(isUserPromptMessage({ id: "s1", role: "system", ts: null, blocks: [{ kind: "text", text: "System" }] }), false);
  assert.equal(
    isUserPromptMessage({ id: "u3", role: "user", ts: null, blocks: [{ kind: "tool-result", ok: true, text: "ok" }] }),
    false,
  );
  assert.equal(isUserPromptMessage({ id: "u4", role: "user", ts: null, blocks: [{ kind: "text", text: "  \n\t " }] }), false);
  assert.equal(isUserPromptMessage({ id: "u5", role: "user", ts: null, blocks: [] }), false);
  // Pi transcript user turns may contain [tool-use, tool-result] with no user text
  assert.equal(
    isUserPromptMessage({
      id: "u6",
      role: "user",
      ts: null,
      blocks: [
        { kind: "tool-use", name: "bash", input: "ls" },
        { kind: "tool-result", ok: true, text: "file.txt" },
      ],
    }),
    false,
  );
});

test("userPromptTextBlocks 剔除 tool-result 与空白块，仅保留非空文本块", () => {
  const mixed = {
    id: "u-mix",
    role: "user",
    ts: null,
    blocks: [
      { kind: "tool-result", ok: false, text: "err" },
      { kind: "text", text: "   " },
      { kind: "text", text: "First prompt" },
      { kind: "tool-use", name: "read", input: "a.ts" },
      { kind: "text", text: "Second line" },
    ],
  };
  assert.deepEqual(userPromptTextBlocks(mixed), [
    { kind: "text", text: "First prompt" },
    { kind: "text", text: "Second line" },
  ]);
  assert.deepEqual(userPromptTextBlocks({ id: "a1", role: "assistant", ts: null, blocks: [{ kind: "text", text: "hi" }] }), []);
});

test("filterUserPromptMessages 保持原始顺序并给出准确的用户消息数量", () => {
  const messages = [
    { id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "First" }] },
    { id: "a1", role: "assistant", ts: null, blocks: [{ kind: "tool-use", name: "bash", input: "ls" }] },
    { id: "tr1", role: "user", ts: null, blocks: [{ kind: "tool-result", ok: true, text: "out" }] },
    { id: "s1", role: "system", ts: null, blocks: [{ kind: "text", text: "notice" }] },
    {
      id: "u2",
      role: "user",
      ts: null,
      blocks: [
        { kind: "tool-result", ok: true, text: "out2" },
        { kind: "text", text: "Follow up" },
      ],
    },
    { id: "u-blank", role: "user", ts: null, blocks: [{ kind: "text", text: "\n" }] },
  ];
  const filtered = filterUserPromptMessages(messages);
  assert.deepEqual(
    filtered.map((m) => m.id),
    ["u1", "u2"],
  );
  assert.equal(filtered.length, 2);
});

function makeChatDom() {
  let doc;
  const heightOf = (node) => {
    if (node._fixedHeight !== undefined) return node._fixedHeight;
    if (node.classList?.contains("chat-toolbar")) return 40;
    if (node.classList?.contains("chat-history")) return 32;
    if (node.classList?.contains("cmsg")) return node.classList.contains("c-assistant") ? 200 : 100;
    if (node.classList?.contains("chat-filter-empty")) return 80;
    if (node.childNodes?.length) return node.scrollHeight;
    return 40;
  };
  class MockNode {
    constructor(nodeType, tagName = "") {
      this.nodeType = nodeType;
      this.tagName = tagName.toUpperCase();
      this.childNodes = [];
      this.attributes = new Map();
      this.dataset = {};
      this.style = {};
      this.listeners = new Map();
      this._className = "";
      this._text = "";
      this.open = false;
      this.tabIndex = -1;
      this.scrollTop = 0;
      this.clientHeight = 300;
      this._topOffset = 0;
      this.parentNode = null;
    }
    set className(v) {
      this._className = v;
    }
    get className() {
      return this._className;
    }
    get classList() {
      const self = this;
      const list = () => self._className.split(/\s+/).filter(Boolean);
      return {
        add(...cls) {
          const set = new Set(list());
          for (const c of cls) set.add(c);
          self._className = [...set].join(" ");
        },
        remove(...cls) {
          const set = new Set(list());
          for (const c of cls) set.delete(c);
          self._className = [...set].join(" ");
        },
        contains(c) {
          return list().includes(c);
        },
      };
    }
    set textContent(v) {
      this._text = String(v);
      this.childNodes = [];
    }
    get textContent() {
      if (this.nodeType === 3) return this._text;
      return this._text + this.childNodes.map((c) => c.textContent).join("");
    }
    get firstElementChild() {
      return this.childNodes.find((c) => c.nodeType === 1) ?? null;
    }
    get scrollHeight() {
      return this.childNodes.reduce((sum, c) => sum + (c.nodeType === 1 ? heightOf(c) : 0), 0);
    }
    appendChild(child) {
      if (child.nodeType === 11) {
        for (const grand of child.childNodes) {
          grand.parentNode = this;
          this.childNodes.push(grand);
        }
        child.childNodes = [];
      } else {
        child.parentNode = this;
        this.childNodes.push(child);
      }
      return child;
    }
    append(...nodes) {
      for (const n of nodes) this.appendChild(typeof n === "string" ? doc.createTextNode(n) : n);
    }
    replaceChildren(...nodes) {
      this.childNodes = [];
      this.append(...nodes);
    }
    setAttribute(k, v) {
      this.attributes.set(k, String(v));
    }
    getAttribute(k) {
      return this.attributes.get(k) ?? null;
    }
    addEventListener(type, fn) {
      if (!this.listeners.has(type)) this.listeners.set(type, []);
      this.listeners.get(type).push(fn);
    }
    click() {
      for (const fn of this.listeners.get("click") ?? []) fn({ target: this });
    }
    keydown(key) {
      let prevented = false;
      const ev = { key, preventDefault() { prevented = true; } };
      for (const fn of this.listeners.get("keydown") ?? []) fn(ev);
      return prevented;
    }
    focus() {
      doc.activeElement = this;
    }
    querySelectorAll(selector) {
      const out = [];
      const match = (n) => {
        if (n.nodeType !== 1) return false;
        if (selector === "[data-message-id]") return n.dataset.messageId !== undefined;
        if (selector === "details[data-block-key]") return n.tagName === "DETAILS" && n.dataset.blockKey !== undefined;
        if (selector.startsWith(".")) return n.classList.contains(selector.slice(1));
        return false;
      };
      const walk = (n) => {
        for (const c of n.childNodes) {
          if (match(c)) out.push(c);
          walk(c);
        }
      };
      walk(this);
      return out;
    }
    getBoundingClientRect() {
      if (!this.parentNode) {
        return { top: this._topOffset, bottom: this._topOffset + this.clientHeight, height: this.clientHeight };
      }
      let scroller = this.parentNode;
      let baseTop = 0;
      while (scroller.parentNode) {
        baseTop += scroller._boxOffset ?? 0;
        scroller = scroller.parentNode;
      }
      let y = baseTop;
      for (const sibling of this.parentNode.childNodes) {
        if (sibling === this) break;
        if (sibling.nodeType === 1) y += heightOf(sibling);
      }
      const h = heightOf(this);
      const top = scroller._topOffset + y - scroller.scrollTop;
      return { top, bottom: top + h, height: h };
    }
  }
  doc = {
    activeElement: null,
    createElement: (tag) => new MockNode(1, tag),
    createElementNS: (_ns, tag) => new MockNode(1, tag),
    createTextNode: (text) => {
      const n = new MockNode(3);
      n._text = String(text);
      return n;
    },
    createDocumentFragment: () => new MockNode(11),
  };
  return { doc, MockNode };
}

test("renderMessages 工具栏筛选、点击/键盘跳转、空态保留加载更早按钮与披露状态保持", (t) => {
  const prevDoc = globalThis.document;
  const prevSetTimeout = globalThis.setTimeout;
  const scheduledTimers = [];
  const { doc } = makeChatDom();
  globalThis.document = doc;
  globalThis.setTimeout = (fn, ms) => {
    const entry = { fn, ms };
    scheduledTimers.push(entry);
    return scheduledTimers.length;
  };
  t.after(() => {
    globalThis.document = prevDoc;
    globalThis.setTimeout = prevSetTimeout;
  });

  const box = doc.createElement("div");
  box.clientHeight = 300;
  const messages = [
    {
      id: "u1",
      role: "user",
      ts: null,
      blocks: [
        { kind: "tool-result", ok: true, text: "ignored result" },
        { kind: "text", text: "First question" },
      ],
    },
    {
      id: "a1",
      role: "assistant",
      ts: null,
      blocks: [
        { kind: "thinking", text: "deep thought" },
        { kind: "text", text: "First answer" },
      ],
    },
    { id: "tr1", role: "user", ts: null, blocks: [{ kind: "tool-result", ok: true, text: "tool out" }] },
    { id: "a2", role: "assistant", ts: null, blocks: [{ kind: "text", text: "Second answer" }] },
    { id: "u2", role: "user", ts: null, blocks: [{ kind: "text", text: "Second question" }] },
    { id: "a3", role: "assistant", ts: null, blocks: [{ kind: "text", text: "Third answer" }] },
  ];

  let userOnly = false;
  let jumpToId = null;
  let toggles = 0;
  const picked = [];
  const paint = () => {
    const j = jumpToId;
    jumpToId = null;
    renderMessages(box, messages, "s1", box, {
      userOnly,
      jumpToId: j,
      onToggleUserOnly: () => {
        toggles++;
        userOnly = !userOnly;
        paint();
      },
      onPickUserMessage: (id) => {
        picked.push(id);
        userOnly = false;
        jumpToId = id;
        paint();
      },
    });
  };

  // 1. Initial full render: toolbar shows count (2), and open disclosure state
  paint();
  const filterBtn1 = box.querySelectorAll(".chat-filter-btn")[0];
  assert.ok(filterBtn1);
  assert.equal(filterBtn1.getAttribute("aria-pressed"), "false");
  assert.ok(filterBtn1.textContent.includes("(2)"));
  const thinkingDetails = box.querySelectorAll("details[data-block-key]")[0];
  assert.ok(thinkingDetails);
  thinkingDetails.open = true;

  // 2. Toggle userOnly on via filter button click
  filterBtn1.click();
  assert.equal(toggles, 1);
  assert.equal(userOnly, true);
  const filterBtn2 = box.querySelectorAll(".chat-filter-btn")[0];
  assert.equal(filterBtn2.getAttribute("aria-pressed"), "true");
  assert.ok(box.querySelectorAll(".chat-filter-hint")[0].textContent.includes("2"));
  assert.equal(box.querySelectorAll(".chat-filter-clear").length, 1);

  const filteredRows = box.querySelectorAll("[data-message-id]");
  assert.deepEqual(
    filteredRows.map((r) => r.dataset.messageId),
    ["u1", "u2"],
  );
  // Mixed user message renders ONLY non-empty text block + jump cue (no tool-result details)
  assert.equal(box.querySelectorAll("details[data-block-key]").length, 0);
  assert.ok(filteredRows[0].classList.contains("is-jumpable"));
  assert.equal(filteredRows[0].getAttribute("role"), "button");
  assert.equal(filteredRows[0].tabIndex, 0);
  assert.ok(filteredRows[0].textContent.includes("First question"));
  assert.equal(filteredRows[0].textContent.includes("ignored result"), false);
  const jumpCue = filteredRows[0].querySelectorAll(".cmsg-jump-cue")[0];
  assert.equal(jumpCue.textContent, "");
  assert.equal(jumpCue.firstElementChild?.tagName, "SVG");
  assert.equal(jumpCue.firstElementChild?.getAttribute("aria-hidden"), "true");

  // Keyboard Enter and Space also invoke onPickUserMessage
  assert.equal(filteredRows[1].keydown(" "), true);
  assert.deepEqual(picked, ["u2"]);
  assert.equal(userOnly, false);

  // Jump target u2 sits at: toolbar(40) + u1(100) + a1(200) + tr1(100) + a2(200) = 640
  // Expected scrollTop = Math.max(0, 640 - 40 - 24) = 576 (NOT snapped to bottom 940!)
  assert.equal(box.scrollTop, 576);
  const jumpedRow = box.querySelectorAll("[data-message-id]").find((r) => r.dataset.messageId === "u2");
  assert.ok(jumpedRow.classList.contains("cmsg-jump-highlight"));
  // Disclosure state from before entering userOnly mode was NOT lost!
  assert.equal(box.querySelectorAll("details[data-block-key]")[0].open, true);

  // 1800ms timer clears highlight class
  const jumpTimer = scheduledTimers.find((entry) => entry.ms === 1800);
  assert.ok(jumpTimer);
  jumpTimer.fn();
  assert.equal(jumpedRow.classList.contains("cmsg-jump-highlight"), false);

  // Subsequent live agent-event refresh preserves jumped scroll position (does not snap to bottom)
  paint();
  assert.equal(box.scrollTop, 576);

  // Enter key works on filtered row too
  box.querySelectorAll(".chat-filter-btn")[0].click();
  const u1Row = box.querySelectorAll("[data-message-id]")[0];
  assert.equal(u1Row.keydown("Enter"), true);
  assert.deepEqual(picked, ["u2", "u1"]);
  // u1 sits at y = 40; Math.max(0, 40 - 40 - 24) = 0
  assert.equal(box.scrollTop, 0);
});

test("renderMessages 筛选空态保留 load-older header 并可从空态清除筛选", (t) => {
  const prevDoc = globalThis.document;
  const { doc } = makeChatDom();
  globalThis.document = doc;
  t.after(() => {
    globalThis.document = prevDoc;
  });

  const box = doc.createElement("div");
  const header = doc.createElement("div");
  header.className = "chat-history";
  header.textContent = "Load earlier messages";
  const onlyAssistantAndTool = [
    { id: "a1", role: "assistant", ts: null, blocks: [{ kind: "text", text: "working..." }] },
    { id: "tr1", role: "user", ts: null, blocks: [{ kind: "tool-result", ok: true, text: "done" }] },
  ];

  let cleared = 0;
  renderMessages(box, onlyAssistantAndTool, "s2", box, {
    header,
    userOnly: true,
    onToggleUserOnly: () => {
      cleared++;
    },
  });

  const children = box.childNodes;
  assert.ok(children[0].classList.contains("chat-toolbar"));
  assert.equal(children[1], header);
  assert.ok(children[2].classList.contains("chat-filter-empty"));
  const emptyClearBtn = children[2].querySelectorAll(".chat-filter-clear")[0];
  assert.ok(emptyClearBtn);
  emptyClearBtn.click();
  assert.equal(cleared, 1);
});

test("renderMessages 在独立 scrollContainer (如 history-preview) 中正确计算相对偏移", (t) => {
  const prevDoc = globalThis.document;
  const prevSetTimeout = globalThis.setTimeout;
  const { doc } = makeChatDom();
  globalThis.document = doc;
  globalThis.setTimeout = () => 1;
  t.after(() => {
    globalThis.document = prevDoc;
    globalThis.setTimeout = prevSetTimeout;
  });

  const preview = doc.createElement("section");
  preview.clientHeight = 400;
  const head = doc.createElement("div");
  head._fixedHeight = 150;
  const body = doc.createElement("div");
  body._boxOffset = 150;
  preview.append(head, body);

  const messages = [
    { id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "first" }] },
    { id: "a1", role: "assistant", ts: null, blocks: [{ kind: "text", text: "reply" }] },
    { id: "u2", role: "user", ts: null, blocks: [{ kind: "text", text: "second" }] },
  ];

  renderMessages(body, messages, "hist-1", preview, {
    userOnly: false,
    jumpToId: "u2",
    onToggleUserOnly: () => {},
  });

  // In preview: headOffset(150) + toolbar(40) + u1(100) + a1(200) = 490
  // scrollTop = Math.max(0, 490 - 40 - 24) = 426
  assert.equal(preview.scrollTop, 426);
});

test("renderMessages 在 userOnly 模式下加载更早消息 (prepend) 时保持锚点滚动位置", (t) => {
  const prevDoc = globalThis.document;
  const { doc } = makeChatDom();
  globalThis.document = doc;
  t.after(() => {
    globalThis.document = prevDoc;
  });

  const box = doc.createElement("div");
  box.clientHeight = 150;
  const initial = [
    { id: "u2", role: "user", ts: null, blocks: [{ kind: "text", text: "second" }] },
    { id: "a2", role: "assistant", ts: null, blocks: [{ kind: "text", text: "ans2" }] },
    { id: "u3", role: "user", ts: null, blocks: [{ kind: "text", text: "third" }] },
    { id: "u4", role: "user", ts: null, blocks: [{ kind: "text", text: "fourth" }] },
  ];

  renderMessages(box, initial, "s3", box, { userOnly: true, onToggleUserOnly: () => {} });
  // Scroll up to look at u2 (top=40, bottom=140)
  box.scrollTop = 40;

  const prepended = [
    { id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "first" }] },
    { id: "a1", role: "assistant", ts: null, blocks: [{ kind: "text", text: "ans1" }] },
    ...initial,
  ];
  renderMessages(box, prepended, "s3", box, { prepend: true, userOnly: true, onToggleUserOnly: () => {} });

  // u1 (100px) was prepended in filtered view (a1 filtered out), so scrollTop shifts by +100
  assert.equal(box.scrollTop, 140);
});

test("fetchMessages preserves HTTP status on non-2xx fallback", async (t) => {
  const prevFetch = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = prevFetch;
  });

  globalThis.fetch = async () => ({ ok: false, status: 404 });
  assert.deepEqual(await fetchMessages("t", "antigravity", "s-old"), {
    source: "terminal",
    reason: "unreadable",
    status: 404,
  });

  globalThis.fetch = async () => ({ ok: false, status: 500 });
  assert.deepEqual(await fetchMessages("t", "antigravity", "s-old"), {
    source: "terminal",
    reason: "unreadable",
    status: 500,
  });

  globalThis.fetch = async () => {
    throw new Error("offline");
  };
  assert.deepEqual(await fetchMessages("t", "antigravity", "s-old"), {
    source: "terminal",
    reason: "unreadable",
  });
});

test("fetchMessagesWithRecovery refreshes agents once and retries on 404", async () => {
  const sOld = { agent_id: "antigravity", agent_session_id: "s-old" };
  const sNew = { agent_id: "antigravity", agent_session_id: "s-new" };
  const expectedWin = win([msg("m1", "recovered")]);
  let active = sOld;
  let refreshCount = 0;
  const fetched = [];

  const result = await fetchMessagesWithRecovery({
    initialAgent: sOld,
    currentAgent: () => active,
    refreshAgents: async () => {
      refreshCount++;
      active = sNew;
    },
    fetchWindow: async (agent) => {
      fetched.push(agent.agent_session_id);
      if (agent.agent_session_id === "s-old") {
        return { source: "terminal", reason: "unreadable", status: 404 };
      }
      return expectedWin;
    },
  });

  assert.equal(refreshCount, 1);
  assert.deepEqual(fetched, ["s-old", "s-new"]);
  assert.deepEqual(result, { agent: sNew, window: expectedWin });
});

test("fetchMessagesWithRecovery stops without looping when retry also returns 404", async () => {
  const sOld = { agent_id: "antigravity", agent_session_id: "s-old" };
  const sNew = { agent_id: "antigravity", agent_session_id: "s-new" };
  const notFound = { source: "terminal", reason: "unreadable", status: 404 };
  let active = sOld;
  let refreshCount = 0;
  const fetched = [];

  const result = await fetchMessagesWithRecovery({
    initialAgent: sOld,
    currentAgent: () => active,
    refreshAgents: async () => {
      refreshCount++;
      active = sNew;
    },
    fetchWindow: async (agent) => {
      fetched.push(agent.agent_session_id);
      return notFound;
    },
  });

  assert.equal(refreshCount, 1);
  assert.deepEqual(fetched, ["s-old", "s-new"]);
  assert.deepEqual(result, { agent: sNew, window: notFound });
});

test("fetchMessagesWithRecovery returns null agent when currentAgent disappears after refresh", async () => {
  const sOld = { agent_id: "antigravity", agent_session_id: "s-old" };
  const notFound = { source: "terminal", reason: "unreadable", status: 404 };
  let active = sOld;
  let refreshCount = 0;
  const fetched = [];

  const result = await fetchMessagesWithRecovery({
    initialAgent: sOld,
    currentAgent: () => active,
    refreshAgents: async () => {
      refreshCount++;
      active = null;
    },
    fetchWindow: async (agent) => {
      fetched.push(agent.agent_session_id);
      return notFound;
    },
  });

  assert.equal(refreshCount, 1);
  assert.deepEqual(fetched, ["s-old"]);
  assert.deepEqual(result, { agent: null, window: notFound });
});

test("fetchMessagesWithRecovery does not refreshAgents on non-404 fallbacks", async () => {
  const initial = { agent_id: "antigravity", agent_session_id: "s-1" };
  for (const fallbackWin of [
    { source: "terminal", reason: "unreadable", status: 500 },
    { source: "terminal", reason: "disabled" },
    { source: "terminal", reason: "no-transcript-path" },
  ]) {
    let refreshCount = 0;
    const fetched = [];
    const result = await fetchMessagesWithRecovery({
      initialAgent: initial,
      currentAgent: () => initial,
      refreshAgents: async () => {
        refreshCount++;
      },
      fetchWindow: async (agent) => {
        fetched.push(agent.agent_session_id);
        return fallbackWin;
      },
    });
    assert.equal(refreshCount, 0);
    assert.deepEqual(fetched, ["s-1"]);
    assert.deepEqual(result, { agent: initial, window: fallbackWin });
  }
});

test("isUserPromptMessage and userPromptTextBlocks filter all 8 harness prefixes and empty Codex marker", () => {
  const harnessTexts = [
    "# AGENTS.md instructions\nAlways run tests",
    "<environment_context>\n<cwd>/repo</cwd>\n</environment_context>",
    "<permissions instructions>Read only</permissions instructions>",
    "<developer_instructions>Be concise</developer_instructions>",
    "<system-reminder>Hook output</system-reminder>",
    "<local-command-caveat>Caveat</local-command-caveat>",
    "<command-name>/clear</command-name>",
    "This session is being continued from a previous conversation that ran out of context.",
    "  \n<system-reminder>with leading whitespace</system-reminder>  ",
    "## My request for Codex:\n   ",
    "IDE context\n## My request for Codex:\n \t ",
  ];
  for (const text of harnessTexts) {
    assert.equal(isHarnessUserText(text), true, `expected harness text: ${text}`);
    assert.equal(
      isUserPromptMessage({ id: "h", role: "user", ts: null, blocks: [{ kind: "text", text }] }),
      false,
      `expected non-prompt for: ${text}`,
    );
  }

  const mixed = {
    id: "u-mixed-harness",
    role: "user",
    ts: null,
    blocks: [
      { kind: "tool-result", ok: true, text: "done" },
      { kind: "text", text: "<system-reminder>injected</system-reminder>" },
      { kind: "text", text: "Real prompt" },
    ],
  };
  assert.equal(isUserPromptMessage(mixed), true);
  assert.deepEqual(userPromptTextBlocks(mixed), [{ kind: "text", text: "Real prompt" }]);

  const codexWrapped = {
    id: "u-codex",
    role: "user",
    ts: null,
    blocks: [{ kind: "text", text: "Context\n## My request for Codex:\nFix it" }],
  };
  assert.equal(isHarnessUserText("Context\n## My request for Codex:\nFix it"), false);
  assert.equal(isUserPromptMessage(codexWrapped), true);
  assert.deepEqual(userPromptTextBlocks(codexWrapped), [
    { kind: "text", text: "Context\n## My request for Codex:\nFix it" },
  ]);

  assert.equal(
    hasUserPrompt([
      { id: "a1", role: "assistant", ts: null, blocks: [{ kind: "text", text: "hi" }] },
      { id: "u-sys", role: "user", ts: null, blocks: [{ kind: "text", text: "<environment_context></environment_context>" }] },
    ]),
    false,
  );
  assert.equal(hasUserPrompt([mixed]), true);
});

test("loadOlderUntilUserPrompt stops after 1 page when page 1 contains a human user prompt", async () => {
  const userPrompt = { id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "Hello" }] };
  let messages = [msg("a2")];
  const fetched = [];
  const pages = [];
  const res = await loadOlderUntilUserPrompt({
    initialCursor: "c1",
    isCancelled: () => false,
    fetchPage: async (before) => {
      fetched.push(before);
      return win([userPrompt, msg("a1")], { older_cursor: "c0" });
    },
    getMessages: () => messages,
    onPage: (w, incoming) => {
      pages.push(incoming.map((m) => m.id));
      messages = mergeWindow(messages, w, true);
    },
    onReset: () => assert.fail("unexpected reset"),
  });
  assert.deepEqual(res, { stop: "found-user-prompt", pagesFetched: 1 });
  assert.deepEqual(fetched, ["c1"]);
  assert.deepEqual(pages, [["u1", "a1"]]);
  assert.deepEqual(messages.map((m) => m.id), ["u1", "a1", "a2"]);
});

test("loadOlderUntilUserPrompt skips tool-results, assistant, harness blocks, and empty pages until prompt arrives", async () => {
  let messages = [msg("a-tail")];
  const responses = new Map([
    [
      "c1",
      win(
        [
          { id: "tr1", role: "user", ts: null, blocks: [{ kind: "tool-result", ok: true, text: "out" }] },
          { id: "h1", role: "user", ts: null, blocks: [{ kind: "text", text: "<system-reminder>skip</system-reminder>" }] },
          msg("a1"),
        ],
        { older_cursor: "c2" },
      ),
    ],
    ["c2", win([], { older_cursor: "c3" })],
    [
      "c3",
      win(
        [
          { id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "Find the bug" }] },
          msg("a0"),
        ],
        { older_cursor: "c4" },
      ),
    ],
  ]);
  const fetched = [];
  const res = await loadOlderUntilUserPrompt({
    initialCursor: "c1",
    isCancelled: () => false,
    fetchPage: async (before) => {
      fetched.push(before);
      return responses.get(before);
    },
    getMessages: () => messages,
    onPage: (w) => {
      messages = mergeWindow(messages, w, true);
    },
    onReset: () => assert.fail("unexpected reset"),
  });
  assert.deepEqual(res, { stop: "found-user-prompt", pagesFetched: 3 });
  assert.deepEqual(fetched, ["c1", "c2", "c3"]);
  assert.deepEqual(messages.map((m) => m.id), ["u1", "a0", "tr1", "h1", "a1", "a-tail"]);
});

test("loadOlderUntilUserPrompt does not stop prematurely on boundary-duplicated user prompt IDs", async () => {
  const u2 = { id: "u2", role: "user", ts: null, blocks: [{ kind: "text", text: "Second" }] };
  const u1 = { id: "u1", role: "user", ts: null, blocks: [{ kind: "text", text: "First" }] };
  let messages = [u2, msg("a2")];
  const fetched = [];
  const res = await loadOlderUntilUserPrompt({
    initialCursor: "c1",
    isCancelled: () => false,
    fetchPage: async (before) => {
      fetched.push(before);
      if (before === "c1") {
        // Repeats u2 on the page boundary; newlyPrependedMessages should exclude u2.
        return win([msg("a1"), u2], { older_cursor: "c2" });
      }
      return win([u1], { older_cursor: "c3" });
    },
    getMessages: () => messages,
    onPage: (w) => {
      messages = mergeWindow(messages, w, true);
    },
    onReset: () => assert.fail("unexpected reset"),
  });
  assert.deepEqual(res, { stop: "found-user-prompt", pagesFetched: 2 });
  assert.deepEqual(fetched, ["c1", "c2"]);
  assert.deepEqual(newlyPrependedMessages([u2, msg("a2")], win([msg("a1"), u2])).map((m) => m.id), ["a1"]);
});

test("loadOlderUntilUserPrompt stops with beginning when older_cursor becomes null", async () => {
  let messages = [msg("a2")];
  const res = await loadOlderUntilUserPrompt({
    initialCursor: "c1",
    isCancelled: () => false,
    fetchPage: async () => win([msg("a1")], { older_cursor: null }),
    getMessages: () => messages,
    onPage: (w) => {
      messages = mergeWindow(messages, w, true);
    },
    onReset: () => assert.fail("unexpected reset"),
  });
  assert.deepEqual(res, { stop: "beginning", pagesFetched: 1 });
});

test("loadOlderUntilUserPrompt stops at MAX_AUTO_OLDER_PAGES (16) with page-limit", async () => {
  assert.equal(MAX_AUTO_OLDER_PAGES, 16);
  let messages = [msg("a-tail")];
  let n = 0;
  const res = await loadOlderUntilUserPrompt({
    initialCursor: "c0",
    isCancelled: () => false,
    fetchPage: async () => {
      n++;
      return win([msg(`a-${n}`)], { older_cursor: `c${n}` });
    },
    getMessages: () => messages,
    onPage: (w) => {
      messages = mergeWindow(messages, w, true);
    },
    onReset: () => assert.fail("unexpected reset"),
  });
  assert.deepEqual(res, { stop: "page-limit", pagesFetched: 16 });
  assert.equal(n, 16);
});

test("loadOlderUntilUserPrompt stops with failed on throw, terminal fallback, null, or stalled cursor while preserving prior pages", async () => {
  // 1. Network throw on page 2 keeps page 1
  {
    let messages = [msg("a-tail")];
    let count = 0;
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "c1",
      isCancelled: () => false,
      fetchPage: async () => {
        count++;
        if (count === 1) return win([msg("a1")], { older_cursor: "c2" });
        throw new Error("network down");
      },
      getMessages: () => messages,
      onPage: (w) => {
        messages = mergeWindow(messages, w, true);
      },
      onReset: () => assert.fail("unexpected reset"),
    });
    assert.deepEqual(res, { stop: "failed", pagesFetched: 1 });
    assert.deepEqual(messages.map((m) => m.id), ["a1", "a-tail"]);
  }

  // 2. Terminal fallback
  {
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "c1",
      isCancelled: () => false,
      fetchPage: async () => ({ source: "terminal", reason: "unreadable" }),
      getMessages: () => [],
      onPage: () => assert.fail("unexpected onPage"),
      onReset: () => assert.fail("unexpected reset"),
    });
    assert.deepEqual(res, { stop: "failed", pagesFetched: 0 });
  }

  // 3. Null return (e.g., caught HistoryError)
  {
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "c1",
      isCancelled: () => false,
      fetchPage: async () => null,
      getMessages: () => [],
      onPage: () => assert.fail("unexpected onPage"),
      onReset: () => assert.fail("unexpected reset"),
    });
    assert.deepEqual(res, { stop: "failed", pagesFetched: 0 });
  }

  // 4. Stalled cursor (!reset && win.older_cursor === before)
  {
    let messages = [msg("a-tail")];
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "c1",
      isCancelled: () => false,
      fetchPage: async (before) => {
        if (before === "c1") return win([msg("a1")], { older_cursor: "c2" });
        return win([msg("a-dup")], { older_cursor: "c2" });
      },
      getMessages: () => messages,
      onPage: (w) => {
        messages = mergeWindow(messages, w, true);
      },
      onReset: () => assert.fail("unexpected reset"),
    });
    assert.deepEqual(res, { stop: "failed", pagesFetched: 1 });
    assert.deepEqual(messages.map((m) => m.id), ["a1", "a-tail"]);
  }
});

test("loadOlderUntilUserPrompt handles reset for old history servers vs real transcript resets", async () => {
  // Old history server ignores ?before= and returns reset: true with same older_cursor
  {
    let resetCalled = 0;
    let pageCalled = 0;
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "same-cursor",
      isHistoryPreview: true,
      isCancelled: () => false,
      fetchPage: async () => win([msg("a-head")], { reset: true, older_cursor: "same-cursor" }),
      getMessages: () => [msg("a-head")],
      onPage: () => {
        pageCalled++;
      },
      onReset: () => {
        resetCalled++;
      },
    });
    assert.deepEqual(res, { stop: "unsupported-old-server", pagesFetched: 0 });
    assert.equal(resetCalled, 0);
    assert.equal(pageCalled, 0);
  }

  // Live chat receiving reset with unchanged cursor fails cleanly instead of calling onReset
  {
    let resetCalled = 0;
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "same-cursor",
      isHistoryPreview: false,
      isCancelled: () => false,
      fetchPage: async () => win([msg("a-head")], { reset: true, older_cursor: "same-cursor" }),
      getMessages: () => [msg("a-head")],
      onPage: () => assert.fail("unexpected onPage"),
      onReset: () => {
        resetCalled++;
      },
    });
    assert.deepEqual(res, { stop: "failed", pagesFetched: 0 });
    assert.equal(resetCalled, 0);
  }

  // Real reset with different older_cursor calls onReset once and stops
  {
    const resets = [];
    const res = await loadOlderUntilUserPrompt({
      initialCursor: "c1",
      isHistoryPreview: true,
      isCancelled: () => false,
      fetchPage: async () => win([msg("fresh")], { reset: true, older_cursor: "c-new" }),
      getMessages: () => [msg("old")],
      onPage: () => assert.fail("unexpected onPage"),
      onReset: (w) => {
        resets.push(w.messages.map((m) => m.id));
      },
    });
    assert.deepEqual(res, { stop: "reset", pagesFetched: 0 });
    assert.deepEqual(resets, [["fresh"]]);
  }
});

test("loadOlderUntilUserPrompt exits with cancelled without calling onPage or onReset when cancelled mid-flight", async () => {
  let cancelled = false;
  let pages = 0;
  let resets = 0;
  const res = await loadOlderUntilUserPrompt({
    initialCursor: "c1",
    isCancelled: () => cancelled,
    fetchPage: async () => {
      cancelled = true;
      return win([msg("a1")], { older_cursor: "c2" });
    },
    getMessages: () => [msg("a2")],
    onPage: () => {
      pages++;
    },
    onReset: () => {
      resets++;
    },
  });
  assert.deepEqual(res, { stop: "cancelled", pagesFetched: 0 });
  assert.equal(pages, 0);
  assert.equal(resets, 0);
});

test("renderMessages keepBottomIfAtBottom pins short page to bottom while preserving anchor when scrolled up", (t) => {
  const prevDoc = globalThis.document;
  const { doc } = makeChatDom();
  globalThis.document = doc;
  t.after(() => {
    globalThis.document = prevDoc;
  });

  // 1. Short initial page (clientHeight = 300, 1 assistant message = 200px, scrollTop = 0, at bottom)
  {
    const box = doc.createElement("div");
    box.clientHeight = 300;
    const initial = [msg("a4")];
    renderMessages(box, initial, "short-1", box);
    box.scrollTop = 0;

    const older = [msg("a1"), msg("a2"), msg("a3"), ...initial];
    renderMessages(box, older, "short-1", box, { prepend: true, keepBottomIfAtBottom: true });
    assert.equal(box.scrollHeight, 800);
    assert.equal(box.scrollTop, 800);
  }

  // 2. Scrolled up (!atBottom): clientHeight = 300, 2 assistant messages = 400px, scrollTop = 40
  {
    const box = doc.createElement("div");
    box.clientHeight = 300;
    const initial = [msg("a3"), msg("a4")];
    renderMessages(box, initial, "long-1", box);
    box.scrollTop = 40;

    const older = [msg("a1"), msg("a2"), ...initial];
    renderMessages(box, older, "long-1", box, { prepend: true, keepBottomIfAtBottom: true });
    // Two 200px messages prepended above a3; anchor keeps a3 at its previous offset -> 40 + 400 = 440 (not bottom 800)
    assert.equal(box.scrollHeight, 800);
    assert.equal(box.scrollTop, 440);
  }
});

test("buildChatHistoryHeader renders load, loading, retry, and beginning states consistently", (t) => {
  const prevDoc = globalThis.document;
  const { doc } = makeChatDom();
  globalThis.document = doc;
  t.after(() => {
    globalThis.document = prevDoc;
  });

  assert.equal(
    buildChatHistoryHeader({
      supported: false,
      olderCursor: null,
      loading: false,
      failed: false,
      onLoadOlder: () => {},
    }),
    undefined,
  );

  let clicks = 0;
  const ready = buildChatHistoryHeader({
    supported: true,
    olderCursor: "c1",
    loading: false,
    failed: false,
    onLoadOlder: () => { clicks++; },
  });
  const btn = ready.firstElementChild;
  assert.ok(btn);
  assert.equal(btn.tagName, "BUTTON");
  assert.equal(btn.disabled, false);
  btn.click();
  assert.equal(clicks, 1);

  const loading = buildChatHistoryHeader({
    supported: true,
    olderCursor: "c1",
    loading: true,
    failed: false,
    onLoadOlder: () => {},
  });
  assert.equal(loading.firstElementChild.disabled, true);

  const failed = buildChatHistoryHeader({
    supported: true,
    olderCursor: "c1",
    loading: false,
    failed: true,
    onLoadOlder: () => {},
  });
  assert.equal(failed.firstElementChild.disabled, false);
  assert.notEqual(failed.textContent, ready.textContent);

  const beginning = buildChatHistoryHeader({
    supported: true,
    olderCursor: null,
    loading: false,
    failed: false,
    onLoadOlder: () => {},
  });
  assert.equal(beginning.firstElementChild, null);
  assert.ok(beginning.textContent.length > 0);
});
