/**
 * Pure-logic MCP endpoint derivation, token masking, preset generation, and
 * server capability probing for the Web client (zero DOM dependency).
 */

export interface McpEndpoints {
  mcpUrl: string;
  apiBaseUrl: string;
}

export interface McpServerIdentity {
  instanceId?: string;
  name?: string;
  hostname?: string;
  os?: string;
  arch?: string;
  port?: number;
  description?: string;
}

export interface McpPresets {
  serverName: string;
  mcpUrl: string;
  authHeader: string;
  genericJson: string;
  claudeCodeCmd: string;
  codexToml: string;
  curlExamples: string;
}

export type McpStatusResult =
  | { state: "enabled"; protocolVersions: string[]; server?: McpServerIdentity }
  | { state: "disabled"; server?: McpServerIdentity }
  | { state: "unsupported" }
  | { state: "error" };

/**
 * Derive the Streamable HTTP MCP endpoint and REST API base URL from a page
 * location href, stripping `?search` and `#hash` while preserving reverse-proxy
 * subpath prefixes (e.g. `/gateterm/`).
 */
export function deriveEndpoints(href: string): McpEndpoints {
  const mcpUrl = new URL("./mcp", href).toString();
  const apiBaseUrl = new URL(".", href).toString().replace(/\/$/, "");
  return { mcpUrl, apiBaseUrl };
}

/**
 * Mask a Bearer token for safe on-screen preview:
 * - empty -> `""`
 * - length >= 8 -> first 4 + `••••` + last 4
 * - 1..7 chars -> `••••••••`
 */
export function maskToken(token: string): string {
  if (!token) return "";
  if (token.length >= 8) {
    return `${token.slice(0, 4)}••••${token.slice(-4)}`;
  }
  return "••••••••";
}

/**
 * Wrap a string in POSIX shell single quotes (`'...'`), escaping embedded
 * single quotes as `'\''` so `$`, `"`, `\`, and whitespace are never expanded.
 */
export function posixSingleQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

/**
 * Escape a string for inclusion inside a TOML basic string (`"..."`).
 */
export function escapeTomlBasicString(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    const code = s.charCodeAt(i);
    if (ch === "\\") out += "\\\\";
    else if (ch === '"') out += '\\"';
    else if (ch === "\b") out += "\\b";
    else if (ch === "\t") out += "\\t";
    else if (ch === "\n") out += "\\n";
    else if (ch === "\f") out += "\\f";
    else if (ch === "\r") out += "\\r";
    else if (code < 0x20 || code === 0x7f) {
      out += `\\u${code.toString(16).padStart(4, "0")}`;
    } else {
      out += ch;
    }
  }
  return out;
}

const MAX_SERVER_KEY_LEN = 48;
const BARE_KEY_RE = /^[A-Za-z0-9_-]+$/;

/**
 * Normalize an arbitrary string into a lowercase `[a-z0-9_-]` slug capped at
 * 48 ASCII chars with no leading/trailing `-` or `_`. Returns `""` if empty.
 */
export function slugifyServerKey(raw: string): string {
  const lowered = raw.replace(/[A-Z]/g, (ch) => ch.toLowerCase());
  const collapsed = lowered.replace(/[^a-z0-9_-]+/g, "-");
  const trimmed = collapsed.replace(/^[-_]+|[-_]+$/g, "");
  if (!trimmed) return "";
  return trimmed.slice(0, MAX_SERVER_KEY_LEN).replace(/[-_]+$/g, "");
}

function withGatetermPrefix(slug: string): string {
  if (
    slug === "gateterm" ||
    slug.startsWith("gateterm-") ||
    slug.startsWith("gateterm_") ||
    slug.endsWith("-gateterm") ||
    slug.endsWith("_gateterm")
  ) {
    return slug.slice(0, MAX_SERVER_KEY_LEN).replace(/[-_]+$/g, "");
  }
  return `gateterm-${slug}`.slice(0, MAX_SERVER_KEY_LEN).replace(/[-_]+$/g, "");
}

