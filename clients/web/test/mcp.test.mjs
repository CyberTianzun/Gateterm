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
  slugifyServerKey,
  extractTitlePrefix,
  deriveMcpServerKey,
  formatTomlTableKey,
  formatShellKeyArg,
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

test("slugifyServerKey normalizes ASCII, collapses separators, and caps length at 48", () => {
  assert.equal(slugifyServerKey("Mac Mini"), "mac-mini");
  assert.equal(slugifyServerKey("  --Mac__Mini--  "), "mac__mini");
  assert.equal(slugifyServerKey("公司测试机"), "");
  assert.equal(slugifyServerKey("---___"), "");

  const long = `${"a".repeat(47)}-b-c`;
  const slugged = slugifyServerKey(long);
  assert.ok(slugged.length <= 48);
  assert.ok(!slugged.endsWith("-") && !slugged.endsWith("_"));
  assert.equal(slugged, "a".repeat(47));
});

test("extractTitlePrefix extracts prefix before ' - gateterm'", () => {
  assert.equal(extractTitlePrefix("prod-01 - gateterm"), "prod-01");
  assert.equal(extractTitlePrefix("(2) prod-01 - gateterm"), "prod-01");
  assert.equal(extractTitlePrefix("gateterm"), "");
  assert.equal(extractTitlePrefix("(2) gateterm"), "");
  assert.equal(extractTitlePrefix(" - gateterm"), "");
  assert.equal(extractTitlePrefix("other title"), "");
});

test("deriveMcpServerKey derives conflict-free keys with prefix deduplication and host-port fallback", () => {
  assert.equal(deriveMcpServerKey(["Mac Mini"], "http://10.0.0.2:7683"), "gateterm-mac-mini");
  assert.equal(deriveMcpServerKey(["Local Gateterm"], "http://127.0.0.1:7683"), "local-gateterm");
  assert.equal(deriveMcpServerKey(["gateterm-prod"], "http://127.0.0.1:7683"), "gateterm-prod");
  assert.equal(deriveMcpServerKey(["gateterm_prod"], "http://127.0.0.1:7683"), "gateterm_prod");
  assert.equal(deriveMcpServerKey(["prod_gateterm"], "http://127.0.0.1:7683"), "prod_gateterm");
  assert.equal(deriveMcpServerKey(["gateterm"], "http://127.0.0.1:7683"), "gateterm");

  // Skips CJK-only candidate and picks next valid ASCII candidate
  assert.equal(deriveMcpServerKey(["公司测试机", "build-01"], "http://10.0.0.2:7683"), "gateterm-build-01");

  // Falls back to host-port when all candidates are non-ASCII / empty
  assert.equal(deriveMcpServerKey(["公司测试机"], "http://192.168.1.20:7683"), "gateterm-192-168-1-20-7683");

  // Distinguishes loopback ports and IPv6 loopback [::1]
  assert.equal(deriveMcpServerKey([], "http://127.0.0.1:17683"), "gateterm-127-0-0-1-17683");
  assert.equal(deriveMcpServerKey([], "http://127.0.0.1:7683"), "gateterm-127-0-0-1-7683");
  assert.equal(deriveMcpServerKey([], "http://localhost:7683"), "gateterm-localhost-7683");
  assert.equal(deriveMcpServerKey([], "http://[::1]:7683"), "gateterm-1-7683");

  // Default HTTPS 443 omits port; reverse-proxy path ignored
  assert.equal(deriveMcpServerKey([], "https://mac.tailnet.ts.net/gateterm"), "gateterm-mac-tailnet-ts-net");
  assert.equal(deriveMcpServerKey([], "https://mac.tailnet.ts.net:8443/gateterm"), "gateterm-mac-tailnet-ts-net-8443");

  // Empty/invalid URL falls back to "gateterm"
  assert.equal(deriveMcpServerKey([], ""), "gateterm");
  assert.equal(deriveMcpServerKey([null, undefined, "   "], "not-a-url"), "gateterm");

  // Prefix cap keeps total length <= 48 with no trailing '-' or '_'
  const capped = deriveMcpServerKey([`${"x".repeat(38)}-yyyyy`], "");
  assert.ok(capped.length <= 48);
  assert.ok(!capped.endsWith("-") && !capped.endsWith("_"));
  assert.equal(capped, `gateterm-${"x".repeat(38)}`);
});

