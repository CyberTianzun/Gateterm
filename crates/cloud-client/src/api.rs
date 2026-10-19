//! Cloud HTTP API。错误码与 OctotermCloud 的 `{"error": "<code>"}` 一一对应。

use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudError {
    /// 网络不通、超时、响应体解析失败。
    Unreachable(String),
    /// 凭据已被撤销(他处退出 / 删号 / 重置密码)。调用方应删除本地 cloud.json。
    Unauthorized,
    RateLimited { retry_after: Option<u64> },
    Api { status: u16, code: String },
}

impl CloudError {
    pub fn user_message(&self) -> String {
        match self {
            CloudError::Unreachable(e) => format!("无法连接 Cloud:{e}"),
            CloudError::Unauthorized => "Cloud 登录已失效,请重新登录".into(),
            CloudError::RateLimited { retry_after: Some(s) } => format!("操作太频繁,请 {s} 秒后再试"),
            CloudError::RateLimited { retry_after: None } => "操作太频繁,请稍后再试".into(),
            CloudError::Api { code, .. } => match code.as_str() {
                "invalid_credentials" => "邮箱或密码错误".into(),
                "invalid_code" => "验证码错误或已过期".into(),
                "code_exhausted" => "验证码错误次数过多,请重新获取".into(),
                "email_taken" => "该邮箱已注册".into(),
                "weak_password" => "密码长度需为 8–128 个字符".into(),
                "invalid_email" => "邮箱格式不正确".into(),
                "wrong_password" => "密码错误".into(),
                other => format!("Cloud 返回错误:{other}"),
            },
        }
    }
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for CloudError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    Device,
    Server,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub kind: ClientKind,
    pub label: String,
}

impl ClientInfo {
    pub fn server(label: &str) -> Self {
        Self { kind: ClientKind::Server, label: label.to_string() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePurpose {
    Register,
    Reset,
}

impl CodePurpose {
    fn as_str(self) -> &'static str {
        match self {
            CodePurpose::Register => "register",
            CodePurpose::Reset => "reset",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthOk {
    pub token: String,
    pub account_id: String,
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    pub label: String,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountInfo {
    pub email: Option<String>,
    pub servers: Vec<ServerEntry>,
    pub devices: Vec<DeviceEntry>,
}

/// `/v1/notify` 的请求体。只有路由所需的最小信息,不含工具入参。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notify {
    pub session: u64,
    pub agent_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// 登录时给 Cloud 看的这台机器的名字。
pub fn default_label() -> String {
    let name: String = gethostname::gethostname().to_string_lossy().trim().chars().take(64).collect();
    if name.is_empty() { "octoterm".into() } else { name }
}

pub struct CloudClient {
    http: reqwest::Client,
    base: String,
}

impl CloudClient {
    pub fn new(base: &str) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap_or_default();
        Self { http, base: base.trim().trim_end_matches('/').to_string() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn send(&self, req: RequestBuilder) -> Result<Response, CloudError> {
        let resp = req.send().await.map_err(|e| CloudError::Unreachable(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok());
            return Err(CloudError::RateLimited { retry_after });
        }
        let code = resp
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["error"].as_str().map(String::from))
            .unwrap_or_else(|| status.as_str().to_string());
        // 只有凭据失效是 `unauthorized`;登录失败是 401 + `invalid_credentials`,不能混为一谈。
        if code == "unauthorized" {
            return Err(CloudError::Unauthorized);
        }
        Err(CloudError::Api { status: status.as_u16(), code })
    }

    async fn send_json<T: serde::de::DeserializeOwned>(&self, req: RequestBuilder) -> Result<T, CloudError> {
        self.send(req).await?.json().await.map_err(|e| CloudError::Unreachable(e.to_string()))
    }

    pub async fn request_code(&self, email: &str, purpose: CodePurpose) -> Result<(), CloudError> {
        let body = json!({"email": email, "purpose": purpose.as_str()});
        self.send(self.http.post(self.url("/v1/auth/email-code")).json(&body)).await.map(drop)
    }

    pub async fn register(&self, email: &str, code: &str, password: &str, client: &ClientInfo) -> Result<AuthOk, CloudError> {
        let body = json!({"email": email, "code": code, "password": password, "client": client});
        self.send_json(self.http.post(self.url("/v1/auth/register")).json(&body)).await
    }

    pub async fn login(&self, email: &str, password: &str, client: &ClientInfo) -> Result<AuthOk, CloudError> {
        let body = json!({"email": email, "password": password, "client": client});
        self.send_json(self.http.post(self.url("/v1/auth/login")).json(&body)).await
    }

    pub async fn reset_password(&self, email: &str, code: &str, new_password: &str) -> Result<(), CloudError> {
        let body = json!({"email": email, "code": code, "new_password": new_password});
        self.send(self.http.post(self.url("/v1/auth/password-reset")).json(&body)).await.map(drop)
    }

    pub async fn logout(&self, token: &str) -> Result<(), CloudError> {
        self.send(self.http.post(self.url("/v1/auth/logout")).bearer_auth(token)).await.map(drop)
    }

    pub async fn account(&self, token: &str) -> Result<AccountInfo, CloudError> {
        self.send_json(self.http.get(self.url("/v1/account")).bearer_auth(token)).await
    }

    pub async fn request_delete_code(&self, token: &str) -> Result<(), CloudError> {
        self.send(self.http.post(self.url("/v1/account/delete-code")).bearer_auth(token)).await.map(drop)
    }

    pub async fn delete_account(&self, token: &str, password: &str, code: &str) -> Result<(), CloudError> {
        let body = json!({"password": password, "code": code});
        self.send(self.http.delete(self.url("/v1/account")).bearer_auth(token).json(&body)).await.map(drop)
    }

    /// 返回 Cloud 实际投递到的设备数。
    pub async fn notify(&self, token: &str, n: &Notify) -> Result<u32, CloudError> {
        let v: Value = self.send_json(self.http.post(self.url("/v1/notify")).bearer_auth(token).json(n)).await?;
        Ok(v["delivered"].as_u64().unwrap_or(0) as u32)
    }
}
