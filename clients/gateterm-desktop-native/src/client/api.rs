use std::time::Duration;

use anyhow::{Context, Result, bail};
use gateterm_protocol::{AgentState, SessionInfo};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        #[serde(default)]
        older_cursor: Option<String>,
    },
    Terminal {
        #[serde(default)]
        reason: String,
    },
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
        if let Some(b) = before {
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
    ) -> Result<ChatWindow> {
        let resp = self
            .http
            .get(self.url("/api/agents/history/messages"))
            .bearer_auth(&self.token)
            .query(&[
                ("agent_id", agent_id),
                ("agent_session_id", agent_session_id),
            ])
            .send()
            .await?;
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
