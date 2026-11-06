/**
 * 托管会话里跑着的 coding agent。
 *
 * 两件事:在会话列表上标出它在干什么,以及在它等人拍板时让用户就地回答。
 *
 * 状态的**增量**来自 `agent-event` 控制消息广播,**全量**来自
 * `GET /api/agents/sessions` —— 页面加载时和每次重连后都要拉一次全量(协议 A5:
 * 不做增量对账,断线期间漏掉的事件靠这次拉取补齐)。
 *
 * 回答走 `POST /api/agents/answer` 而不是控制消息:新增 client→server 消息类型
 * 按协议 X3 是破坏性变更,要 bump proto 并让所有已打开的页面全断。
 *
 * 纯逻辑(状态归并、取哪个 agent 会话代表这个终端、状态图标)与 DOM 分开,
 * 这样它们能被 node:test 直接跑。
 */
import { type MsgKey, t } from "./i18n";

export type AgentState = "idle" | "thinking" | "working" | "waiting" | "done" | "error";

export interface AgentSession {
  agent_id: string;
  agent_session_id: string;
  /** 关联到的 gateterm 托管会话 id */
  session: number | null;
  state: AgentState;
  detail: string | null;
  /** 有值 = 正在等人回答,值是回答时要带的自然键 */
  pending: string | null;
  /** 这家 agent 的对话记录能不能读(聊天视图靠它) */
  supports_transcript?: boolean;
}

export type AgentMap = Map<string, AgentSession>;

export function keyOf(s: { agent_id: string; agent_session_id: string }): string {
  return `${s.agent_id} ${s.agent_session_id}`;
}

/**
 * 把一条 `agent-event` 并进表里。
 *
 * `done` 的会话直接移除:它已经结束了,留在列表上只会让用户以为还有东西在跑。
 * 服务端的清理也会把它扫掉,这里先一步只是为了界面即时。
 */
export function applyEvent(map: AgentMap, ev: AgentSession): AgentMap {
  if (ev.state === "done") map.delete(keyOf(ev));
  // agent-event 不携带格式能力;保留 HTTP 快照已经给出的 supports_transcript。
  else map.set(keyOf(ev), { ...map.get(keyOf(ev)), ...ev });
  return map;
}

export function replaceAll(map: AgentMap, list: AgentSession[]): AgentMap {
  map.clear();
  for (const s of list) if (s.state !== "done") map.set(keyOf(s), s);
  return map;
}

/**
 * 状态优先级。一个托管会话里可能同时有多个 agent 会话(开了 subagent,或者用户
 * 在同一个终端里换了个 agent),列表上只有一个位置,得挑一个最该被看见的。
 *
 * `waiting` 永远排第一 —— 它是唯一需要用户**动手**的状态。
 */
const PRIORITY: Record<AgentState, number> = {
  waiting: 5,
  error: 4,
  working: 3,
  thinking: 2,
  idle: 1,
  done: 0,
};

/** 这个托管会话该显示哪个 agent 的状态。没有 agent 就返回 null。 */
export function forSession(map: AgentMap, sessionId: number): AgentSession | null {
  let best: AgentSession | null = null;
  for (const s of map.values()) {
    if (s.session !== sessionId) continue;
    if (!best || PRIORITY[s.state] > PRIORITY[best.state]) best = s;
  }
  return best;
}

/**
 * 聊天视图该看哪个 agent。
 *
 * 和 `forSession` 不同:那个按**状态**优先级挑(谁最该被看见),这个按**能不能读记录**
 * 挑。一个托管会话里先后跑过两家 agent 是常事(先 codex 后 claude),而按状态挑出来
 * 的那个未必是能看聊天的那个 —— 结果就是明明终端里有输出,聊天视图却说「这个 AI 的
 * 对话记录暂时还读不了」。读得了的优先;都读不了才回退到状态最高的那个,好让回落
 * 文案仍能说出是哪一家。
 */
export function chatAgentFor(map: AgentMap, sessionId: number): AgentSession | null {
  let readable: AgentSession | null = null;
  for (const s of map.values()) {
    if (s.session !== sessionId || !s.supports_transcript) continue;
    if (!readable || PRIORITY[s.state] > PRIORITY[readable.state]) readable = s;
  }
  return readable ?? forSession(map, sessionId);
}

/** 聊天入口既需要本会话的格式支持,也需要服务端明确允许读 transcript。 */
export function chatAvailable(map: AgentMap, sessionId: number | null, transcriptEnabled: boolean): boolean {
  return transcriptEnabled && sessionId !== null && chatAgentFor(map, sessionId)?.supports_transcript === true;
}

