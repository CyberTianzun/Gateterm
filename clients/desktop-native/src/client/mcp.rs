//! Pure-logic MCP endpoint derivation, token masking, conflict-free server key
//! derivation, preset generation, and stale-response race guard for the Desktop
//! Native client.

use serde::Deserialize;
use url::Url;

/// Optional node identity metadata returned by `GET /api/mcp/status`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct McpServerIdentity {
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Live capability probe status returned by `GET /api/mcp/status`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum McpServerStatus {
    #[default]
    Loading,
    Enabled {
        protocol_versions: Vec<String>,
        server: Option<McpServerIdentity>,
    },
    Disabled {
        server: Option<McpServerIdentity>,
    },
    Unsupported,
    Error(String),
}

impl McpServerStatus {
    /// Returns the optional server identity reported by `/api/mcp/status`, if any.
    pub fn server_identity(&self) -> Option<&McpServerIdentity> {
        match self {
            Self::Enabled { server, .. } | Self::Disabled { server } => server.as_ref(),
            Self::Loading | Self::Unsupported | Self::Error(_) => None,
        }
    }
}

/// Copyable configuration presets generated for an active Gateterm server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPresets {
    pub server_name: String,
    pub mcp_url: String,
    pub auth_header: String,
    pub generic_json: String,
    pub claude_code_cmd: String,
    pub codex_toml: String,
    pub curl_examples: String,
}

const MAX_SERVER_KEY_LEN: usize = 48;

/// Derive `(mcp_url, api_base_url)` from a server base URL, stripping any
/// `?query`, `#fragment`, and trailing slashes while preserving subpaths.
pub fn derive_mcp_urls(base_url: &str) -> (String, String) {
    let without_fragment = base_url.split('#').next().unwrap_or(base_url);
    let without_query = without_fragment.split('?').next().unwrap_or(without_fragment);
    let api_base_url = without_query.trim().trim_end_matches('/').to_string();
    let mcp_url = format!("{api_base_url}/mcp");
    (mcp_url, api_base_url)
}

/// Mask a Bearer token for safe on-screen preview:
/// - empty -> `""`
/// - >= 8 Unicode scalar values -> first 4 + `••••` + last 4
/// - 1..=7 Unicode scalar values -> `••••••••`
pub fn mask_token(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    let char_count = token.chars().count();
    if char_count >= 8 {
        let prefix: String = token.chars().take(4).collect();
        let suffix: String = token.chars().skip(char_count - 4).collect();
        format!("{prefix}••••{suffix}")
    } else {
        "••••••••".to_string()
    }
}

/// Wrap a value in POSIX shell single quotes (`'...'`), escaping embedded single
/// quotes as `'\''` so `$`, `"`, `\`, and whitespace are never expanded.
pub fn posix_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Escape a string for inclusion inside a TOML basic string (`"..."`).
pub fn escape_toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Normalize an arbitrary string into a lowercase `[a-z0-9_-]` slug capped at
/// 48 ASCII chars with no leading/trailing `-` or `_`. Returns `None` if empty.
pub fn slugify_server_key(raw: &str) -> Option<String> {
    let mut collapsed = String::with_capacity(raw.len());
    let mut in_sep = false;
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if matches!(lower, 'a'..='z' | '0'..='9' | '_' | '-') {
            in_sep = false;
            collapsed.push(lower);
        } else if !in_sep {
            collapsed.push('-');
            in_sep = true;
        }
    }
    let trimmed = collapsed.trim_matches(['-', '_']);
    if trimmed.is_empty() {
        return None;
    }
    let capped: String = trimmed.chars().take(MAX_SERVER_KEY_LEN).collect();
    let final_slug = capped.trim_end_matches(['-', '_']);
    if final_slug.is_empty() {
        None
    } else {
        Some(final_slug.to_string())
    }
}

fn with_gateterm_prefix(slug: &str) -> String {
    let raw = if slug == "gateterm"
        || slug.starts_with("gateterm-")
        || slug.starts_with("gateterm_")
        || slug.ends_with("-gateterm")
        || slug.ends_with("_gateterm")
    {
        slug.to_string()
    } else {
        format!("gateterm-{slug}")
    };
    let capped: String = raw.chars().take(MAX_SERVER_KEY_LEN).collect();
    capped.trim_end_matches(['-', '_']).to_string()
}

