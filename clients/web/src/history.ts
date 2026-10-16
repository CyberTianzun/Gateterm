import { type ChatWindow } from "./chat";
import { type MsgKey, t } from "./i18n";

export interface HistoryEntry {
  agent_id: string;
  agent_name: string;
  agent_session_id: string;
  title: string;
  cwd: string | null;
  updated_at: number;
  terminal_session: number | null;
  can_resume: boolean;
  unavailable_reason: string | null;
}

export interface HistoryPage {
  sessions: HistoryEntry[];
  agents: { id: string; name: string; installed: boolean }[];
  total: number;
  next_offset: number | null;
  truncated: boolean;
  skipped: number;
  indexing?: boolean;
  max_sessions?: number;
}

const ERRORS: Record<string, MsgKey> = {
  disabled: "history.disabled",
  "missing-directory": "history.missingDirectory",
  "missing-cli": "history.missingCli",
  "not-found": "history.notFound",
  "unsupported-agent": "history.unsupported",
  "spawn-failed": "history.spawnFailed",
  unreadable: "history.unreadable",
};

export function historyError(reason: string): string {
  return t(ERRORS[reason] ?? "history.unreadable");
}

export class HistoryError extends Error {
  constructor(public readonly reason: string) { super(reason); }
}

async function request(token: string, path: string, init: RequestInit = {}): Promise<any> {
  try {
    const response = await fetch(`/api/agents/history${path}`, {
      ...init,
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    });
    if (!response.ok) {
      const body = await response.json().catch(() => null);
      throw new HistoryError(body?.error ?? "unreadable");
    }
    return await response.json();
  } catch (error) {
    if (error instanceof HistoryError) throw error;
    throw new HistoryError("unreadable");
  }
}

export function fetchHistory(token: string, q = "", agent = "", offset = 0, refresh = false): Promise<HistoryPage> {
  const params = new URLSearchParams({ q, agent_id: agent, offset: String(offset), refresh: String(refresh) });
  return request(token, `?${params}`);
}

function key(entry: HistoryEntry): { agent_id: string; agent_session_id: string } {
  return { agent_id: entry.agent_id, agent_session_id: entry.agent_session_id };
}

export async function fetchHistoryMessages(token: string, entry: HistoryEntry): Promise<ChatWindow> {
  const window = await request(token, `/messages?${new URLSearchParams(key(entry))}`);
  if (window?.source !== "transcript" || !Array.isArray(window.messages)) throw new HistoryError("unreadable");
  return window;
}

export function openHistory(token: string, entry: HistoryEntry): Promise<{ session: { id: number; name: string }; reused: boolean }> {
  return request(token, "/open", { method: "POST", body: JSON.stringify(key(entry)) });
}