/** 所有正在等人回答的。按托管会话 id 排序,让列表稳定不跳。 */
export function waitingList(map: AgentMap): AgentSession[] {
  return [...map.values()]
    .filter((s) => s.pending)
    .sort((a, b) => (a.session ?? 0) - (b.session ?? 0));
}


/**
 * 状态 → 词条键。**显式写全**,不用模板字面量拼 —— 拼出来的键既躲开了 `MsgKey`
 * 的类型检查(只能 `as never`),也躲开了「没人引用的死词条」那条测试,等于把两道
 * 防线一起绕过去了。
 */
const STATE_KEY: Record<AgentState, MsgKey> = {
  idle: "agent.state.idle",
  thinking: "agent.state.thinking",
  working: "agent.state.working",
  waiting: "agent.state.waiting",
  done: "agent.state.done",
  error: "agent.state.error",
};

/**
 * `Notification` 事件的 `notification_type` 是机器串(`permission_prompt` 之类),
 * 直接显示在中文界面里很糙。认得出的翻译掉,认不出的原样返回 —— 因为 `detail`
 * 这个字段是复用的:它也可能是工具名(`Bash`、`Edit`),那种原样显示才对。
 */
const NOTIFICATION_KEY: Record<string, MsgKey> = {
  permission_prompt: "agent.notice.permission",
  idle_prompt: "agent.notice.idle",
  agent_needs_input: "agent.notice.input",
  elicitation_dialog: "agent.notice.choice",
};

function detailText(detail: string | null): string | null {
  if (!detail) return null;
  const key = NOTIFICATION_KEY[detail];
  return key ? t(key) : detail;
}

/**
 * 给人看的一行。
 *
 * `waiting` 有**两种**来源,必须分开说:有 `pending` 的是「这里能回答」,没有的是
 * `Notification` 报来的「它在终端那边等你」。混成同一句话的话,后者会显示成一个
 * 醒目的「等你回答」,而用户点遍界面也找不到能回答的地方。
 */
export function stateText(s: AgentSession): string {
  const key = s.state === "waiting" && !s.pending ? "agent.state.waitingTerminal" : STATE_KEY[s.state];
  const name = t(key);
  const detail = detailText(s.detail);
  return detail ? `${name} · ${detail}` : name;
}

/* ---------- HTTP ---------- */

function authHeaders(token: string): HeadersInit {
  return { Authorization: `Bearer ${token}` };
}

/**
 * 全量快照。任何一步失败都返回空会话表并关闭聊天入口 —— 服务端可能没有这个路由的
 * 旧版本(协议 T12:客户端必须容忍 `/api/` 路由缺失,降级而不是卡住)。
 */
export interface AgentSessions {
  sessions: AgentSession[];
  transcript_enabled: boolean;
}

export async function fetchAgentSessions(token: string): Promise<AgentSessions> {
  const unavailable = { sessions: [], transcript_enabled: false };
  try {
    const r = await fetch("/api/agents/sessions", { headers: authHeaders(token) });
    if (!r.ok) return unavailable;
    const body = await r.json();
    return {
      sessions: Array.isArray(body?.sessions) ? body.sessions : [],
      // 旧服务端、坏响应或未成功取得权限时一律隐藏入口。
      transcript_enabled: body?.transcript_enabled === true,
    };
  } catch {
    return unavailable;
  }
}

export type AnswerOutcome = "ok" | "gone" | "already" | "failed";

/**
 * 替 agent 拍板。
 *
 * 三种失败要分开:`gone` = 这个请求已经不在了(agent 自己超时了或断了),
 * `already` = 别的客户端抢先答过了(多设备同时看着同一台机器,这是常态,
 * 不是错误)。
 */
export async function answerPending(
  token: string,
  pendingId: string,
  decision: "allow" | "deny",
  message?: string,
  /** 回答选择题时带上:原入参 + 以问题原文为键的 answers(见 `buildChoiceAnswer`)。 */
  updatedInput?: unknown,
): Promise<AnswerOutcome> {
  try {
    const r = await fetch("/api/agents/answer", {
      method: "POST",
      headers: { ...authHeaders(token), "Content-Type": "application/json" },
      body: JSON.stringify({
        pending_id: pendingId,
        decision,
        message,
        updated_input: updatedInput,
      }),
    });
    if (r.ok) return "ok";
    if (r.status === 404) return "gone";
    if (r.status === 409) return "already";
    return "failed";
  } catch {
    return "failed";
  }
}

/* ---------- 安装(设置页用) ---------- */

