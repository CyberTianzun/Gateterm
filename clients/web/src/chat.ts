/**
 * 聊天视图的数据层。
 *
 * 消息来自 agent 自己写的对话记录(服务端读、归一化,见 `agent/transcript`),走 HTTP
 * 拉取 —— 一段对话可以是几 MB,协议 R4 不许在控制通道走大块数据。**增量由
 * `agent-event` 触发**:有事件才可能有新消息,所以不做轮询。
 *
 * 纯逻辑与 DOM 分开,和 `agents.ts` 同规矩:合并、去重能被 node:test 直接跑。
 */
import { type MsgKey, t } from "./i18n";

export type Role = "user" | "assistant" | "system";

export type Block =
  | { kind: "text"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "tool-use"; name: string; input: string }
  | { kind: "tool-result"; ok: boolean; text: string };

export interface Message {
  id: string;
  role: Role;
  ts: number | null;
  blocks: Block[];
}

/** 读不到对话记录时的原因。服务端给的是机器可读的键,文案在这里。 */
export type FallbackReason =
  | "disabled"
  | "no-transcript-path"
  | "unsupported-agent"
  | "unreadable"
  | "parse-failed";

export type ChatWindow =
  | { source: "transcript"; messages: Message[]; cursor: string; reset: boolean; more: boolean; older_cursor?: string | null }
  | { source: "terminal"; reason: FallbackReason; status?: number };

const FALLBACK_KEY: Record<FallbackReason, MsgKey> = {
  disabled: "chat.fallback.disabled",
  "no-transcript-path": "chat.fallback.noPath",
  "unsupported-agent": "chat.fallback.unsupported",
  unreadable: "chat.fallback.unreadable",
  "parse-failed": "chat.fallback.parseFailed",
};

/**
 * 认不出的原因也要说人话,而不是把机器键怼给用户。
 *
 * `unsupported-agent` 额外带上是哪一家:「这个 AI 读不了」比「读不了」有用得多 ——
 * 用户至少知道是这家的问题、换个 agent 就能看,而不是以为整个功能坏了。
 */
export function fallbackText(reason: string, agentName?: string): string {
  if (reason === "unsupported-agent") {
    return t("chat.fallback.unsupported", { agent: agentName ?? "?" });
  }
  const key = FALLBACK_KEY[reason as FallbackReason];
  return key ? t(key) : t("chat.fallback.unreadable");
}

