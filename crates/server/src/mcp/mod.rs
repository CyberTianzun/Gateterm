use std::net::Ipv6Addr;
use std::sync::OnceLock;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::app::{bearer_ok, AppState};
use crate::config::normalize_mcp_text;

pub mod http;
pub mod protocol;
pub mod service;

pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

fn cached_hostname() -> Option<String> {
    static HOSTNAME: OnceLock<Option<String>> = OnceLock::new();
    HOSTNAME
        .get_or_init(|| normalize_mcp_text(sysinfo::System::host_name().as_deref(), 64))
        .clone()
}

/// Resolved identity metadata for this gateterm-server process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerIdentity {
    pub instance_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub os: &'static str,
    pub arch: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ServerIdentity {
    /// Resolves server identity from runtime `AppState` (`[mcp].name` -> `web_title` -> OS hostname -> `"gateterm"`).
    pub fn from_state(state: &AppState) -> Self {
        let hostname = cached_hostname();
        let name = normalize_mcp_text(state.mcp.name.as_deref(), 64)
            .or_else(|| normalize_mcp_text(state.web_title.get().as_deref(), 64))
            .or_else(|| hostname.clone())
            .unwrap_or_else(|| "gateterm".to_string());
        let port = (state.listen_port > 0).then_some(state.listen_port);
        let description = normalize_mcp_text(state.mcp.description.as_deref(), 256);
        Self {
            instance_id: state.manager.instance_id().to_string(),
            name,
            hostname,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            port,
            description,
        }
    }

    pub fn text_tag(&self) -> String {
        match self.port {
            Some(port) => format!("{} | {}:{}", self.name, self.os, port),
            None => format!("{} | {}", self.name, self.os),
        }
    }

    pub fn title(&self) -> String {
        format!("Gateterm ({} · {}/{})", self.name, self.os, self.arch)
    }
}

fn parse_port(raw: &str) -> Option<u16> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u16>().ok().filter(|&p| p > 0)
}

fn parse_authority(raw: &str) -> Option<(&str, Option<u16>)> {
    if raw.is_empty()
        || raw.chars().any(char::is_whitespace)
        || raw
            .bytes()
            .any(|b| matches!(b, b'@' | b'/' | b'?' | b'#' | b'\\'))
    {
        return None;
    }
    if raw.starts_with('[') {
        let close = raw.find(']')?;
        let inner = &raw[1..close];
        if inner.parse::<Ipv6Addr>().is_err() {
            return None;
        }
        let host = &raw[..=close];
        let after = &raw[close + 1..];
        if after.is_empty() {
            Some((host, None))
        } else if let Some(port_str) = after.strip_prefix(':') {
            Some((host, Some(parse_port(port_str)?)))
        } else {
            None
        }
    } else {
        if raw.contains('[') || raw.contains(']') {
            return None;
        }
        match raw.split_once(':') {
            Some((host, port_str)) => {
                if host.is_empty() || host.contains(':') {
                    return None;
                }
                Some((host, Some(parse_port(port_str)?)))
            }
            None => Some((raw, None)),
        }
    }
}

fn parse_origin(raw: &str) -> Option<(u16, &str, Option<u16>)> {
    let (scheme, rest) = raw.split_once("://")?;
    let default_port = if scheme.eq_ignore_ascii_case("http") {
        80
    } else if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        return None;
    };
    let (host, port) = parse_authority(rest)?;
    Some((default_port, host, port))
}

fn single_header_str<'a>(headers: &'a HeaderMap, name: header::HeaderName) -> Option<Option<&'a str>> {
    let mut iter = headers.get_all(name).iter();
    let first = match iter.next() {
        Some(v) => v,
        None => return Some(None),
    };
    if iter.next().is_some() {
        return None;
    }
    first.to_str().ok().map(Some)
}