export interface AgentStatus {
  id: string;
  name: string;
  detected: { installed: boolean; confidence: string; reason: string; detail: string };
  /** "not-installed" | "installed" | "stale-port" | "outdated" | "needs-activation" */
  integration: string;
  /**
   * 装完还需要用户做什么才生效。机器可读的键,文案在客户端。
   * `null` = 装完即生效。
   */
  activation: string | null;
  /** 同一事件上别家的阻塞式 hook。不是错误,但要让用户看见 */
  conflicts: string[];
}

/** 路由缺失/报错一律返回空清单(协议 T12),设置页显示「没找到」而不是崩掉。 */
export async function fetchAgents(token: string): Promise<AgentStatus[]> {
  try {
    const r = await fetch("/api/agents", { headers: authHeaders(token) });
    if (!r.ok) return [];
    const body = await r.json();
    return Array.isArray(body?.agents) ? body.agents : [];
  } catch {
    return [];
  }
}

export type InstallOutcome = "ok" | "disabled" | "conflict" | "failed";

/**
 * 装 / 卸 hook。
 *
 * `disabled`(403)不是错误,是「服务端把这个能力关着」—— 默认是开的,但部署方
 * 可以在 config.toml 里关掉。UI 要把它当成一句说明,而不是一条失败。
 */