export async function fetchMessages(
  token: string,
  agentId: string,
  agentSessionId: string,
  after?: string,
  before?: string,
): Promise<ChatWindow> {
  const q = new URLSearchParams({ agent_id: agentId, agent_session_id: agentSessionId });
  if (after) q.set("after", after);
  if (before) q.set("before", before);
  try {
    const r = await fetch(`/api/agents/messages?${q}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    // 老版本服务端没有这个路由。降级,不是报错(协议 T12)。
    if (!r.ok) return { source: "terminal", reason: "unreadable", status: r.status };
    return (await r.json()) as ChatWindow;
  } catch {
    return { source: "terminal", reason: "unreadable" };
  }
}

export interface AgentRef {
  agent_id: string;
  agent_session_id: string;
}

/**
 * 拉取对话窗口，遇到 HTTP 404 时刷新一次 agent 会话快照并重试一次。
 *
 * 当服务端已经替换/重连了 agent_session_id 而前端 `agents` 尚未同步时，旧的
 * `agent_session_id` 会收到 `404 no such agent session`。只重试一次，防止
 * 服务端持续 404 时陷入死循环。
 */
export async function fetchMessagesWithRecovery<T extends AgentRef>(opts: {
  fetchWindow: (agent: T) => Promise<ChatWindow>;
  refreshAgents: () => Promise<void>;
  currentAgent: () => T | null;
  initialAgent: T;
}): Promise<{ agent: T | null; window: ChatWindow }> {
  const window = await opts.fetchWindow(opts.initialAgent);
  if (window.source !== "terminal" || window.status !== 404) {
    return { agent: opts.initialAgent, window };
  }
  await opts.refreshAgents();
  const next = opts.currentAgent();
  if (!next) {
    return { agent: null, window };
  }
  const retryWin = await opts.fetchWindow(next);
  return { agent: next, window: retryWin };
}

function uniqueIncoming(base: Message[], messages: Message[]): Message[] {
  const seen = new Set(base.map((m) => m.id));
  return messages.filter((m) => {
    if (seen.has(m.id)) return false;
    seen.add(m.id);
    return true;
  });
}

/**
 * 把一次拉取合并进已有的消息。
 *
 * `reset` 是**整段替换**而不是追加:它意味着服务端认定旧游标失效(记录被 compact 了、
 * 或者换了个会话),那时旧内容和新内容不是同一条时间线,接在一起会得到一段前后不搭的
 * 对话。
 *
 * 追加时按 id 去重:窗口边界上的那条可能被重复送来。
 */
export function mergeWindow(prev: Message[], win: ChatWindow, prepend = false): Message[] {
  if (win.source !== "transcript") return prev;
  const base = win.reset ? [] : prev;
  const incoming = uniqueIncoming(base, win.messages);
  return prepend ? incoming.concat(base) : base.concat(incoming);
}

/** 一条消息的纯文本预览,给折叠态和无障碍用。 */
export function preview(m: Message): string {
  for (const b of m.blocks) {
    if (b.kind === "text") return b.text;
    if (b.kind === "tool-use") return `${b.name} ${b.input}`;
  }
  return "";
}

const HARNESS_USER_PREFIXES = [
  "# AGENTS.md instructions",
  "<environment_context>",
  "<permissions instructions>",
  "<developer_instructions>",
  "<system-reminder>",
  "<local-command-caveat>",
  "<command-name>",
  "This session is being continued from a previous conversation",
] as const;

const CODEX_REQUEST_MARKER = "## My request for Codex:";

/** 判断 role="user" 的文本块是否为空白或框架注入上下文（对齐服务端 history::prompt_body）。 */
export function isHarnessUserText(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return true;
  if (HARNESS_USER_PREFIXES.some((prefix) => trimmed.startsWith(prefix))) return true;
  const markerIdx = trimmed.indexOf(CODEX_REQUEST_MARKER);
  if (markerIdx !== -1 && !trimmed.slice(markerIdx + CODEX_REQUEST_MARKER.length).trim()) {
    return true;
  }
  return false;
}

/** 仅提取用户主动发出的非空正文块，剔除同条里的 tool-result、空白与框架注入上下文。 */
export function userPromptTextBlocks(m: Message): Extract<Block, { kind: "text" }>[] {
  if (m.role !== "user") return [];
  return m.blocks.filter(
    (b): b is Extract<Block, { kind: "text" }> =>
      b.kind === "text" && b.text.trim().length > 0 && !isHarnessUserText(b.text),
  );
}

/**
 * 判断一条消息是否为用户真正输入的一轮提示（而非工具回传/空白占位/系统注入的 user turn）。
 * 不少 agent 的记录会把 tool-result 甚至 Pi 的 [tool-use, tool-result] 记成 role="user"。
 */
export function isUserPromptMessage(m: Message): boolean {
  return userPromptTextBlocks(m).length > 0;
}

/** 筛出所有用户输入的消息并保持原始时间顺序。 */
export function filterUserPromptMessages(messages: Message[]): Message[] {
  return messages.filter(isUserPromptMessage);
}

/** 判断消息列表中是否已包含至少一条用户主动提问。 */
export function hasUserPrompt(messages: Message[]): boolean {
  return messages.some(isUserPromptMessage);
}

export const MAX_AUTO_OLDER_PAGES = 16;

export type OlderLoadStop =
  | "found-user-prompt"
  | "beginning"
  | "page-limit"
  | "reset"
  | "unsupported-old-server"
  | "failed"
  | "cancelled";

/** 计算本次向后翻页真正新增（未与已有消息重复）的前置消息集合。 */
export function newlyPrependedMessages(prev: Message[], win: ChatWindow): Message[] {
  if (win.source !== "transcript") return [];
  const base = win.reset ? [] : prev;
  return uniqueIncoming(base, win.messages);
}

/**
 * 自动连续向后拉取更早消息，直到新前置的消息中包含用户主动提问、到达开头、
 * 触及单次上限（16 页）、遇到 reset、失败或被取消为止。
 */
export async function loadOlderUntilUserPrompt(opts: {
  initialCursor: string | null;
  maxPages?: number;
  isHistoryPreview?: boolean;
  isCancelled: () => boolean;
  fetchPage: (before: string) => Promise<ChatWindow | null>;
  getMessages: () => Message[];
  onPage: (win: Extract<ChatWindow, { source: "transcript" }>, incoming: Message[]) => void;
  onReset: (win: Extract<ChatWindow, { source: "transcript" }>) => void;
}): Promise<{ stop: OlderLoadStop; pagesFetched: number }> {
  const maxPages = opts.maxPages ?? MAX_AUTO_OLDER_PAGES;
  let cursor = opts.initialCursor;
  let pagesFetched = 0;

  while (true) {
    if (opts.isCancelled()) return { stop: "cancelled", pagesFetched };
    if (!cursor) return { stop: "beginning", pagesFetched };
    if (pagesFetched >= maxPages) return { stop: "page-limit", pagesFetched };

    let win: ChatWindow | null;
    try {
      win = await opts.fetchPage(cursor);
    } catch {
      if (opts.isCancelled()) return { stop: "cancelled", pagesFetched };
      return { stop: "failed", pagesFetched };
    }
    if (opts.isCancelled()) return { stop: "cancelled", pagesFetched };
    if (!win || win.source !== "transcript") return { stop: "failed", pagesFetched };

    const nextCursor = win.older_cursor ?? null;
    if (win.reset) {
      if (nextCursor === cursor) {
        return { stop: opts.isHistoryPreview ? "unsupported-old-server" : "failed", pagesFetched };
      }
      opts.onReset(win);
      return { stop: "reset", pagesFetched };
    }
    if (nextCursor === cursor) return { stop: "failed", pagesFetched };

    const incoming = newlyPrependedMessages(opts.getMessages(), win);
    pagesFetched += 1;
    opts.onPage(win, incoming);
    if (hasUserPrompt(incoming)) return { stop: "found-user-prompt", pagesFetched };
    cursor = nextCursor;
  }
}
