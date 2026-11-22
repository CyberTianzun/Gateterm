/**
 * Pure-logic MCP endpoint derivation, token masking, preset generation, and
 * server capability probing for the Web client (zero DOM dependency).
 */

export interface McpEndpoints {
  mcpUrl: string;
  apiBaseUrl: string;
}

export interface McpPresets {
  mcpUrl: string;
  authHeader: string;
  genericJson: string;
  claudeCodeCmd: string;
  codexToml: string;
  curlExamples: string;
}

export type McpStatusResult =
  | { state: "enabled"; protocolVersions: string[] }
  | { state: "disabled" }
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

/**
 * Build the 4 copyable MCP/HTTP presets (Endpoint & Header, generic Streamable
 * HTTP JSON, Claude Code CLI + Codex TOML, and REST API curl examples).
 */
export function buildMcpPresets(opts: {
  mcpUrl: string;
  apiBaseUrl: string;
  token: string;
  masked?: boolean;
}): McpPresets {
  const displayToken = opts.masked ? maskToken(opts.token) : opts.token;
  const authHeader = `Bearer ${displayToken}`;

  const genericJson = JSON.stringify(
    {
      mcpServers: {
        gateterm: {
          url: opts.mcpUrl,
          headers: {
            Authorization: authHeader,
          },
        },
      },
    },
    null,
    2,
  );

  const authArg = posixSingleQuote(`Authorization: ${authHeader}`);
  const claudeCodeCmd = `claude mcp add --transport http gateterm ${posixSingleQuote(opts.mcpUrl)} --header ${authArg}`;

  const codexToml = [
    "[mcp_servers.gateterm]",
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
    mcpUrl: opts.mcpUrl,
    authHeader,
    genericJson,
    claudeCodeCmd,
    codexToml,
    curlExamples,
  };
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
    const body = (await res.json()) as { enabled?: unknown; protocol_versions?: unknown };
    if (body?.enabled === true) {
      const protocolVersions = Array.isArray(body.protocol_versions)
        ? body.protocol_versions.filter((v): v is string => typeof v === "string")
        : [];
      return { state: "enabled", protocolVersions };
    }
    if (body?.enabled === false) {
      return { state: "disabled" };
    }
    return { state: "error" };
  } catch {
    return { state: "error" };
  }
}