export async function setAgentIntegration(
  token: string,
  id: string,
  install: boolean,
): Promise<InstallOutcome> {
  try {
    const r = await fetch(`/api/agents/${encodeURIComponent(id)}/${install ? "install" : "uninstall"}`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (r.ok) return "ok";
    if (r.status === 403) return "disabled";
    if (r.status === 409) return "conflict";
    return "failed";
  } catch {
    return "failed";
  }
}

/**
 * 清空:删掉这家配置里**所有** gateterm 写过的 hook —— 不论端口、不论事件。
 *
 * 卸载只认当前版本托管的那些事件;旧端口、旧版本、异常中断留下的残留只有这个
 * 清得掉。服务端不对它做 `install_enabled` 门控(只删我们自己的东西),改前照样备份。
 */
export async function purgeAgentHooks(token: string, id: string): Promise<InstallOutcome | "unchanged"> {
  try {
    const r = await fetch(`/api/agents/${encodeURIComponent(id)}/purge`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (r.ok) return (await r.json()).changed === false ? "unchanged" : "ok";
    if (r.status === 409) return "conflict";
    return "failed";
  } catch {
    return "failed";
  }
}

/** 一条将要发生的改动。`spec` 是要写进去的 hook 原文,预演时原样展示。 */
export interface PlanEdit {
  path: string;
  action: "ensure" | "remove";
  event: string;
  spec: unknown;
}

export interface AgentPlan {
  install: PlanEdit[];
  uninstall: PlanEdit[];
  /** 是否包含决策类 hook。检测到别家阻塞式 hook 时服务端会自动置 false */
  include_blocking: boolean;
  install_enabled: boolean;
}

/**
 * 预演:装这一下到底会改什么。
 *
 * 这是只读的,和 install 走同一份计划 —— 「先看后装」不是另一条代码路径,而是
 * 同一条路径的干跑。改的是**用户的**配置文件,不给看一眼就动手是不合适的。
 */
export async function fetchAgentPlan(token: string, id: string): Promise<AgentPlan | null> {
  try {
    const r = await fetch(`/api/agents/${encodeURIComponent(id)}/plan`, {
      headers: authHeaders(token),
    });
    if (!r.ok) return null;
    return (await r.json()) as AgentPlan;
  } catch {
    return null;
  }
}

/**
 * 把计划压成给人看的几行:哪个文件、涉及哪些事件。
 *
 * 不展开 hook 的完整 JSON —— 那是给机器看的,一屏塞不下,而用户真正要判断的是
 * 「你要动我哪个文件、动几处」。
 */
export function summarizePlan(edits: PlanEdit[]): { path: string; events: string[] }[] {
  const byPath = new Map<string, string[]>();
  for (const e of edits) {
    const list = byPath.get(e.path) ?? [];
    list.push(e.event);
    byPath.set(e.path, list);
  }
  return [...byPath.entries()].map(([path, events]) => ({ path, events }));
}

/** 一条挂起请求的完整详情。**决定要不要放行,靠的就是这里面的东西。** */
export interface PendingDetail {
  id: string;
  agent_id: string;
  agent_session_id: string;
  session: number | null;
  tool_name: string | null;
  /** 工具入参原文。命令、文件路径都在这儿 —— 不展示它就等于让人盲签。 */
  tool_input: unknown;
  created_at: number;
  /** unix 秒。服务端按自己的超时配置算好,客户端不猜。 */
  expires_at: number;
}

/**
 * 拉挂起请求的详情。
 *
 * 为什么不把这些塞进 `agent-event` 广播:控制通道有 4 KiB 上限,而且协议 R4 明确
 * 不许在那里走大块数据 —— 命令原文可以很长。广播只负责说「有事了」,详情走 HTTP,
 * 和会话全量快照(A5)是同一个路子。
 */
export async function fetchPending(token: string): Promise<PendingDetail[]> {
  try {
    const r = await fetch("/api/agents/pending", { headers: authHeaders(token) });
    if (!r.ok) return [];
    const body = await r.json();
    return Array.isArray(body?.pending) ? body.pending : [];
  } catch {
    return [];
  }
}

/**
 * 把工具入参压成一行给人看的命令。
 *
 * 优先取 `command`(shell 类工具),否则 `file_path`,再否则整个 JSON。**不截断** ——
 * 截断正是「看不清自己在批准什么」的另一种形式,长就让它自己滚动。
 */
export function describeToolInput(input: unknown): string {
  if (input == null) return "";
  if (typeof input === "string") return input;
  if (typeof input === "object") {
    const o = input as Record<string, unknown>;
    for (const key of ["command", "file_path", "path", "url"]) {
      if (typeof o[key] === "string") return o[key] as string;
    }
  }
  try {
    return JSON.stringify(input, null, 2);
  } catch {
    return String(input);
  }
}

/** 还剩多少秒。已过期返回 0。 */
export function secondsLeft(expiresAt: number, nowSecs: number): number {
  return Math.max(0, expiresAt - nowSecs);
}

/* ---------- 选择题 ---------- */

export interface ChoiceOption {
  label: string;
  description?: string;
}

export interface ChoiceQuestion {
  question: string;
  header?: string;
  multiSelect?: boolean;
  options: ChoiceOption[];
}

/** 上界。超了就不渲染,回落到「去终端回答」—— 终端里那套 UI 本来就处理得了。 */
const MAX_QUESTIONS = 5;
const MAX_OPTIONS = 8;

/**
 * 认出一道选择题。
 *
 * `AskUserQuestion` 走的也是 `PermissionRequest` 通道,只是它要的不是 allow/deny,
 * 而是「选哪个」。形状不对或超限一律返回 `null`,由调用方回落到普通授权 UI 或
 * 「去这个会话」—— **绝不半渲染**:选项少显示一个,用户就会在看不见的选项上做决定。
 */
export function parseChoice(toolName: string | null, toolInput: unknown): ChoiceQuestion[] | null {
  if (toolName !== "AskUserQuestion") return null;
  const input = toolInput as { questions?: unknown } | null;
  const raw = input && Array.isArray(input.questions) ? input.questions : null;
  if (!raw || raw.length === 0 || raw.length > MAX_QUESTIONS) return null;

  const out: ChoiceQuestion[] = [];
  for (const q of raw as Record<string, unknown>[]) {
    const question = typeof q?.question === "string" ? q.question : null;
    const options = Array.isArray(q?.options) ? (q.options as Record<string, unknown>[]) : null;
    if (!question || !options || options.length === 0 || options.length > MAX_OPTIONS) return null;
    const parsed: ChoiceOption[] = [];
    for (const o of options) {
      if (typeof o?.label !== "string" || !o.label) return null;
      parsed.push({
        label: o.label,
        description: typeof o.description === "string" ? o.description : undefined,
      });
    }
    out.push({
      question,
      header: typeof q.header === "string" ? q.header : undefined,
      multiSelect: q.multiSelect === true,
      options: parsed,
    });
  }
  // 同一句问题出现两次的话,以问题原文为键的 answers 会互相覆盖 —— 宁可不渲染
  if (new Set(out.map((q) => q.question)).size !== out.length) return null;
  return out;
}

/**
 * 构造回传的 `updatedInput`:原入参 + `answers`,**以问题原文为键**。
 *
 * 键必须是原文这件事有个坑:界面上问题可能很长,想截断显示是很自然的念头 ——
 * 但一旦拿截断后的文本当键,答案就对不上任何一个问题,会被 agent 静默丢掉。
 * 所以这里只接受原文,显示层的省略号交给 CSS 去做。
 */
export function buildChoiceAnswer(
  toolInput: unknown,
  answers: Record<string, string>,
): Record<string, unknown> {
  const input = (toolInput && typeof toolInput === "object" ? toolInput : {}) as Record<
    string,
    unknown
  >;
  return { ...input, answers };
}


/* ---------- 品牌图标 (组件化单点真相来源) ---------- */

let brandIconSeq = 0;

export function agentBrandLabel(agentId: string): string {
  const lower = agentId.toLowerCase();
  if (lower.includes("claude")) return "Claude";
  if (lower.includes("codex")) return "Codex";
  if (lower.includes("opencode")) return "OpenCode";
  if (lower.includes("copilot")) return "Copilot";
  if (lower === "pi" || lower.startsWith("pi-")) return "pi";
  if (lower.includes("grok")) return "Grok";
  if (lower.includes("agy") || lower.includes("antigravity")) return "AGY";
  return agentId || "Agent";
}

export function agentBrandIconSvg(agentId: string): string {
  const lower = agentId.toLowerCase();
  const gid = `gt-ag-${++brandIconSeq}`;
  if (lower.includes("claude")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M 4.709 15.955 L 9.429 13.308 L 9.509 13.078 L 9.429 12.95 L 9.2 12.95 L 8.41 12.902 L 5.712 12.829 L 3.373 12.732 L 1.107 12.61 L 0.536 12.489 L 0 11.784 L 0.055 11.432 L 0.535 11.111 L 1.221 11.171 L 2.741 11.274 L 5.019 11.432 L 6.671 11.529 L 9.12 11.784 L 9.509 11.784 L 9.564 11.627 L 9.43 11.529 L 9.327 11.432 L 6.969 9.836 L 4.417 8.148 L 3.081 7.176 L 2.357 6.685 L 1.993 6.223 L 1.835 5.215 L 2.491 4.493 L 3.372 4.553 L 3.597 4.614 L 4.49 5.3 L 6.398 6.776 L 8.889 8.609 L 9.254 8.913 L 9.399 8.81 L 9.418 8.737 L 9.254 8.463 L 7.899 6.017 L 6.453 3.527 L 5.809 2.495 L 5.639 1.876 C 5.574 1.638 5.539 1.393 5.535 1.147 L 6.283 0.134 L 6.696 0 L 7.692 0.134 L 8.112 0.498 L 8.732 1.912 L 9.734 4.141 L 11.289 7.171 L 11.745 8.069 L 11.988 8.901 L 12.079 9.156 L 12.237 9.156 L 12.237 9.01 L 12.365 7.304 L 12.602 5.209 L 12.832 2.514 L 12.912 1.754 L 13.288 0.844 L 14.035 0.352 L 14.619 0.632 L 15.099 1.317 L 15.032 1.761 L 14.746 3.612 L 14.187 6.515 L 13.823 8.457 L 14.035 8.457 L 14.278 8.215 L 15.263 6.909 L 16.915 4.845 L 17.645 4.025 L 18.495 3.121 L 19.042 2.69 L 20.075 2.69 L 20.835 3.819 L 20.495 4.985 L 19.431 6.332 L 18.55 7.474 L 17.286 9.174 L 16.496 10.534 L 16.569 10.644 L 16.757 10.624 L 19.613 10.018 L 21.156 9.738 L 22.997 9.423 L 23.83 9.811 L 23.921 10.206 L 23.593 11.013 L 21.624 11.499 L 19.315 11.961 L 15.876 12.774 L 15.834 12.804 L 15.883 12.865 L 17.432 13.011 L 18.094 13.047 L 19.716 13.047 L 22.736 13.272 L 23.526 13.794 L 24 14.432 L 23.921 14.917 L 22.706 15.537 L 21.066 15.148 L 17.237 14.238 L 15.925 13.909 L 15.743 13.909 L 15.743 14.019 L 16.836 15.087 L 18.842 16.897 L 21.351 19.227 L 21.478 19.805 L 21.156 20.26 L 20.816 20.211 L 18.611 18.554 L 17.76 17.807 L 15.834 16.187 L 15.706 16.187 L 15.706 16.357 L 16.15 17.006 L 18.495 20.527 L 18.617 21.607 L 18.447 21.96 L 17.839 22.173 L 17.171 22.051 L 15.797 20.126 L 14.382 17.959 L 13.239 16.016 L 13.099 16.096 L 12.425 23.35 L 12.109 23.72 L 11.38 24 L 10.773 23.539 L 10.451 22.792 L 10.773 21.316 L 11.162 19.392 L 11.477 17.862 L 11.763 15.962 L 11.933 15.33 L 11.921 15.288 L 11.781 15.306 L 10.347 17.273 L 8.167 20.218 L 6.441 22.063 L 6.027 22.227 L 5.31 21.857 L 5.377 21.195 L 5.778 20.606 L 8.166 17.57 L 9.606 15.688 L 10.536 14.602 L 10.53 14.444 L 10.475 14.444 L 4.132 18.56 L 3.002 18.706 L 2.515 18.25 L 2.576 17.504 L 2.807 17.261 L 4.715 15.949 L 4.709 15.955 Z"/></svg>`;
  }
  if (lower.includes("codex")) {
    return `<svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><defs><linearGradient id="${gid}" x1="12" y1="0" x2="12" y2="24" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#B1A7FF"/><stop offset="0.45" stop-color="#7A9DFF"/><stop offset="1" stop-color="#3941FF"/></linearGradient></defs><path fill="url(#${gid})" fill-rule="evenodd" d="M 8.086 0.457 C 9.049 0.061 10.098 -0.082 11.132 0.042 C 12.465 0.195 13.653 0.762 14.696 1.742 C 14.725 1.769 14.765 1.78 14.803 1.771 C 16.211 1.425 17.565 1.547 18.864 2.137 L 18.927 2.167 L 19.081 2.243 C 20.438 2.946 21.411 4.013 21.999 5.441 C 22.277 6.12 22.417 6.829 22.42 7.567 C 22.44 8.117 22.379 8.666 22.24 9.198 C 22.226 9.253 22.241 9.312 22.28 9.353 C 23.066 10.15 23.613 11.152 23.858 12.244 C 24.243 14.145 23.848 15.859 22.675 17.384 L 22.493 17.604 C 21.716 18.494 20.696 19.137 19.559 19.455 C 19.509 19.47 19.468 19.508 19.451 19.557 C 19.196 20.293 18.94 20.921 18.464 21.549 C 17.265 23.131 15.502 24.011 13.516 24 C 11.933 23.992 10.53 23.413 9.306 22.264 C 9.268 22.229 9.215 22.217 9.166 22.232 C 8.648 22.399 8.126 22.423 7.562 22.417 C 6.661 22.41 5.773 22.197 4.967 21.795 C 4.123 21.376 3.388 20.766 2.821 20.014 C 2.618 19.745 2.417 19.492 2.27 19.193 C 2.067 18.781 1.902 18.352 1.775 17.91 C 1.509 16.907 1.503 15.852 1.758 14.846 C 1.766 14.822 1.769 14.797 1.766 14.772 C 1.761 14.747 1.748 14.725 1.729 14.708 C 1.113 14.084 0.641 13.333 0.349 12.506 C 0.155 11.997 0.043 11.461 0.016 10.917 C -0.032 10.201 0.031 9.482 0.204 8.785 C 0.654 7.301 1.513 6.137 2.781 5.292 C 3.063 5.104 3.331 4.958 3.583 4.854 C 3.869 4.734 4.156 4.634 4.444 4.55 C 4.486 4.538 4.519 4.505 4.531 4.463 C 4.749 3.678 5.125 2.945 5.635 2.31 C 6.315 1.464 7.132 0.846 8.086 0.457 Z M 7.282 8.307 C 7.049 7.9 6.531 7.759 6.125 7.992 C 5.718 8.224 5.576 8.742 5.809 9.149 L 7.503 12.114 L 5.815 14.962 C 5.6 15.363 5.739 15.862 6.131 16.094 C 6.522 16.326 7.027 16.207 7.275 15.826 L 9.215 12.554 C 9.371 12.291 9.373 11.965 9.222 11.7 L 7.282 8.307 Z M 12.728 14.547 C 12.279 14.574 11.929 14.945 11.929 15.395 C 11.929 15.844 12.279 16.215 12.728 16.242 L 17.576 16.242 C 18.028 16.22 18.384 15.847 18.384 15.394 C 18.384 14.941 18.028 14.568 17.576 14.546 L 12.728 14.546 Z"/></svg>`;
  }
  if (lower.includes("opencode")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M 16 6 L 8 6 L 8 18 L 16 18 L 16 6 Z M 20 22 L 4 22 L 4 2 L 20 2 L 20 22 Z"/></svg>`;
  }
  if (lower.includes("copilot")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M 23.922 16.992 C 23.061 18.495 18.067 22.012 12 22.012 C 5.933 22.012 0.939 18.495 0.078 16.992 C 0.027 16.895 0 16.785 0 16.673 L 0 14.226 C 0 13.662 0.262 13.131 0.708 12.789 C 1.2 12.406 1.797 11.995 2.525 11.584 C 2.458 9.211 2.838 7.229 3.694 5.799 C 4.539 4.386 5.899 3.558 7.757 3.249 C 9.199 3.012 10.606 2.987 11.999 2.988 C 13.393 2.987 14.8 3.012 16.242 3.249 C 18.1 3.558 19.46 4.386 20.305 5.799 C 21.161 7.229 21.542 9.211 21.475 11.584 C 22.202 11.995 22.8 12.406 23.291 12.789 C 23.738 13.131 24 13.662 24 14.226 L 24 16.673 C 24 16.785 23.973 16.895 23.922 16.992 Z M 4.868 12.966 C 6.238 13.524 8.226 13.861 11.225 13.987 L 11.225 10.228 C 9.488 10.172 8.248 9.914 7.367 9.497 C 6.283 8.983 5.736 8.211 5.367 7.121 C 4.94 8.352 4.764 10.258 4.868 12.966 Z M 12.775 13.987 C 15.774 13.861 17.762 13.524 19.132 12.966 C 19.236 10.258 19.06 8.352 18.633 7.121 C 18.264 8.211 17.717 8.983 16.633 9.497 C 15.752 9.914 14.512 10.172 12.775 10.228 L 12.775 13.987 Z M 9.25 16.25 C 8.56 16.25 8 16.81 8 17.5 C 8 18.19 8.56 18.75 9.25 18.75 C 9.94 18.75 10.5 18.19 10.5 17.5 C 10.5 16.81 9.94 16.25 9.25 16.25 Z M 14.75 16.25 C 14.06 16.25 13.5 16.81 13.5 17.5 C 13.5 18.19 14.06 18.75 14.75 18.75 C 15.44 18.75 16 18.19 16 17.5 C 16 16.81 15.44 16.25 14.75 16.25 Z"/></svg>`;
  }
  if (lower === "pi" || lower.startsWith("pi-")) {
    return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M 4.5 6.75 L 19.5 6.75 M 9.375 6.75 L 9.375 17.625 M 15.375 6.75 L 15.375 15.375 C 15.375 16.618 16.382 17.625 17.625 17.625"/></svg>`;
  }
  if (lower.includes("grok")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M 9.27 15.29 L 17.248 9.393 C 17.639 9.103 18.198 9.216 18.385 9.665 C 19.365 12.034 18.927 14.88 16.975 16.834 C 15.024 18.788 12.308 19.216 9.826 18.24 L 7.115 19.497 C 11.004 22.158 15.726 21.5 18.677 18.544 C 21.018 16.2 21.743 13.005 21.065 10.124 L 21.071 10.131 C 20.088 5.899 21.313 4.207 23.821 0.748 C 23.881 0.666 23.941 0.584 24 0.5 L 20.699 3.805 L 20.699 3.795 L 9.267 15.292 Z M 7.623 16.723 C 4.831 14.053 5.313 9.922 7.694 7.539 C 9.455 5.776 12.341 5.056 14.86 6.114 L 17.565 4.864 C 17.003 4.45 16.388 4.114 15.736 3.864 C 12.386 2.493 8.541 3.268 5.984 5.83 C 3.451 8.366 2.654 12.266 4.022 15.594 C 5.044 18.081 3.369 19.84 1.682 21.616 C 1.083 22.246 0.483 22.875 0 23.541 L 7.62 16.726 Z"/></svg>`;
  }
  if (lower.includes("agy") || lower.includes("antigravity")) {
    const agyPath =
      "M 21.751 22.607 C 23.091 23.612 25.101 22.942 23.259 21.099 C 17.73 15.74 18.904 1 12.037 1 C 5.17 1 6.342 15.74 0.815 21.1 C -1.195 23.109 0.982 23.611 2.322 22.606 C 7.514 19.089 7.179 12.892 12.037 12.892 C 16.894 12.892 16.559 19.089 21.751 22.607 Z";
    return `<svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><path fill="#3186FF" fill-rule="evenodd" d="${agyPath}"/><mask id="${gid}-m" maskUnits="userSpaceOnUse" x="-1" y="0" width="26" height="24"><path fill="#fff" fill-rule="evenodd" d="${agyPath}"/></mask><g mask="url(#${gid}-m)"><g filter="url(#${gid}-f1)"><path d="M-1.018-3.992c-.408 3.591 2.686 6.89 6.91 7.37 4.225.48 7.98-2.043 8.387-5.633.408-3.59-2.686-6.89-6.91-7.37-4.225-.479-7.98 2.043-8.387 5.633z" fill="#FFE432"/></g><g filter="url(#${gid}-f2)"><path d="M15.269 7.747c1.058 4.557 5.691 7.374 10.348 6.293 4.657-1.082 7.575-5.653 6.516-10.21-1.058-4.556-5.691-7.374-10.348-6.292-4.657 1.082-7.575 5.653-6.516 10.21z" fill="#FC413D"/></g><g filter="url(#${gid}-f3)"><path d="M-12.443 10.804c1.338 4.703 7.36 7.11 13.453 5.378 6.092-1.733 9.947-6.95 8.61-11.652C8.282-.173 2.26-2.58-3.833-.848-9.925.884-13.78 6.1-12.443 10.804z" fill="#00B95C"/></g><g filter="url(#${gid}-f4)"><path d="M-12.443 10.804c1.338 4.703 7.36 7.11 13.453 5.378 6.092-1.733 9.947-6.95 8.61-11.652C8.282-.173 2.26-2.58-3.833-.848-9.925.884-13.78 6.1-12.443 10.804z" fill="#00B95C"/></g><g filter="url(#${gid}-f5)"><path d="M-7.608 14.703c3.352 3.424 9.126 3.208 12.896-.483 3.77-3.69 4.108-9.459.756-12.883C2.69-2.087-3.083-1.871-6.853 1.82c-3.77 3.69-4.108 9.458-.755 12.883z" fill="#00B95C"/></g><g filter="url(#${gid}-f6)"><path d="M9.932 27.617c1.04 4.482 5.384 7.303 9.7 6.3 4.316-1.002 6.971-5.448 5.93-9.93-1.04-4.483-5.384-7.304-9.7-6.301-4.316 1.002-6.971 5.448-5.93 9.93z" fill="#3186FF"/></g><g filter="url(#${gid}-f7)"><path d="M2.572-8.185C.392-3.329 2.778 2.472 7.9 4.771c5.122 2.3 11.042.227 13.222-4.63 2.18-4.855-.205-10.656-5.327-12.955-5.122-2.3-11.042-.227-13.222 4.63z" fill="#FBBC04"/></g><g filter="url(#${gid}-f8)"><path d="M-3.267 38.686c-5.277-2.072 3.742-19.117 5.984-24.83 2.243-5.712 8.34-8.664 13.616-6.592 5.278 2.071 11.533 13.482 9.29 19.195-2.242 5.713-23.613 14.298-28.89 12.227z" fill="#3186FF"/></g><g filter="url(#${gid}-f9)"><path d="M28.71 17.471c-1.413 1.649-5.1.808-8.236-1.878-3.135-2.687-4.531-6.201-3.118-7.85 1.412-1.649 5.1-.808 8.235 1.878s4.532 6.2 3.119 7.85z" fill="#749BFF"/></g><g filter="url(#${gid}-f10)"><path d="M18.163 9.077c5.81 3.93 12.502 4.19 14.946.577 2.443-3.612-.287-9.727-6.098-13.658-5.81-3.931-12.502-4.19-14.946-.577-2.443 3.612.287 9.727 6.098 13.658z" fill="#FC413D"/></g><g filter="url(#${gid}-f11)"><path d="M-.915 2.684c-1.44 3.473-.97 6.967 1.05 7.804 2.02.837 4.824-1.3 6.264-4.772 1.44-3.473.97-6.967-1.05-7.804-2.02-.837-4.824 1.3-6.264 4.772z" fill="#FFEE48"/></g></g><defs><filter id="${gid}-f1" x="-3.288" y="-11.917" width="19.838" height="17.587" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="1.117"/></filter><filter id="${gid}-f2" x="4.251" y="-13.493" width="38.9" height="38.565" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="5.4"/></filter><filter id="${gid}-f3" x="-21.889" y="-10.592" width="40.955" height="36.517" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="4.591"/></filter><filter id="${gid}-f4" x="-21.889" y="-10.592" width="40.955" height="36.517" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="4.591"/></filter><filter id="${gid}-f5" x="-19.099" y="-10.278" width="36.632" height="36.595" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="4.591"/></filter><filter id="${gid}-f6" x=".981" y="8.758" width="33.533" height="34.087" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="4.363"/></filter><filter id="${gid}-f7" x="-6.143" y="-21.659" width="35.978" height="35.276" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="3.954"/></filter><filter id="${gid}-f8" x="-11.96" y="-.46" width="45.114" height="46.523" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="3.531"/></filter><filter id="${gid}-f9" x="10.485" y=".58" width="25.094" height="24.054" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="3.159"/></filter><filter id="${gid}-f10" x="5.833" y="-12.467" width="33.508" height="30.007" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="2.669"/></filter><filter id="${gid}-f11" x="-8.355" y="-8.876" width="22.194" height="26.151" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="3.303"/></filter></defs></svg>`;
  }
  return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4.5" y="6.75" width="15" height="12" rx="3"/><path d="M12 3v3.75M9 12.375h.015M15 12.375h.015M9 15.75h6"/></svg>`;
}

/** 统一、可复用的 Agent 品牌图标节点,供侧边栏、会话卡片、历史会话列表共用。 */
export function createAgentBrandIcon(agentId: string): HTMLElement {
  const icon = document.createElement("span");
  icon.className = "aicon";
  icon.dataset.agent = agentId.toLowerCase();
  const brand = agentBrandLabel(agentId);
  icon.innerHTML = agentBrandIconSvg(agentId);
  icon.title = brand;
  icon.setAttribute("aria-label", brand);
  return icon;
}