fn host_port_hint(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = Url::parse(trimmed).ok()?;
    let bare_host = parsed
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if bare_host.is_empty() {
        return None;
    }
    let is_loopback = matches!(bare_host.as_str(), "127.0.0.1" | "localhost" | "::1");
    match parsed.port() {
        Some(port) if is_loopback || (port != 80 && port != 443) => {
            Some(format!("{bare_host}-{port}"))
        }
        _ => Some(bare_host),
    }
}

/// Derive a conflict-free default MCP server key from ordered candidates
/// (falling back to URL `host[-port]` and ultimately `"gateterm"`).
pub fn derive_mcp_server_key(candidates: &[Option<&str>], base_url: &str) -> String {
    for candidate in candidates.iter().flatten() {
        if let Some(slug) = slugify_server_key(candidate) {
            return with_gateterm_prefix(&slug);
        }
    }
    if let Some(hint) = host_port_hint(base_url)
        && let Some(slug) = slugify_server_key(&hint)
    {
        return with_gateterm_prefix(&slug);
    }
    "gateterm".to_string()
}

fn is_bare_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Format a key for use under `[mcp_servers.<key>]` in TOML: bare when strictly
/// `[A-Za-z0-9_-]+`, otherwise a TOML basic quoted string.
pub fn format_toml_table_key(key: &str) -> String {
    if is_bare_key(key) {
        key.to_string()
    } else {
        format!("\"{}\"", escape_toml_basic_string(key))
    }
}

/// Format a server key argument for POSIX shell commands: bare when strictly
/// `[A-Za-z0-9_-]+`, otherwise wrapped with `posix_single_quote`.
pub fn format_shell_key_arg(key: &str) -> String {
    if is_bare_key(key) {
        key.to_string()
    } else {
        posix_single_quote(key)
    }
}

fn normalize_server_name(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "gateterm".to_string();
    };
    let mut cleaned = String::with_capacity(raw.len());
    let mut in_ctrl_ws = false;
    for ch in raw.chars() {
        if matches!(ch, '\r' | '\n' | '\t') {
            if !in_ctrl_ws {
                cleaned.push(' ');
                in_ctrl_ws = true;
            }
        } else {
            in_ctrl_ws = false;
            cleaned.push(ch);
        }
    }
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "gateterm".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Format a concise single-line node identity summary (`<name> · <os>/<arch>`)
/// from an optional `McpServerIdentity`, omitting empty fields and separators.
pub fn format_mcp_node_meta(server: Option<&McpServerIdentity>) -> Option<String> {
    let server = server?;
    let label = server
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            server
                .hostname
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        });
    let os = server.os.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let arch = server
        .arch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let platform = match (os, arch) {
        (Some(o), Some(a)) => Some(format!("{o}/{a}")),
        (Some(o), None) => Some(o.to_string()),
        (None, Some(a)) => Some(a.to_string()),
        (None, None) => None,
    };
    match (label, platform) {
        (Some(l), Some(p)) => Some(format!("{l} · {p}")),
        (Some(l), None) => Some(l.to_string()),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    }
}