/// Validates `Origin` against the `Host` header for browser-originated `/mcp` requests.
///
/// Absent `Origin` is allowed (non-browser CLI/agent clients). When present, `Origin`
/// must use `http`/`https`, contain no userinfo/path/query/fragment, and match the
/// single `Host` header case-insensitively on host and effective port. `X-Forwarded-Host`
/// is never consulted.
pub fn origin_allowed(headers: &HeaderMap) -> bool {
    let origin_raw = match single_header_str(headers, header::ORIGIN) {
        Some(Some(v)) => v,
        Some(None) => return true,
        None => return false,
    };
    let Some((default_port, origin_host, origin_port)) = parse_origin(origin_raw) else {
        return false;
    };
    let Some(Some(host_raw)) = single_header_str(headers, header::HOST) else {
        return false;
    };
    let Some((host_name, host_port)) = parse_authority(host_raw) else {
        return false;
    };
    if !origin_host.eq_ignore_ascii_case(host_name) {
        return false;
    }
    match host_port {
        Some(hp) => origin_port.unwrap_or(default_port) == hp,
        None => origin_port.is_none() || origin_port == Some(default_port),
    }
}

/// `GET /api/mcp/status` —— returns whether external MCP endpoints are enabled and
/// the supported protocol versions.
pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "enabled": state.mcp.enabled,
            "protocol_versions": SUPPORTED_PROTOCOL_VERSIONS,
            "endpoint": "/mcp",
            "server": ServerIdentity::from_state(&state),
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::HeaderMap;

    use super::*;

    fn mk_headers(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(o) = origin {
            h.insert("Origin", o.parse().unwrap());
        }
        if let Some(v) = host {
            h.insert("Host", v.parse().unwrap());
        }
        h
    }

    #[test]
    fn origin_validation_handles_absent_same_origin_ipv6_default_ports_and_spoofs() {
        assert!(origin_allowed(&mk_headers(None, Some("127.0.0.1:7683"))));
        assert!(origin_allowed(&mk_headers(
            Some("http://127.0.0.1:7683"),
            Some("127.0.0.1:7683")
        )));
        assert!(origin_allowed(&mk_headers(
            Some("http://[::1]:7683"),
            Some("[::1]:7683")
        )));
        assert!(origin_allowed(&mk_headers(
            Some("https://TERM.example.com:443"),
            Some("term.example.com")
        )));
        assert!(origin_allowed(&mk_headers(
            Some("http://term.example.com"),
            Some("term.example.com:80")
        )));

        assert!(!origin_allowed(&mk_headers(
            Some("http://127.0.0.1.evil.com:7683"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("http://127.0.0.1:9999"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("null"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("file://"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(Some("http://127.0.0.1:7683"), None)));
    }

    #[test]
    fn origin_validation_rejects_userinfo_paths_multi_headers_and_forwarded_host() {
        assert!(!origin_allowed(&mk_headers(
            Some("http://user@127.0.0.1:7683"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("http://127.0.0.1:7683/path"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("http://127.0.0.1:7683?q=1"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("http://127.0.0.1:7683#frag"),
            Some("127.0.0.1:7683")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("http://term.example.com:443"),
            Some("term.example.com")
        )));
        assert!(!origin_allowed(&mk_headers(
            Some("https://term.example.com"),
            Some("term.example.com:80")
        )));

        let mut multi_host = mk_headers(Some("http://127.0.0.1:7683"), Some("127.0.0.1:7683"));
        multi_host.append("Host", "127.0.0.1:7683".parse().unwrap());
        assert!(!origin_allowed(&multi_host));

        let mut forwarded = mk_headers(Some("http://evil.example.com"), Some("127.0.0.1:7683"));
        forwarded.insert("X-Forwarded-Host", "evil.example.com".parse().unwrap());
        assert!(!origin_allowed(&forwarded));
    }

    fn test_state(enabled: bool) -> AppState {
        AppState {
            manager: crate::session::manager::SessionManager::new(1 << 20, Default::default()),
            token: "secret".into(),
            launchers: Arc::new(Vec::new()),
            listen_port: 0,
            agents: Default::default(),
            agent_sessions: Arc::new(crate::agent::store::AgentSessionStore::new()),
            cloud: Default::default(),
            web_title: Default::default(),
            mcp: crate::config::McpConfig {
                enabled,
                ..Default::default()
            },
        }
    }

    #[test]
    fn server_identity_resolves_priority_hot_web_title_port_tag_and_unique_instance_ids() {
        let mut state = test_state(true);
        state.listen_port = 7683;
        state.web_title.set(Some("web-box".into()));
        let id_web = ServerIdentity::from_state(&state);
        assert_eq!(id_web.name, "web-box");
        assert_eq!(id_web.port, Some(7683));
        assert_eq!(id_web.os, std::env::consts::OS);
        assert_eq!(id_web.arch, std::env::consts::ARCH);
        assert_eq!(id_web.instance_id.len(), 32);
        assert_eq!(id_web.text_tag(), format!("web-box | {}:7683", std::env::consts::OS));

        // Hot web_title update reflects immediately when mcp.name is unset.
        state.web_title.set(Some("renamed-box".into()));
        let id_hot = ServerIdentity::from_state(&state);
        assert_eq!(id_hot.name, "renamed-box");
        assert_eq!(id_hot.instance_id, id_web.instance_id);

        // Explicit [mcp].name overrides web_title; description is normalized.
        state.mcp.name = Some("  mac-build\n".into());
        state.mcp.description = Some("  iOS\tbuilder  ".into());
        let id_mcp = ServerIdentity::from_state(&state);
        assert_eq!(id_mcp.name, "mac-build");
        assert_eq!(id_mcp.description.as_deref(), Some("iOS builder"));

        // Two managers on the same host (e.g. :7683 and :17683) have distinct instance_ids and port tags.
        let mut state2 = test_state(true);
        state2.listen_port = 17683;
        let id2 = ServerIdentity::from_state(&state2);
        assert_ne!(id_mcp.instance_id, id2.instance_id);
        assert!(id2.text_tag().ends_with(":17683"));

        // Port 0 omits port in JSON and text_tag.
        let zero_port = ServerIdentity::from_state(&test_state(true));
        assert_eq!(zero_port.port, None);
        assert!(!zero_port.text_tag().contains(':'));
    }

    #[tokio::test]
    async fn status_endpoint_requires_bearer_and_returns_no_store_payload() {
        let state = test_state(true);

        let unauth = status(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        let mut bad_headers = HeaderMap::new();
        bad_headers.insert(header::AUTHORIZATION, "Bearer wrong".parse().unwrap());
        let bad = status(State(state.clone()), bad_headers).await;
        assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

        let mut ok_headers = HeaderMap::new();
        ok_headers.insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let ok = status(State(state.clone()), ok_headers.clone()).await;
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(
            ok.headers().get(header::CACHE_CONTROL).and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        let bytes = axum::body::to_bytes(ok.into_body(), 1024 * 16).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["enabled"], true);
        assert_eq!(
            json["protocol_versions"],
            serde_json::json!(SUPPORTED_PROTOCOL_VERSIONS)
        );
        assert_eq!(json["endpoint"], "/mcp");
        assert_eq!(json["server"]["instance_id"], state.manager.instance_id());
        assert_eq!(json["server"]["os"], std::env::consts::OS);
        assert_eq!(json["server"]["arch"], std::env::consts::ARCH);

        let disabled_state = test_state(false);
        let disabled = status(State(disabled_state.clone()), ok_headers).await;
        assert_eq!(disabled.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(disabled.into_body(), 1024 * 16).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["enabled"], false);
        assert_eq!(
            json["server"]["instance_id"],
            disabled_state.manager.instance_id()
        );
        assert_eq!(json["server"]["os"], std::env::consts::OS);
        assert_eq!(json["server"]["arch"], std::env::consts::ARCH);
    }

    #[tokio::test]
    async fn api_mcp_status_route_serves_authenticated_json_and_rejects_query_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = test_state(true);
        let expected_instance_id = state.manager.instance_id().to_string();
        let server = tokio::spawn(async move {
            let _ = crate::app::serve(listener, state).await;
        });

        let client = reqwest::Client::new();
        let unauth = client
            .get(format!("http://{addr}/api/mcp/status"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), 401);

        let query_tok = client
            .get(format!("http://{addr}/api/mcp/status?token=secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(query_tok.status(), 401);

        let ok = client
            .get(format!("http://{addr}/api/mcp/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), 200);
        assert_eq!(
            ok.headers().get("cache-control").and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        let body: serde_json::Value = ok.json().await.unwrap();
        assert_eq!(body["enabled"], true);
        assert_eq!(
            body["protocol_versions"],
            serde_json::json!(SUPPORTED_PROTOCOL_VERSIONS)
        );
        assert_eq!(body["endpoint"], "/mcp");
        assert_eq!(body["server"]["instance_id"], expected_instance_id);
        assert_eq!(body["server"]["os"], std::env::consts::OS);
        assert_eq!(body["server"]["arch"], std::env::consts::ARCH);

        server.abort();
    }
}