function hostPortHint(baseUrl: string): string {
  if (!baseUrl) return "";
  let parsed: URL;
  try {
    parsed = new URL(baseUrl);
  } catch {
    return "";
  }
  const bareHost = parsed.hostname.replace(/^\[|\]$/g, "").toLowerCase();
  if (!bareHost) return "";
  const isLoopback = bareHost === "127.0.0.1" || bareHost === "localhost" || bareHost === "::1";
  const port = parsed.port;
  if (port && (isLoopback || (port !== "80" && port !== "443"))) {
    return `${bareHost}-${port}`;
  }
  return bareHost;
}

/**
 * Extract the server title prefix from a browser `document.title` formatted as
 * `"<prefix> - gateterm"` (optionally preceded by a `"(n) "` badge count).
 */
export function extractTitlePrefix(documentTitle: string): string {
  const stripped = documentTitle.trim().replace(/^\(\d+\)\s+/, "");
  const suffix = " - gateterm";
  if (!stripped.toLowerCase().endsWith(suffix)) {
    return "";
  }
  return stripped.slice(0, stripped.length - suffix.length).trim();
}

/**
 * Derive a conflict-free default MCP server key from ordered candidates
 * (falling back to URL host[-port] and ultimately `"gateterm"`).
 */
export function deriveMcpServerKey(
  candidates: Array<string | undefined | null>,
  baseUrl: string,
): string {
  for (const candidate of candidates) {
    if (typeof candidate !== "string") continue;
    const slug = slugifyServerKey(candidate);
    if (slug) {
      return withGatetermPrefix(slug);
    }
  }
  const hint = hostPortHint(baseUrl);
  if (hint) {
    const slug = slugifyServerKey(hint);
    if (slug) {
      return withGatetermPrefix(slug);
    }
  }
  return "gateterm";
}

/**
 * Format a key for use under `[mcp_servers.<key>]` in TOML: bare when strictly
 * `[A-Za-z0-9_-]+`, otherwise a TOML basic quoted string.
 */
export function formatTomlTableKey(key: string): string {
  if (BARE_KEY_RE.test(key)) {
    return key;
  }
  return `"${escapeTomlBasicString(key)}"`;
}

/**
 * Format a server key argument for POSIX shell commands: bare when strictly
 * `[A-Za-z0-9_-]+`, otherwise wrapped with `posixSingleQuote`.
 */
export function formatShellKeyArg(key: string): string {
  if (BARE_KEY_RE.test(key)) {
    return key;
  }
  return posixSingleQuote(key);
}

function normalizeServerName(raw?: string): string {
  if (typeof raw !== "string") return "gateterm";
  const cleaned = raw.replace(/[\r\n\t]+/g, " ").trim();
  return cleaned || "gateterm";
}

/**
 * Build the 4 copyable MCP/HTTP presets (Endpoint & Header, generic Streamable
 * HTTP JSON, Claude Code CLI + Codex TOML, and REST API curl examples).
 */