/// Build the 4 copyable MCP/HTTP presets (Endpoint & Header, generic Streamable
/// HTTP JSON, Claude Code CLI + Codex TOML, and REST API curl examples).
pub fn build_mcp_presets(
    base_url: &str,
    token: &str,
    server_name: Option<&str>,
    masked: bool,
) -> McpPresets {
    let server_name = normalize_server_name(server_name);
    let (mcp_url, api_base_url) = derive_mcp_urls(base_url);
    let display_token = if masked {
        mask_token(token)
    } else {
        token.to_string()
    };
    let auth_header = format!("Bearer {display_token}");

    let mut mcp_servers = serde_json::Map::new();
    mcp_servers.insert(
        server_name.clone(),
        serde_json::json!({
            "url": mcp_url,
            "headers": {
                "Authorization": auth_header,
            }
        }),
    );
    let generic_json = serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": mcp_servers,
    }))
    .unwrap_or_default();

    let auth_arg = posix_single_quote(&format!("Authorization: {auth_header}"));
    let claude_code_cmd = format!(
        "claude mcp add --transport http {} {} --header {auth_arg}",
        format_shell_key_arg(&server_name),
        posix_single_quote(&mcp_url)
    );

    let codex_toml = [
        format!("[mcp_servers.{}]", format_toml_table_key(&server_name)),
        format!("url = \"{}\"", escape_toml_basic_string(&mcp_url)),
        format!(
            "http_headers = {{ Authorization = \"{}\" }}",
            escape_toml_basic_string(&auth_header)
        ),
    ]
    .join("\n");

    let curl_examples = [
        "# 1. List sessions".to_string(),
        format!(
            "curl -sS -H {auth_arg} {}",
            posix_single_quote(&format!("{api_base_url}/api/sessions"))
        ),
        String::new(),
        "# 2. Read session screen".to_string(),
        format!(
            "curl -sS -H {auth_arg} {}",
            posix_single_quote(&format!("{api_base_url}/api/sessions/1/screen?scrollback_lines=50"))
        ),
        String::new(),
        "# 3. Send input and wait for output".to_string(),
        format!(
            "curl -sS -X POST -H {auth_arg} -H 'Content-Type: application/json' -d '{{\"text\":\"git status\",\"enter\":true,\"wait_ms\":3000}}' {}",
            posix_single_quote(&format!("{api_base_url}/api/sessions/1/input"))
        ),
        String::new(),
        "# 4. List and answer pending approvals".to_string(),
        format!(
            "curl -sS -H {auth_arg} {}",
            posix_single_quote(&format!("{api_base_url}/api/agents/pending"))
        ),
        format!(
            "curl -sS -X POST -H {auth_arg} -H 'Content-Type: application/json' -d '{{\"pending_id\":\"<pending_id>\",\"decision\":\"allow\"}}' {}",
            posix_single_quote(&format!("{api_base_url}/api/agents/answer"))
        ),
    ]
    .join("\n");

    McpPresets {
        server_name,
        mcp_url,
        auth_header,
        generic_json,
        claude_code_cmd,
        codex_toml,
        curl_examples,
    }
}

