use std::time::Duration;

use anyhow::{Context, Result, bail};
use gateterm_protocol::{AgentState, SessionInfo};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::mcp::{McpServerIdentity, McpServerStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherItem {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub detail: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LaunchersEnvelope {
    pub launchers: Vec<LauncherItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionInfo {
    pub agent_id: String,
    pub agent_session_id: String,
    #[serde(default)]
    pub session: Option<u64>,
    pub state: AgentState,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub pending: Option<String>,
    #[serde(default)]
    pub supports_transcript: bool,
}

impl AgentSessionInfo {
    pub fn key(&self) -> String {
        format!("{} {}", self.agent_id, self.agent_session_id)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentSessionsResponse {
    #[serde(default)]
    pub sessions: Vec<AgentSessionInfo>,
    #[serde(default)]
    pub transcript_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequestInfo {
    pub id: String,
    pub agent_id: String,
    pub agent_session_id: String,
    #[serde(default)]
    pub session: Option<u64>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Value,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub expires_at: u64,
}

impl PendingRequestInfo {
    pub fn summary(&self) -> String {
        let tool = self.tool_name.as_deref().unwrap_or("Tool");
        if let Some(cmd) = self.tool_input.get("command").and_then(|v| v.as_str()) {
            return format!("{tool}: {cmd}");
        }
        if let Some(path) = self.tool_input.get("file_path").and_then(|v| v.as_str()) {
            return format!("{tool}: {path}");
        }
        if let Some(path) = self.tool_input.get("path").and_then(|v| v.as_str()) {
            return format!("{tool}: {path}");
        }
        if self.tool_input.is_null() {
            tool.to_string()
        } else {
            let raw = self.tool_input.to_string();
            let short: String = raw.chars().take(120).collect();
            format!("{tool}: {short}")
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PendingEnvelope {
    #[serde(default)]
    pub pending: Vec<PendingRequestInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ChatBlock {
    Text { text: String },
    Thinking { text: String },
    ToolUse { name: String, input: String },
    ToolResult { ok: bool, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    #[serde(default)]
    pub ts: Option<u64>,
    #[serde(default)]
    pub blocks: Vec<ChatBlock>,
}

fn deserialize_present_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(
        Option::<String>::deserialize(deserializer)?.filter(|s| !s.is_empty()),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum ChatWindow {
    Transcript {
        #[serde(default)]
        messages: Vec<ChatMessage>,
        #[serde(default)]
        cursor: String,
        #[serde(default)]
        reset: bool,
        #[serde(default)]
        more: bool,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_present_opt"
        )]
        older_cursor: Option<Option<String>>,
    },
    Terminal {
        #[serde(default)]
        reason: String,
    },
}

impl ChatWindow {
    pub fn older_cursor(&self) -> Option<&str> {
        match self {
            Self::Transcript { older_cursor, .. } => older_cursor
                .as_ref()
                .and_then(|o| o.as_deref())
                .filter(|s| !s.is_empty()),
            Self::Terminal { .. } => None,
        }
    }

    pub fn history_supported(&self) -> bool {
        match self {
            Self::Transcript { older_cursor, .. } => older_cursor.is_some(),
            Self::Terminal { .. } => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub agent_id: String,
    pub agent_name: String,
    pub agent_session_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub terminal_session: Option<u64>,
    #[serde(default)]
    pub can_resume: bool,
    #[serde(default)]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryAgentFilter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub installed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryPage {
    #[serde(default)]
    pub sessions: Vec<HistoryEntry>,
    #[serde(default)]
    pub agents: Vec<HistoryAgentFilter>,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub next_offset: Option<usize>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub skipped: usize,
    #[serde(default)]
    pub indexing: bool,
    #[serde(default)]
    pub workspaces: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryOpenResponse {
    pub session: SessionInfo,
    #[serde(default)]
    pub reused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIntegrationStatus {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub integration: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AgentsStatusEnvelope {
    #[serde(default)]
    pub agents: Vec<AgentIntegrationStatus>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudStatus {
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub email: Option<String>,
}

fn deserialize_optional_server_identity<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<McpServerIdentity>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<Value>::deserialize(deserializer)?;
    let Some(Value::Object(map)) = raw else {
        return Ok(None);
    };
    let str_field = |key: &str| map.get(key).and_then(Value::as_str).map(ToOwned::to_owned);
    let port = map
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|n| u16::try_from(n).ok());
    Ok(Some(McpServerIdentity {
        instance_id: str_field("instance_id"),
        name: str_field("name"),
        hostname: str_field("hostname"),
        os: str_field("os"),
        arch: str_field("arch"),
        port,
        description: str_field("description"),
    }))
}

#[derive(Debug, Deserialize)]
struct McpStatusBody {
    enabled: bool,
    #[serde(default)]
    protocol_versions: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_optional_server_identity")]
    server: Option<McpServerIdentity>,
}

#[derive(Clone)]
pub struct GateHttpClient {
    http: Client,
    base_url: String,
    token: String,
}

impl GateHttpClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub async fn fetch_mcp_status(&self) -> McpServerStatus {
        let resp = match self
            .http
            .get(self.url("/api/mcp/status"))
            .bearer_auth(&self.token)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(err) => return McpServerStatus::Error(err.to_string()),
        };
        let status = resp.status();
        if status.as_u16() == 404 {
            return McpServerStatus::Unsupported;
        }
        if !status.is_success() {
            return McpServerStatus::Error(format!("HTTP {status}"));
        }
        match resp.json::<McpStatusBody>().await {
            Ok(body) if body.enabled => McpServerStatus::Enabled {
                protocol_versions: body.protocol_versions,
                server: body.server,
            },
            Ok(body) => McpServerStatus::Disabled {
                server: body.server,
            },
            Err(err) => McpServerStatus::Error(err.to_string()),
        }
    }

    pub async fn fetch_launchers(&self) -> Result<Vec<LauncherItem>> {
        let resp = self
            .http
            .get(self.url("/api/launchers"))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GET /api/launchers failed")?;
        if !resp.status().is_success() {
            bail!("GET /api/launchers returned {}", resp.status());
        }
        let env: LaunchersEnvelope = resp.json().await?;
        Ok(env.launchers)
    }

    pub async fn fetch_agent_sessions(&self) -> Result<AgentSessionsResponse> {
        let resp = self
            .http
            .get(self.url("/api/agents/sessions"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(AgentSessionsResponse::default());
        }
        Ok(resp.json().await.unwrap_or_default())
    }

    pub async fn fetch_pending(&self) -> Result<Vec<PendingRequestInfo>> {
        let resp = self
            .http
            .get(self.url("/api/agents/pending"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let env: PendingEnvelope = resp.json().await.unwrap_or_default();
        Ok(env.pending)
    }

    pub async fn answer_pending(
        &self,
        pending_id: &str,
        decision: &str,
        message: Option<String>,
        updated_input: Option<Value>,
    ) -> Result<()> {
        let body = serde_json::json!({
            "pending_id": pending_id,
            "decision": decision,
            "message": message,
            "updated_input": updated_input,
        });
        let resp = self
            .http
            .post(self.url("/api/agents/answer"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() && resp.status().as_u16() != 409 {
            bail!("POST /api/agents/answer returned {}", resp.status());
        }
        Ok(())
    }

    pub async fn fetch_messages(
        &self,
        agent_id: &str,
        agent_session_id: &str,
        after: Option<&str>,
        before: Option<&str>,
    ) -> ChatWindow {
        let mut req = self.http.get(self.url("/api/agents/messages")).bearer_auth(&self.token).query(&[
            ("agent_id", agent_id),
            ("agent_session_id", agent_session_id),
        ]);
        if let Some(a) = after {
            req = req.query(&[("after", a)]);
        }
        if let Some(b) = before.filter(|s| !s.is_empty()) {
            req = req.query(&[("before", b)]);
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<ChatWindow>().await.unwrap_or(ChatWindow::Terminal {
                    reason: "parse-failed".into(),
                })
            }
            _ => ChatWindow::Terminal {
                reason: "unreadable".into(),
            },
        }
    }

    pub async fn fetch_history(
        &self,
        q: &str,
        agent_id: &str,
        offset: usize,
        refresh: bool,
    ) -> Result<HistoryPage> {
        let resp = self
            .http
            .get(self.url("/api/agents/history"))
            .bearer_auth(&self.token)
            .query(&[
                ("q", q),
                ("agent_id", agent_id),
                ("offset", &offset.to_string()),
                ("refresh", if refresh { "true" } else { "false" }),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("GET /api/agents/history returned {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn fetch_history_messages(
        &self,
        agent_id: &str,
        agent_session_id: &str,
        before: Option<&str>,
    ) -> Result<ChatWindow> {
        let mut req = self
            .http
            .get(self.url("/api/agents/history/messages"))
            .bearer_auth(&self.token)
            .query(&[
                ("agent_id", agent_id),
                ("agent_session_id", agent_session_id),
            ]);
        if let Some(b) = before.filter(|s| !s.is_empty()) {
            req = req.query(&[("before", b)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            bail!("GET /api/agents/history/messages returned {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn open_history(
        &self,
        agent_id: &str,
        agent_session_id: &str,
    ) -> Result<HistoryOpenResponse> {
        let body = serde_json::json!({
            "agent_id": agent_id,
            "agent_session_id": agent_session_id,
        });
        let resp = self
            .http
            .post(self.url("/api/agents/history/open"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("POST /api/agents/history/open returned {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn fetch_agents_status(&self) -> Result<Vec<AgentIntegrationStatus>> {
        let resp = self
            .http
            .get(self.url("/api/agents"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let env: AgentsStatusEnvelope = resp.json().await.unwrap_or_default();
        Ok(env.agents)
    }

    pub async fn mutate_agent_hook(&self, agent_id: &str, action: &str) -> Result<()> {
        let path = format!("/api/agents/{agent_id}/{action}");
        let resp = self
            .http
            .post(self.url(&path))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("POST {path} returned {}", resp.status());
        }
        Ok(())
    }

    pub async fn fetch_cloud_status(&self) -> Result<CloudStatus> {
        let resp = self
            .http
            .get(self.url("/api/cloud/status"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(CloudStatus::default());
        }
        Ok(resp.json().await.unwrap_or_default())
    }

    pub async fn upload_pasted_image(&self, bytes: Vec<u8>) -> Result<String> {
        #[derive(Deserialize)]
        struct PasteImageResp {
            path: String,
        }
        let resp = self
            .http
            .post(self.url("/api/paste-image"))
            .bearer_auth(&self.token)
            .body(bytes)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("POST /api/paste-image returned {}", resp.status());
        }
        let parsed: PasteImageResp = resp.json().await?;
        Ok(parsed.path)
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    async fn serve_one_http_response(
        status_line: &str,
        body: &str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let resp_str = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = sock.write_all(resp_str.as_bytes()).await;
            let _ = sock.shutdown().await;
            req
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn fetch_mcp_status_maps_enabled_disabled_404_and_errors() {
        let (base_ok, req_ok) = serve_one_http_response(
            "200 OK",
            r#"{"enabled":true,"protocol_versions":["2025-11-25","2025-06-18"],"endpoint":"/mcp"}"#,
        )
        .await;
        let client_ok = GateHttpClient::new(format!("{base_ok}/gateterm/"), "tok-123");
        let status_ok = client_ok.fetch_mcp_status().await;
        assert_eq!(
            status_ok,
            McpServerStatus::Enabled {
                protocol_versions: vec!["2025-11-25".to_string(), "2025-06-18".to_string()],
                server: None,
            }
        );
        let raw_req = req_ok.await.unwrap();
        assert!(raw_req.starts_with("GET /gateterm/api/mcp/status "));
        assert!(raw_req.to_ascii_lowercase().contains("authorization: bearer tok-123"));

        let (base_ok_srv, _) = serve_one_http_response(
            "200 OK",
            r#"{"enabled":true,"protocol_versions":["2025-11-25"],"endpoint":"/mcp","server":{"instance_id":"a1b2c3d4e5f64789a0b1c2d3e4f56789","name":"mac-build","hostname":"Mac-Studio.local","os":"macos","arch":"aarch64","port":7683,"description":"macOS arm64 iOS build"}}"#,
        )
        .await;
        assert_eq!(
            GateHttpClient::new(base_ok_srv, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Enabled {
                protocol_versions: vec!["2025-11-25".to_string()],
                server: Some(McpServerIdentity {
                    instance_id: Some("a1b2c3d4e5f64789a0b1c2d3e4f56789".to_string()),
                    name: Some("mac-build".to_string()),
                    hostname: Some("Mac-Studio.local".to_string()),
                    os: Some("macos".to_string()),
                    arch: Some("aarch64".to_string()),
                    port: Some(7683),
                    description: Some("macOS arm64 iOS build".to_string()),
                }),
            }
        );

        let (base_dis, _) =
            serve_one_http_response("200 OK", r#"{"enabled":false,"protocol_versions":[]}"#).await;
        assert_eq!(
            GateHttpClient::new(base_dis, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Disabled { server: None }
        );

        let (base_dis_srv, _) = serve_one_http_response(
            "200 OK",
            r#"{"enabled":false,"protocol_versions":["2025-11-25"],"endpoint":"/mcp","server":{"instance_id":"deadbeef","name":"win-box","os":"windows","arch":"x86_64"}}"#,
        )
        .await;
        assert_eq!(
            GateHttpClient::new(base_dis_srv, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Disabled {
                server: Some(McpServerIdentity {
                    instance_id: Some("deadbeef".to_string()),
                    name: Some("win-box".to_string()),
                    hostname: None,
                    os: Some("windows".to_string()),
                    arch: Some("x86_64".to_string()),
                    port: None,
                    description: None,
                }),
            }
        );

        let (base_404, _) = serve_one_http_response("404 Not Found", "").await;
        assert_eq!(
            GateHttpClient::new(base_404, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Unsupported
        );

        let (base_401, _) = serve_one_http_response("401 Unauthorized", "").await;
        assert!(matches!(
            GateHttpClient::new(base_401, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Error(_)
        ));

        let (base_bad_json, _) = serve_one_http_response("200 OK", r#"{"oops":true}"#).await;
        assert!(matches!(
            GateHttpClient::new(base_bad_json, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Error(_)
        ));

        let (base_malformed_server, _) = serve_one_http_response(
            "200 OK",
            r#"{"enabled":true,"protocol_versions":["2025-11-25"],"server":["not-an-object"]}"#,
        )
        .await;
        assert_eq!(
            GateHttpClient::new(base_malformed_server, "tok-123")
                .fetch_mcp_status()
                .await,
            McpServerStatus::Enabled {
                protocol_versions: vec!["2025-11-25".to_string()],
                server: None,
            }
        );
    }

    #[test]
    fn chat_window_transcript_distinguishes_older_cursor_presence() {
        let old_server: ChatWindow =
            serde_json::from_str(r#"{"source":"transcript","messages":[],"cursor":"c1"}"#)
                .unwrap();
        assert!(!old_server.history_supported());
        assert_eq!(old_server.older_cursor(), None);

        let at_beginning: ChatWindow = serde_json::from_str(
            r#"{"source":"transcript","messages":[],"cursor":"c1","older_cursor":null}"#,
        )
        .unwrap();
        assert!(at_beginning.history_supported());
        assert_eq!(at_beginning.older_cursor(), None);

        let has_older: ChatWindow = serde_json::from_str(
            r#"{"source":"transcript","messages":[],"cursor":"c1","older_cursor":"opaque-1"}"#,
        )
        .unwrap();
        assert!(has_older.history_supported());
        assert_eq!(has_older.older_cursor(), Some("opaque-1"));

        let term: ChatWindow =
            serde_json::from_str(r#"{"source":"terminal","reason":"unreadable"}"#).unwrap();
        assert!(!term.history_supported());
        assert_eq!(term.older_cursor(), None);
    }

    #[tokio::test]
    async fn fetch_history_messages_encodes_optional_before_query_param() {
        let body = r#"{"source":"transcript","messages":[],"cursor":"c1","older_cursor":null}"#;

        let (base_no_before, req_no_before) = serve_one_http_response("200 OK", body).await;
        let win = GateHttpClient::new(base_no_before, "tok")
            .fetch_history_messages("codex", "sess-1", None)
            .await
            .unwrap();
        assert!(win.history_supported());
        let raw_no_before = req_no_before.await.unwrap();
        let line_no_before = raw_no_before.lines().next().unwrap();
        assert!(line_no_before.contains("/api/agents/history/messages?"));
        assert!(line_no_before.contains("agent_id=codex"));
        assert!(line_no_before.contains("agent_session_id=sess-1"));
        assert!(!line_no_before.contains("before="));

        let (base_empty_before, req_empty_before) = serve_one_http_response("200 OK", body).await;
        GateHttpClient::new(base_empty_before, "tok")
            .fetch_history_messages("codex", "sess-1", Some(""))
            .await
            .unwrap();
        let raw_empty_before = req_empty_before.await.unwrap();
        assert!(!raw_empty_before.lines().next().unwrap().contains("before="));

        let (base_with_before, req_with_before) = serve_one_http_response("200 OK", body).await;
        GateHttpClient::new(base_with_before, "tok")
            .fetch_history_messages("codex", "sess-1", Some("cur/a+b"))
            .await
            .unwrap();
        let raw_with_before = req_with_before.await.unwrap();
        let line_with_before = raw_with_before.lines().next().unwrap();
        assert!(line_with_before.contains("before=cur%2Fa%2Bb"));
    }
}
