//! Pure-logic MCP endpoint derivation, token masking, preset generation, and
//! stale-response race guard for the Desktop Native client.

/// Live capability probe status returned by `GET /api/mcp/status`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum McpServerStatus {
    #[default]
    Loading,
    Enabled {
        protocol_versions: Vec<String>,
    },
    Disabled,
    Unsupported,
    Error(String),
}

/// Copyable configuration presets generated for an active Gateterm server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPresets {
    pub mcp_url: String,
    pub auth_header: String,
    pub generic_json: String,
    pub claude_code_cmd: String,
    pub codex_toml: String,
    pub curl_examples: String,
}

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

/// Build the 4 copyable MCP/HTTP presets (Endpoint & Header, generic Streamable
/// HTTP JSON, Claude Code CLI + Codex TOML, and REST API curl examples).
pub fn build_mcp_presets(base_url: &str, token: &str, masked: bool) -> McpPresets {
    let (mcp_url, api_base_url) = derive_mcp_urls(base_url);
    let display_token = if masked {
        mask_token(token)
    } else {
        token.to_string()
    };
    let auth_header = format!("Bearer {display_token}");

    let generic_json = serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": {
            "gateterm": {
                "url": mcp_url,
                "headers": {
                    "Authorization": auth_header,
                }
            }
        }
    }))
    .unwrap_or_default();

    let auth_arg = posix_single_quote(&format!("Authorization: {auth_header}"));
    let claude_code_cmd = format!(
        "claude mcp add --transport http gateterm {} --header {auth_arg}",
        posix_single_quote(&mcp_url)
    );

    let codex_toml = [
        "[mcp_servers.gateterm]".to_string(),
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

        let masked = build_mcp_presets(base_url, special_token, true);
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

        let unmasked = build_mcp_presets(base_url, special_token, false);
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
}