test("formatTomlTableKey, formatShellKeyArg, and buildMcpPresets safely serialize bare and special serverName values", () => {
  assert.equal(formatTomlTableKey("gateterm-mac-mini"), "gateterm-mac-mini");
  assert.equal(formatShellKeyArg("gateterm-mac-mini"), "gateterm-mac-mini");

  const bare = buildMcpPresets({
    mcpUrl: "http://10.0.0.2:7683/mcp",
    apiBaseUrl: "http://10.0.0.2:7683",
    token: "tok_12345678",
    serverName: "gateterm-mac-mini",
  });
  assert.equal(bare.serverName, "gateterm-mac-mini");
  assert.ok(bare.codexToml.includes("[mcp_servers.gateterm-mac-mini]"));
  assert.ok(
    bare.claudeCodeCmd.startsWith("claude mcp add --transport http gateterm-mac-mini 'http://10.0.0.2:7683/mcp'"),
  );
  assert.equal(JSON.parse(bare.genericJson).mcpServers["gateterm-mac-mini"].url, "http://10.0.0.2:7683/mcp");

  const customName = 'my.box "dev" 中文';
  assert.equal(formatTomlTableKey(customName), '"my.box \\"dev\\" 中文"');
  assert.equal(formatShellKeyArg(customName), posixSingleQuote(customName));

  const custom = buildMcpPresets({
    mcpUrl: "http://10.0.0.2:7683/mcp",
    apiBaseUrl: "http://10.0.0.2:7683",
    token: "tok_12345678",
    serverName: customName,
  });
  assert.equal(custom.serverName, customName);
  assert.equal(JSON.parse(custom.genericJson).mcpServers[customName].url, "http://10.0.0.2:7683/mcp");
  assert.ok(custom.codexToml.includes('[mcp_servers."my.box \\"dev\\" 中文"]'));
  assert.ok(
    custom.claudeCodeCmd.startsWith(
      `claude mcp add --transport http ${posixSingleQuote(customName)} 'http://10.0.0.2:7683/mcp'`,
    ),
  );

  // Blank/whitespace serverName falls back to "gateterm", and control whitespace is normalized
  const blank = buildMcpPresets({
    mcpUrl: "http://10.0.0.2:7683/mcp",
    apiBaseUrl: "http://10.0.0.2:7683",
    token: "tok_12345678",
    serverName: "  \n\t ",
  });
  assert.equal(blank.serverName, "gateterm");
  assert.ok(blank.codexToml.includes("[mcp_servers.gateterm]"));

  const multiline = buildMcpPresets({
    mcpUrl: "http://10.0.0.2:7683/mcp",
    apiBaseUrl: "http://10.0.0.2:7683",
    token: "tok_12345678",
    serverName: "  box\n\tone  ",
  });
  assert.equal(multiline.serverName, "box one");
});

test("fetchMcpStatus parses optional server identity on enabled and disabled responses and preserves legacy compatibility", async () => {
  const serverPayload = {
    instance_id: "a1b2c3d4e5f64789a0b1c2d3e4f56789",
    name: "mac-build",
    hostname: "Mac-Studio.local",
    os: "macos",
    arch: "aarch64",
    port: 7683,
    description: "macOS arm64 iOS build",
  };

  const resEnabledWithServer = await fetchMcpStatus("secret-tok", async () => ({
    ok: true,
    status: 200,
    json: async () => ({
      enabled: true,
      protocol_versions: ["2025-11-25"],
      endpoint: "/mcp",
      server: serverPayload,
    }),
  }));
  assert.deepEqual(resEnabledWithServer, {
    state: "enabled",
    protocolVersions: ["2025-11-25"],
    server: {
      instanceId: "a1b2c3d4e5f64789a0b1c2d3e4f56789",
      name: "mac-build",
      hostname: "Mac-Studio.local",
      os: "macos",
      arch: "aarch64",
      port: 7683,
      description: "macOS arm64 iOS build",
    },
  });

  const resDisabledWithServer = await fetchMcpStatus("secret-tok", async () => ({
    ok: true,
    status: 200,
    json: async () => ({
      enabled: false,
      protocol_versions: ["2025-11-25"],
      endpoint: "/mcp",
      server: {
        instance_id: "deadbeef",
        name: "win-box",
        os: "windows",
        arch: "x86_64",
      },
    }),
  }));
  assert.deepEqual(resDisabledWithServer, {
    state: "disabled",
    server: {
      instanceId: "deadbeef",
      name: "win-box",
      os: "windows",
      arch: "x86_64",
    },
  });

  // Legacy response without `server` leaves `server === undefined`
  const resLegacy = await fetchMcpStatus("secret-tok", async () => ({
    ok: true,
    status: 200,
    json: async () => ({
      enabled: true,
      protocol_versions: ["2025-11-25"],
      endpoint: "/mcp",
    }),
  }));
  assert.equal(resLegacy.server, undefined);
  assert.deepEqual(resLegacy, {
    state: "enabled",
    protocolVersions: ["2025-11-25"],
  });

  // Non-object `server` or invalid field types are safely ignored
  const resMalformedServer = await fetchMcpStatus("secret-tok", async () => ({
    ok: true,
    status: 200,
    json: async () => ({
      enabled: true,
      protocol_versions: ["2025-11-25"],
      server: ["not-an-object"],
    }),
  }));
  assert.equal(resMalformedServer.server, undefined);
});
