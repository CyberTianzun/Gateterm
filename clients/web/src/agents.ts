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
  if (lower.includes("gemini")) return "Gemini";
  if (lower.includes("opencode")) return "OpenCode";
  if (lower === "pi" || lower.startsWith("pi-")) return "pi";
  if (lower.includes("grok")) return "Grok";
  if (lower.includes("agy") || lower.includes("antigravity")) return "AGY";
  return agentId || "Agent";
}

export function agentBrandIconSvg(agentId: string): string {
  const lower = agentId.toLowerCase();
  const gid = `gt-ag-${++brandIconSeq}`;
  if (lower.includes("claude")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"/></svg>`;
  }
  if (lower.includes("codex")) {
    return `<svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><defs><linearGradient id="${gid}" x1="12" y1="0" x2="12" y2="24" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#B1A7FF"/><stop offset="0.45" stop-color="#7A9DFF"/><stop offset="1" stop-color="#3941FF"/></linearGradient></defs><path fill="url(#${gid})" fill-rule="evenodd" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"/></svg>`;
  }
  if (lower.includes("gemini")) {
    return `<svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><defs><linearGradient id="${gid}" x1="4" y1="18" x2="20" y2="6" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#3186FF"/><stop offset="0.45" stop-color="#08B962"/><stop offset="0.75" stop-color="#FABC12"/><stop offset="1" stop-color="#F94543"/></linearGradient></defs><path fill="url(#${gid})" d="M20.616 10.835a14.147 14.147 0 01-4.45-3.001 14.111 14.111 0 01-3.678-6.452.503.503 0 00-.975 0 14.134 14.134 0 01-3.679 6.452 14.155 14.155 0 01-4.45 3.001c-.65.28-1.318.505-2.002.678a.502.502 0 000 .975c.684.172 1.35.397 2.002.677a14.147 14.147 0 014.45 3.001 14.112 14.112 0 013.679 6.453.502.502 0 00.975 0c.172-.685.397-1.351.677-2.003a14.145 14.145 0 013.001-4.45 14.113 14.113 0 016.453-3.678.503.503 0 000-.975 13.245 13.245 0 01-2.003-.678z"/></svg>`;
  }
  if (lower.includes("opencode")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M16 6H8v12h8V6zm4 16H4V2h16v20z"/></svg>`;
  }
  if (lower === "pi" || lower.startsWith("pi-")) {
    return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4.5 6.75h15M9.375 6.75v10.875M15.375 6.75v8.625a2.25 2.25 0 0 0 2.25 2.25"/></svg>`;
  }
  if (lower.includes("grok")) {
    return `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815"/></svg>`;
  }
  if (lower.includes("agy") || lower.includes("antigravity")) {
    return `<svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><defs><linearGradient id="${gid}" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#3186FF"/><stop offset="0.35" stop-color="#00B95C"/><stop offset="0.7" stop-color="#FBBC04"/><stop offset="1" stop-color="#FC413D"/></linearGradient></defs><path fill="url(#${gid})" fill-rule="evenodd" d="M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z"/></svg>`;
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