export function buildMcpPresets(opts: {
  mcpUrl: string;
  apiBaseUrl: string;
  token: string;
  serverName?: string;
  masked?: boolean;
}): McpPresets {
  const serverName = normalizeServerName(opts.serverName);
  const displayToken = opts.masked ? maskToken(opts.token) : opts.token;
  const authHeader = `Bearer ${displayToken}`;

  const genericJson = JSON.stringify(
    {
      mcpServers: Object.fromEntries([
        [
          serverName,
          {
            url: opts.mcpUrl,
            headers: {
              Authorization: authHeader,
            },
          },
        ],
      ]),
    },
    null,
    2,
  );

  const authArg = posixSingleQuote(`Authorization: ${authHeader}`);
  const claudeCodeCmd = `claude mcp add --transport http ${formatShellKeyArg(serverName)} ${posixSingleQuote(opts.mcpUrl)} --header ${authArg}`;

  const codexToml = [
    `[mcp_servers.${formatTomlTableKey(serverName)}]`,
    `url = "${escapeTomlBasicString(opts.mcpUrl)}"`,
    `http_headers = { Authorization = "${escapeTomlBasicString(authHeader)}" }`,
  ].join("\n");

  const base = opts.apiBaseUrl.replace(/\/+$/, "");
  const curlExamples = [
    "# 1. List sessions",
    `curl -sS -H ${authArg} ${posixSingleQuote(`${base}/api/sessions`)}`,
    "",
    "# 2. Read session screen",
    `curl -sS -H ${authArg} ${posixSingleQuote(`${base}/api/sessions/1/screen?scrollback_lines=50`)}`,
    "",
    "# 3. Send input and wait for output",
    `curl -sS -X POST -H ${authArg} -H 'Content-Type: application/json' -d '{"text":"git status","enter":true,"wait_ms":3000}' ${posixSingleQuote(`${base}/api/sessions/1/input`)}`,
    "",
    "# 4. List and answer pending approvals",
    `curl -sS -H ${authArg} ${posixSingleQuote(`${base}/api/agents/pending`)}`,
    `curl -sS -X POST -H ${authArg} -H 'Content-Type: application/json' -d '{"pending_id":"<pending_id>","decision":"allow"}' ${posixSingleQuote(`${base}/api/agents/answer`)}`,
  ].join("\n");

  return {
    serverName,
    mcpUrl: opts.mcpUrl,
    authHeader,
    genericJson,
    claudeCodeCmd,
    codexToml,
    curlExamples,
  };
}

function parseServerIdentity(raw: unknown): McpServerIdentity | undefined {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    return undefined;
  }
  const proto = Object.getPrototypeOf(raw);
  if (proto !== Object.prototype && proto !== null) {
    return undefined;
  }
  const obj = raw as Record<string, unknown>;
  const identity: McpServerIdentity = {};
  if (typeof obj.instance_id === "string") identity.instanceId = obj.instance_id;
  if (typeof obj.name === "string") identity.name = obj.name;
  if (typeof obj.hostname === "string") identity.hostname = obj.hostname;
  if (typeof obj.os === "string") identity.os = obj.os;
  if (typeof obj.arch === "string") identity.arch = obj.arch;
  if (typeof obj.port === "number" && Number.isFinite(obj.port)) identity.port = obj.port;
  if (typeof obj.description === "string") identity.description = obj.description;
  return identity;
}

/**
 * Probe `GET /api/mcp/status` to determine whether MCP is enabled, disabled,
 * unsupported (`404`), or unreachable/unauthorized.
 */
export async function fetchMcpStatus(
  token: string,
  fetchImpl: typeof fetch = fetch,
  apiBaseUrl = "",
): Promise<McpStatusResult> {
  const base = apiBaseUrl.replace(/\/+$/, "");
  const url = base ? `${base}/api/mcp/status` : "/api/mcp/status";
  try {
    const res = await fetchImpl(url, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (res.status === 404) {
      return { state: "unsupported" };
    }
    if (!res.ok) {
      return { state: "error" };
    }
    const body = (await res.json()) as {
      enabled?: unknown;
      protocol_versions?: unknown;
      server?: unknown;
    };
    const server = parseServerIdentity(body?.server);
    if (body?.enabled === true) {
      const protocolVersions = Array.isArray(body.protocol_versions)
        ? body.protocol_versions.filter((v): v is string => typeof v === "string")
        : [];
      return server ? { state: "enabled", protocolVersions, server } : { state: "enabled", protocolVersions };
    }
    if (body?.enabled === false) {
      return server ? { state: "disabled", server } : { state: "disabled" };
    }
    return { state: "error" };
  } catch {
    return { state: "error" };
  }
}
