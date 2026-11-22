import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { execSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
execSync("npx esbuild src/mcp.ts --bundle --format=esm --outfile=test/.mcp.build.mjs", {
  cwd: join(here, ".."),
});
const {
  deriveEndpoints,
  maskToken,
  posixSingleQuote,
  escapeTomlBasicString,
  buildMcpPresets,
  fetchMcpStatus,
} = await import("./.mcp.build.mjs");

test("deriveEndpoints strips query/hash and preserves reverse-proxy subpaths", () => {
  assert.deepEqual(deriveEndpoints("http://127.0.0.1:7683/#token=abc"), {
    mcpUrl: "http://127.0.0.1:7683/mcp",
    apiBaseUrl: "http://127.0.0.1:7683",
  });

  assert.deepEqual(deriveEndpoints("https://proxy.example.com/gateterm/?x=1#token=abc"), {
    mcpUrl: "https://proxy.example.com/gateterm/mcp",
    apiBaseUrl: "https://proxy.example.com/gateterm",
  });

  assert.deepEqual(deriveEndpoints("https://proxy.example.com/gateterm/index.html?x=1#token=abc"), {
    mcpUrl: "https://proxy.example.com/gateterm/mcp",
    apiBaseUrl: "https://proxy.example.com/gateterm",
  });
});

test("maskToken masks empty, short, and >= 8 char tokens safely", () => {
  assert.equal(maskToken(""), "");
  assert.equal(maskToken("a"), "••••••••");
  assert.equal(maskToken("short"), "••••••••");
  assert.equal(maskToken("1234567"), "••••••••");
  assert.equal(maskToken("12345678"), "1234••••5678");
  assert.equal(maskToken("tok_abcdefghijkl"), "tok_••••ijkl");
});

test("posixSingleQuote and escapeTomlBasicString escape special characters", () => {
  const special = `a"b'c$d\\e`;
  assert.equal(posixSingleQuote(special), `'a"b'\\''c$d\\e'`);
  assert.equal(posixSingleQuote(""), "''");

  assert.equal(escapeTomlBasicString(special), `a\\"b'c$d\\\\e`);
  assert.equal(escapeTomlBasicString("line1\nline2\t\r"), `line1\\nline2\\t\\r`);
});

test("buildMcpPresets masks by request and safely escapes special token in JSON, TOML, and shell commands", () => {
  const specialToken = `a"b'c$d\\e`;
  const mcpUrl = "https://proxy.example.com/gateterm/mcp";
  const apiBaseUrl = "https://proxy.example.com/gateterm";

  const masked = buildMcpPresets({ mcpUrl, apiBaseUrl, token: specialToken, masked: true });
  assert.equal(masked.mcpUrl, mcpUrl);
  assert.equal(masked.authHeader, "Bearer a\"b'••••$d\\e");
  for (const field of [
    masked.authHeader,
    masked.genericJson,
    masked.claudeCodeCmd,
    masked.codexToml,
    masked.curlExamples,
  ]) {
    assert.ok(!field.includes(specialToken), "masked preset must not leak plaintext token");
  }

  const unmasked = buildMcpPresets({ mcpUrl, apiBaseUrl, token: specialToken, masked: false });
  assert.equal(unmasked.authHeader, `Bearer ${specialToken}`);

  const parsedJson = JSON.parse(unmasked.genericJson);
  assert.equal(parsedJson.mcpServers.gateterm.url, mcpUrl);
  assert.equal(parsedJson.mcpServers.gateterm.headers.Authorization, `Bearer ${specialToken}`);

  // Codex TOML escapes backslashes and double quotes inside basic strings
  assert.ok(unmasked.codexToml.includes("[mcp_servers.gateterm]"));
  assert.ok(unmasked.codexToml.includes(`url = "${mcpUrl}"`));
  assert.ok(
    unmasked.codexToml.includes(
      `http_headers = { Authorization = "Bearer ${escapeTomlBasicString(specialToken)}" }`,
    ),
  );

  // Claude Code CLI uses posixSingleQuote and avoids double-quoted $d expansion
  const expectedHeaderArg = posixSingleQuote(`Authorization: Bearer ${specialToken}`);
  assert.equal(
    unmasked.claudeCodeCmd,
    `claude mcp add --transport http gateterm '${mcpUrl}' --header ${expectedHeaderArg}`,
  );
  assert.ok(!unmasked.claudeCodeCmd.includes(`"Authorization:`));

  // curlExamples includes the 4 minimal API workflows with posixSingleQuote auth and apiBaseUrl
  assert.ok(unmasked.curlExamples.includes(expectedHeaderArg));
  assert.ok(!unmasked.curlExamples.includes(`"Authorization:`));
  assert.ok(unmasked.curlExamples.includes(`${apiBaseUrl}/api/sessions`));
  assert.ok(unmasked.curlExamples.includes(`/screen`));
  assert.ok(unmasked.curlExamples.includes(`/input`));
  assert.ok(unmasked.curlExamples.includes(`wait_ms`));
  assert.ok(unmasked.curlExamples.includes(`${apiBaseUrl}/api/agents/pending`));
  assert.ok(unmasked.curlExamples.includes(`${apiBaseUrl}/api/agents/answer`));
});

test("fetchMcpStatus handles enabled, disabled, 404 unsupported, 401/500, and network error", async () => {
  const seen = [];
  const resEnabled = await fetchMcpStatus(
    "secret-tok",
    async (url, init) => {
      seen.push({ url, init });
      return {
        ok: true,
        status: 200,
        json: async () => ({
          enabled: true,
          protocol_versions: ["2025-11-25", "2025-06-18"],
          endpoint: "/mcp",
        }),
      };
    },
    "https://proxy.example.com/gateterm/",
  );
  assert.deepEqual(resEnabled, {
    state: "enabled",
    protocolVersions: ["2025-11-25", "2025-06-18"],
  });
  assert.equal(seen[0].url, "https://proxy.example.com/gateterm/api/mcp/status");
  assert.equal(seen[0].init.headers.Authorization, "Bearer secret-tok");

  const resDisabled = await fetchMcpStatus("secret-tok", async () => ({
    ok: true,
    status: 200,
    json: async () => ({ enabled: false, protocol_versions: ["2025-11-25"], endpoint: "/mcp" }),
  }));
  assert.deepEqual(resDisabled, { state: "disabled" });

  const resUnsupported = await fetchMcpStatus("secret-tok", async () => ({
    ok: false,
    status: 404,
  }));
  assert.deepEqual(resUnsupported, { state: "unsupported" });

  const resUnauthorized = await fetchMcpStatus("secret-tok", async () => ({
    ok: false,
    status: 401,
  }));
  assert.deepEqual(resUnauthorized, { state: "error" });

  const resNetworkError = await fetchMcpStatus("secret-tok", async () => {
    throw new Error("network down");
  });
  assert.deepEqual(resNetworkError, { state: "error" });
});