/// Guard against stale async `/api/mcp/status` responses overwriting state
/// after the user has switched to a different target server.
pub fn should_apply_status_response(current_server_id: &str, response_server_id: &str) -> bool {
    current_server_id == response_server_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_mcp_urls_strips_trailing_slash_and_preserves_subpath() {
        assert_eq!(
            derive_mcp_urls("http://127.0.0.1:7683"),
            (
                "http://127.0.0.1:7683/mcp".to_string(),
                "http://127.0.0.1:7683".to_string(),
            )
        );
        assert_eq!(
            derive_mcp_urls("http://127.0.0.1:7683/"),
            (
                "http://127.0.0.1:7683/mcp".to_string(),
                "http://127.0.0.1:7683".to_string(),
            )
        );
        assert_eq!(
            derive_mcp_urls("http://127.0.0.1:7683/#token=abc"),
            (
                "http://127.0.0.1:7683/mcp".to_string(),
                "http://127.0.0.1:7683".to_string(),
            )
        );
        assert_eq!(
            derive_mcp_urls("https://proxy.example.com/gateterm"),
            (
                "https://proxy.example.com/gateterm/mcp".to_string(),
                "https://proxy.example.com/gateterm".to_string(),
            )
        );
        assert_eq!(
            derive_mcp_urls("https://proxy.example.com/gateterm/"),
            (
                "https://proxy.example.com/gateterm/mcp".to_string(),
                "https://proxy.example.com/gateterm".to_string(),
            )
        );
        assert_eq!(
            derive_mcp_urls("https://proxy.example.com/gateterm/?x=1#token=abc"),
            (
                "https://proxy.example.com/gateterm/mcp".to_string(),
                "https://proxy.example.com/gateterm".to_string(),
            )
        );
    }

    #[test]
    fn mask_token_respects_length_threshold() {
        assert_eq!(mask_token(""), "");
        assert_eq!(mask_token("a"), "••••••••");
        assert_eq!(mask_token("short"), "••••••••");
        assert_eq!(mask_token("1234567"), "••••••••");
        assert_eq!(mask_token("12345678"), "1234••••5678");
        assert_eq!(mask_token("tok_abcdefghijkl"), "tok_••••ijkl");
        // UTF-8 char boundaries: 4 multi-byte chars (< 8 chars despite > 8 bytes)
        assert_eq!(mask_token("🔐🔑🗝🛡"), "••••••••");
        // UTF-8 char boundaries: 8 multi-byte chars (>= 8 chars)
        assert_eq!(mask_token("αβγδεζηθ"), "αβγδ••••εζηθ");
    }

    #[test]
    fn escape_toml_basic_string_handles_control_chars() {
        assert_eq!(
            escape_toml_basic_string("line1\nline2\t\r\u{0008}\u{000c}\u{0001}"),
            r#"line1\nline2\t\r\b\f\u0001"#
        );
    }

    #[test]
    fn build_mcp_presets_escapes_special_characters_in_json_toml_and_shell() {
        let special_token = r#"a"b'c$d\e"#;
        let base_url = "https://proxy.example.com/gateterm/";
        let expected_mcp_url = "https://proxy.example.com/gateterm/mcp";
        let expected_api_base = "https://proxy.example.com/gateterm";

        let masked = build_mcp_presets(base_url, special_token, None, true);
        assert_eq!(masked.server_name, "gateterm");
        assert_eq!(masked.mcp_url, expected_mcp_url);
        assert_eq!(masked.auth_header, "Bearer a\"b'••••$d\\e");
        for field in [
            &masked.auth_header,
            &masked.generic_json,
            &masked.claude_code_cmd,
            &masked.codex_toml,
            &masked.curl_examples,
        ] {
            assert!(
                !field.contains(special_token),
                "masked preset must not leak plaintext token"
            );
        }
        let parsed_masked_json: serde_json::Value =
            serde_json::from_str(&masked.generic_json).expect("masked JSON must parse");
        assert_eq!(
            parsed_masked_json["mcpServers"]["gateterm"]["headers"]["Authorization"],
            "Bearer a\"b'••••$d\\e"
        );

        let unmasked = build_mcp_presets(base_url, special_token, None, false);
        assert_eq!(unmasked.server_name, "gateterm");
        assert_eq!(unmasked.mcp_url, expected_mcp_url);
        assert_eq!(unmasked.auth_header, format!("Bearer {special_token}"));

        let parsed_json: serde_json::Value =
            serde_json::from_str(&unmasked.generic_json).expect("unmasked JSON must parse");
        assert_eq!(
            parsed_json["mcpServers"]["gateterm"]["url"],
            expected_mcp_url
        );
        assert_eq!(
            parsed_json["mcpServers"]["gateterm"]["headers"]["Authorization"],
            format!("Bearer {special_token}")
        );

        // Codex TOML basic string escaping and TOML round-trip
        assert!(unmasked.codex_toml.contains("[mcp_servers.gateterm]"));
        assert!(
            unmasked
                .codex_toml
                .contains(&format!("url = \"{expected_mcp_url}\""))
        );
        assert!(
            unmasked
                .codex_toml
                .contains(r#"http_headers = { Authorization = "Bearer a\"b'c$d\\e" }"#)
        );
        let parsed_toml: toml::Value =
            toml::from_str(&unmasked.codex_toml).expect("unmasked TOML must parse");
        assert_eq!(
            parsed_toml["mcp_servers"]["gateterm"]["url"].as_str(),
            Some(expected_mcp_url)
        );
        assert_eq!(
            parsed_toml["mcp_servers"]["gateterm"]["http_headers"]["Authorization"].as_str(),
            Some(format!("Bearer {special_token}").as_str())
        );

        // Claude Code CLI uses POSIX single quoting without double-quoted expansion
        let expected_auth_arg = r#"'Authorization: Bearer a"b'\''c$d\e'"#;
        assert_eq!(
            unmasked.claude_code_cmd,
            format!(
                "claude mcp add --transport http gateterm '{expected_mcp_url}' --header {expected_auth_arg}"
            )
        );
        assert!(!unmasked.claude_code_cmd.contains("\"Authorization:"));

        // curl examples match Web 1:1
        assert!(unmasked.curl_examples.contains(expected_auth_arg));
        assert!(!unmasked.curl_examples.contains("\"Authorization:"));
        assert!(
            unmasked
                .curl_examples
                .contains(&format!("{expected_api_base}/api/sessions"))
        );
        assert!(
            unmasked
                .curl_examples
                .contains("/screen?scrollback_lines=50")
        );
        assert!(unmasked.curl_examples.contains("/input"));
        assert!(unmasked.curl_examples.contains("wait_ms"));
        assert!(
            unmasked
                .curl_examples
                .contains(&format!("{expected_api_base}/api/agents/pending"))
        );
        assert!(
            unmasked
                .curl_examples
                .contains(&format!("{expected_api_base}/api/agents/answer"))
        );
    }

    #[test]
    fn should_apply_status_response_rejects_stale_server_responses() {
        assert!(should_apply_status_response(
            "local-embedded",
            "local-embedded"
        ));
        assert!(!should_apply_status_response("srv-2", "local-embedded"));
        assert!(!should_apply_status_response("local-embedded", "srv-2"));
    }

    #[test]
    fn slugify_server_key_normalizes_ascii_collapses_separators_and_caps_length() {
        assert_eq!(slugify_server_key("Mac Mini"), Some("mac-mini".to_string()));
        assert_eq!(
            slugify_server_key("  --Mac__Mini--  "),
            Some("mac__mini".to_string())
        );
        assert_eq!(slugify_server_key("公司测试机"), None);
        assert_eq!(slugify_server_key("---___"), None);

        let long = format!("{}-b-c", "a".repeat(47));
        let slugged = slugify_server_key(&long).expect("non-empty slug");
        assert!(slugged.len() <= 48);
        assert!(!slugged.ends_with('-') && !slugged.ends_with('_'));
        assert_eq!(slugged, "a".repeat(47));
    }

    #[test]
    fn derive_mcp_server_key_derives_conflict_free_keys_and_fallbacks() {
        assert_eq!(
            derive_mcp_server_key(&[Some("Mac Mini")], "http://10.0.0.2:7683"),
            "gateterm-mac-mini"
        );
        assert_eq!(
            derive_mcp_server_key(&[Some("Local Gateterm")], "http://127.0.0.1:7683"),
            "local-gateterm"
        );
        assert_eq!(
            derive_mcp_server_key(&[Some("gateterm-prod")], "http://127.0.0.1:7683"),
            "gateterm-prod"
        );
        assert_eq!(
            derive_mcp_server_key(&[Some("gateterm_prod")], "http://127.0.0.1:7683"),
            "gateterm_prod"
        );
        assert_eq!(
            derive_mcp_server_key(&[Some("prod_gateterm")], "http://127.0.0.1:7683"),
            "prod_gateterm"
        );
        assert_eq!(
            derive_mcp_server_key(&[Some("prod-gateterm")], "http://127.0.0.1:7683"),
            "prod-gateterm"
        );
        assert_eq!(
            derive_mcp_server_key(&[Some("gateterm")], "http://127.0.0.1:7683"),
            "gateterm"
        );

        // Skips pure CJK candidate and picks next valid ASCII candidate
        assert_eq!(
            derive_mcp_server_key(
                &[Some("公司测试机"), Some("build-01")],
                "http://10.0.0.2:7683"
            ),
            "gateterm-build-01"
        );
        assert_eq!(
            derive_mcp_server_key(
                &[Some("公司测试机"), None, Some("Mac-Studio.local")],
                "http://10.0.0.2:7683"
            ),
            "gateterm-mac-studio-local"
        );

        // Falls back to host-port when all candidates are non-ASCII / empty
        assert_eq!(
            derive_mcp_server_key(&[Some("公司测试机")], "http://192.168.1.20:7683"),
            "gateterm-192-168-1-20-7683"
        );

        // Distinguishes loopback ports and IPv6 loopback [::1]
        assert_eq!(
            derive_mcp_server_key(&[], "http://127.0.0.1:17683"),
            "gateterm-127-0-0-1-17683"
        );
        assert_eq!(
            derive_mcp_server_key(&[], "http://127.0.0.1:7683"),
            "gateterm-127-0-0-1-7683"
        );
        assert_eq!(
            derive_mcp_server_key(&[], "http://localhost:7683"),
            "gateterm-localhost-7683"
        );
        assert_eq!(
            derive_mcp_server_key(&[], "http://[::1]:7683"),
            "gateterm-1-7683"
        );

        // Default HTTPS 443 / HTTP 80 omit port; non-default port included
        assert_eq!(
            derive_mcp_server_key(&[], "https://mac.tailnet.ts.net/gateterm"),
            "gateterm-mac-tailnet-ts-net"
        );
        assert_eq!(
            derive_mcp_server_key(&[], "https://mac.tailnet.ts.net:443/gateterm"),
            "gateterm-mac-tailnet-ts-net"
        );
        assert_eq!(
            derive_mcp_server_key(&[], "https://mac.tailnet.ts.net:8443/gateterm"),
            "gateterm-mac-tailnet-ts-net-8443"
        );
        assert_eq!(
            derive_mcp_server_key(&[], "http://mac.tailnet.ts.net:80/gateterm"),
            "gateterm-mac-tailnet-ts-net"
        );

        // Empty / invalid URL falls back to "gateterm"
        assert_eq!(derive_mcp_server_key(&[], ""), "gateterm");
        assert_eq!(
            derive_mcp_server_key(&[None, Some("   ")], "not-a-url"),
            "gateterm"
        );

        // Prefix cap keeps total length <= 48 with no trailing '-' or '_'
        let capped_input = format!("{}-yyyyy", "x".repeat(38));
        let capped = derive_mcp_server_key(&[Some(&capped_input)], "");
        assert!(capped.len() <= 48);
        assert!(!capped.ends_with('-') && !capped.ends_with('_'));
        assert_eq!(capped, format!("gateterm-{}", "x".repeat(38)));

        let long_prefixed = format!("gateterm-{}", "z".repeat(50));
        let capped_prefixed = derive_mcp_server_key(&[Some(&long_prefixed)], "");
        assert_eq!(capped_prefixed.len(), 48);
        assert_eq!(capped_prefixed, format!("gateterm-{}", "z".repeat(39)));
    }

    #[test]
    fn build_mcp_presets_safely_serializes_bare_and_special_server_names() {
        assert_eq!(format_toml_table_key("gateterm-mac-mini"), "gateterm-mac-mini");
        assert_eq!(format_shell_key_arg("gateterm-mac-mini"), "gateterm-mac-mini");

        let bare = build_mcp_presets(
            "http://10.0.0.2:7683",
            "tok_12345678",
            Some("gateterm-mac-mini"),
            false,
        );
        assert_eq!(bare.server_name, "gateterm-mac-mini");
        assert!(bare.codex_toml.contains("[mcp_servers.gateterm-mac-mini]"));
        assert!(bare.claude_code_cmd.starts_with(
            "claude mcp add --transport http gateterm-mac-mini 'http://10.0.0.2:7683/mcp'"
        ));
        let bare_json: serde_json::Value =
            serde_json::from_str(&bare.generic_json).expect("bare JSON must parse");
        assert_eq!(
            bare_json["mcpServers"]["gateterm-mac-mini"]["url"],
            "http://10.0.0.2:7683/mcp"
        );
        let bare_toml: toml::Value =
            toml::from_str(&bare.codex_toml).expect("bare TOML must parse");
        assert_eq!(
            bare_toml["mcp_servers"]["gateterm-mac-mini"]["url"].as_str(),
            Some("http://10.0.0.2:7683/mcp")
        );

        let custom_name = r#"my.box "dev" 中文"#;
        assert_eq!(
            format_toml_table_key(custom_name),
            r#""my.box \"dev\" 中文""#
        );
        assert_eq!(
            format_shell_key_arg(custom_name),
            posix_single_quote(custom_name)
        );

        let custom = build_mcp_presets(
            "http://10.0.0.2:7683",
            "tok_12345678",
            Some(custom_name),
            false,
        );
        assert_eq!(custom.server_name, custom_name);
        let custom_json: serde_json::Value =
            serde_json::from_str(&custom.generic_json).expect("custom JSON must parse");
        assert_eq!(
            custom_json["mcpServers"][custom_name]["url"],
            "http://10.0.0.2:7683/mcp"
        );
        assert!(
            custom
                .codex_toml
                .contains(r#"[mcp_servers."my.box \"dev\" 中文"]"#)
        );
        let custom_toml: toml::Value =
            toml::from_str(&custom.codex_toml).expect("custom TOML must parse");
        assert_eq!(
            custom_toml["mcp_servers"][custom_name]["url"].as_str(),
            Some("http://10.0.0.2:7683/mcp")
        );
        assert_eq!(
            custom_toml["mcp_servers"][custom_name]["http_headers"]["Authorization"].as_str(),
            Some("Bearer tok_12345678")
        );
        assert!(custom.claude_code_cmd.starts_with(&format!(
            "claude mcp add --transport http {} 'http://10.0.0.2:7683/mcp'",
            posix_single_quote(custom_name)
        )));

        // Blank/whitespace server_name falls back to "gateterm"; control whitespace collapses
        let blank = build_mcp_presets(
            "http://10.0.0.2:7683",
            "tok_12345678",
            Some("  \n\t "),
            false,
        );
        assert_eq!(blank.server_name, "gateterm");
        assert!(blank.codex_toml.contains("[mcp_servers.gateterm]"));

        let multiline = build_mcp_presets(
            "http://10.0.0.2:7683",
            "tok_12345678",
            Some("  box\n\tone  "),
            false,
        );
        assert_eq!(multiline.server_name, "box one");
        let multiline_toml: toml::Value =
            toml::from_str(&multiline.codex_toml).expect("multiline TOML must parse");
        assert_eq!(
            multiline_toml["mcp_servers"]["box one"]["url"].as_str(),
            Some("http://10.0.0.2:7683/mcp")
        );
    }

    #[test]
    fn format_mcp_node_meta_handles_partial_and_empty_identities() {
        assert_eq!(format_mcp_node_meta(None), None);
        assert_eq!(format_mcp_node_meta(Some(&McpServerIdentity::default())), None);
        assert_eq!(
            format_mcp_node_meta(Some(&McpServerIdentity {
                name: Some("   ".to_string()),
                hostname: Some("".to_string()),
                os: Some(" ".to_string()),
                arch: None,
                ..Default::default()
            })),
            None
        );
        assert_eq!(
            format_mcp_node_meta(Some(&McpServerIdentity {
                name: Some("mac-build".to_string()),
                hostname: Some("Mac-Studio.local".to_string()),
                os: Some("macos".to_string()),
                arch: Some("aarch64".to_string()),
                ..Default::default()
            })),
            Some("mac-build · macos/aarch64".to_string())
        );
        assert_eq!(
            format_mcp_node_meta(Some(&McpServerIdentity {
                name: None,
                hostname: Some("Host-01".to_string()),
                os: Some("linux".to_string()),
                arch: None,
                ..Default::default()
            })),
            Some("Host-01 · linux".to_string())
        );
        assert_eq!(
            format_mcp_node_meta(Some(&McpServerIdentity {
                name: Some("only-name".to_string()),
                ..Default::default()
            })),
            Some("only-name".to_string())
        );
        assert_eq!(
            format_mcp_node_meta(Some(&McpServerIdentity {
                os: Some("windows".to_string()),
                arch: Some("x86_64".to_string()),
                ..Default::default()
            })),
            Some("windows/x86_64".to_string())
        );
    }
}
