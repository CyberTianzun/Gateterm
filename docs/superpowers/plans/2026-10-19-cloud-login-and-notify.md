# Octoterm Cloud 登录与推送接入 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 octoterm-server / desktop 可选地登录 Octoterm Cloud(注册、登录、退出、找回密码、删除账号),并在 agent 进入 waiting 时调用 Cloud 的 `/v1/notify`;不登录时行为完全不变。

**Architecture:** 新 crate `crates/cloud-client` 封装 Cloud HTTP API、`cloud.json` 凭据文件与「调用 + 落盘」组合操作(`ops`),server CLI 与 desktop 设置窗口都只是它的薄壳。server 内新增 `cloud::notifier`,订阅 `SessionManager::events()` 的 `AgentEvent`,只在状态迁移进入 waiting / 出现新 pending 时异步推一次;每次发送前按 mtime 重读 `cloud.json`,所以 desktop 登录/退出无需重启。

**Tech Stack:** Rust 2024、reqwest 0.12(rustls)、serde、directories 5、gethostname、rpassword、axum 0.8(测试用假 Cloud)、egui 0.36。

**Spec:** `~/online/eclosion/OctotermCloud/docs/specs/2026-10-19-octoterm-cloud-account-push-design.md` §6
**依赖:** `~/online/eclosion/OctotermCloud/docs/plans/2026-10-19-cloud-account-push.md` 已完成(Task 6 的端到端冒烟需要它)。

## Global Constraints

- 不登录 Cloud 时,server / desktop 的行为与现在完全一致;Cloud 任何失败都不得阻塞 hook、WS 推送或 UI。
- Cloud 永远拿不到 server 的 bearer token 与 hook token;`/v1/notify` 只发 `{session, agent_id, kind, tool_name?}`,**不发工具入参、不发 detail**。
- 凭据文件 `<ProjectDirs("", "", "octoterm").config_dir()>/cloud.json`(与 `config.toml`、`hook-token` 同目录),Unix 权限 `0600`;环境变量 `OCTOTERM_CLOUD_FILE` 可覆盖路径(测试与冒烟用)。
- 收到 Cloud 的 `401 {"error":"unauthorized"}` = 凭据已被撤销,删除本地 `cloud.json`。
- `kind`:`pending` 有值为 `permission`,否则 `waiting`;同一 agent 会话持续 waiting 不重复推;`done` 不推。
- 登录时 `client.kind = "server"`(desktop 也是 server),`label` = 主机名(截断 64 字符,空则 `octoterm`)。
- 提交流程:在 `feat/cloud-login` 分支上每个 Task 提交一次,完成后 `git merge --ff-only` 回 `dev`,不推送。commit message 结尾加 `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`。

## File Structure

```
Cargo.toml                                   workspace 加 crates/cloud-client
crates/cloud-client/Cargo.toml
crates/cloud-client/src/lib.rs               re-export
crates/cloud-client/src/api.rs               CloudClient、CloudError、请求/响应类型、default_label
crates/cloud-client/src/credentials.rs       Credentials、default_path、load/save/delete
crates/cloud-client/src/ops.rs               调用 + 落盘组合:register/login/logout/reset/delete
crates/cloud-client/tests/common/mod.rs      FakeCloud(axum)
crates/cloud-client/tests/api.rs
crates/cloud-client/tests/ops.rs
crates/server/Cargo.toml                     加 cloud-client、rpassword
crates/server/src/config.rs                  [cloud] url
crates/server/src/lib.rs                     pub mod cloud
crates/server/src/cloud/mod.rs
crates/server/src/cloud/notifier.rs          AgentEvent → /v1/notify
crates/server/src/cloud/cli.rs               `octoterm-server cloud ...`
crates/server/src/main.rs                    Cmd::Cloud、启动 notifier
crates/server/tests/cloud_notifier.rs
crates/desktop/Cargo.toml                    加 cloud-client
crates/desktop/src/main.rs                   启动 notifier
crates/desktop/src/supervisor.rs             agent_sessions() 访问器
crates/desktop/src/settings/mod.rs           pub mod cloud
crates/desktop/src/settings/cloud.rs         CloudForm/CloudStatus/CloudAction/CloudJob + prepare(纯逻辑)
crates/desktop/src/cloud_ops.rs              执行 CloudJob(调用 ops)
crates/desktop/src/settings/ui.rs            「Cloud 账号」区
crates/desktop/src/app.rs                    Outcome::Cloud、UserEvent::CloudDone
README.md / README-cnzh.md                   Cloud 一节
```

---

### Task 1: cloud-client crate —— 凭据文件与 HTTP 客户端

**Files:**
- Create: `crates/cloud-client/Cargo.toml`, `crates/cloud-client/src/lib.rs`, `crates/cloud-client/src/api.rs`, `crates/cloud-client/src/credentials.rs`, `crates/cloud-client/tests/common/mod.rs`, `crates/cloud-client/tests/api.rs`
- Modify: `Cargo.toml`(workspace members)

**Interfaces:**
- Produces:
  - `credentials::{Credentials { url, email, token, server_id: Option<String> }, default_path() -> Option<PathBuf>, load(&Path) -> io::Result<Option<Credentials>>, save(&Path, &Credentials) -> io::Result<()>, delete(&Path) -> io::Result<()>}`
  - `api::{CloudClient, CloudError, ClientKind, ClientInfo, CodePurpose, AuthOk, AccountInfo, ServerEntry, DeviceEntry, Notify, default_label}`
  - `CloudClient::new(base: &str)`;async 方法:`request_code(email, CodePurpose)`, `register(email, code, password, &ClientInfo) -> AuthOk`, `login(email, password, &ClientInfo) -> AuthOk`, `reset_password(email, code, new_password)`, `logout(token)`, `account(token) -> AccountInfo`, `request_delete_code(token)`, `delete_account(token, password, code)`, `notify(token, &Notify) -> u32`;全部返回 `Result<_, CloudError>`
  - `CloudError::{Unreachable(String), Unauthorized, RateLimited { retry_after: Option<u64> }, Api { status: u16, code: String }}`,`user_message(&self) -> String`
  - `ClientInfo::server(label: &str) -> ClientInfo`
  - 测试夹具 `common::FakeCloud { url, seen }`,`FakeCloud::start()`、`respond(key, status, body)`、`respond_retry_after(key, status, body, secs)`、`last(key) -> Seen`

- [ ] **Step 1: 建分支、加 crate 骨架**

```bash
cd ~/online/eclosion/Octoterm && git checkout -b feat/cloud-login
```

workspace `Cargo.toml` 的 members 改为:

```toml
members = ["crates/protocol", "crates/server", "crates/client-core", "crates/desktop", "crates/cloud-client"]
```

`crates/cloud-client/Cargo.toml`:

```toml
[package]
name = "octoterm-cloud-client"
version = "0.2.1"
edition.workspace = true
license.workspace = true

[dependencies]
directories = "5"
gethostname = "1"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
axum = "0.8"
tempfile = "3"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
```

`crates/cloud-client/src/lib.rs`:

```rust
//! Octoterm Cloud 客户端:HTTP API、本地凭据文件、以及两者组合起来的账号操作。
//! server CLI 和 desktop 设置窗口都只是这里的薄壳。

pub mod api;
pub mod credentials;

pub use api::{
    AccountInfo, AuthOk, ClientInfo, ClientKind, CloudClient, CloudError, CodePurpose, DeviceEntry, Notify,
    ServerEntry, default_label,
};
pub use credentials::Credentials;
```

- [ ] **Step 2: 写 `src/credentials.rs`(含单测)**

```rust
//! `cloud.json`:Cloud 登录态。文件存在 = 已登录。
//!
//! 里面的 token 能以这台机器的身份调用 Cloud,所以 Unix 上权限是 0600,
//! 并且先写临时文件再 rename,避免读到写了一半的文件。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub url: String,
    pub email: String,
    pub token: String,
    #[serde(default)]
    pub server_id: Option<String>,
}

/// 与 config.toml、hook-token 同目录。`OCTOTERM_CLOUD_FILE` 可覆盖(测试与冒烟用)。
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OCTOTERM_CLOUD_FILE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    directories::ProjectDirs::from("", "", "octoterm").map(|d| d.config_dir().join("cloud.json"))
}

pub fn load(path: &Path) -> io::Result<Option<Credentials>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn save(path: &Path, creds: &Credentials) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    // 先删:OpenOptions 的 mode 只在创建时生效,残留的临时文件可能是别的权限。
    let _ = fs::remove_file(&tmp);
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(creds)?)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn delete(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Credentials {
        Credentials {
            url: "https://cloud.example.com".into(),
            email: "a@example.com".into(),
            token: "oct_abc".into(),
            server_id: Some("s1".into()),
        }
    }

    #[test]
    fn roundtrip_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/cloud.json");
        assert_eq!(load(&path).unwrap(), None);
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), Some(sample()));
        delete(&path).unwrap();
        assert_eq!(load(&path).unwrap(), None);
        delete(&path).unwrap(); // 不存在也不报错
    }

    #[cfg(unix)]
    #[test]
    fn file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        save(&path, &sample()).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn corrupt_file_is_an_error_not_logged_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        fs::write(&path, b"{not json").unwrap();
        assert!(load(&path).is_err());
    }
}
```

- [ ] **Step 3: 写测试夹具 `tests/common/mod.rs`**

```rust
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct Seen {
    pub key: String,
    pub auth: Option<String>,
    pub body: Value,
}

type Canned = (u16, Value, Option<u64>);

#[derive(Clone, Default)]
struct Shared {
    seen: Arc<Mutex<Vec<Seen>>>,
    canned: Arc<Mutex<HashMap<String, Canned>>>,
}

/// 假 Cloud:按 "METHOD /path" 返回预设响应,并记下每个请求。
pub struct FakeCloud {
    pub url: String,
    shared: Shared,
}

impl FakeCloud {
    pub async fn start() -> Self {
        let shared = Shared::default();
        let app = Router::new().fallback(handle).with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, shared }
    }

    pub fn respond(&self, key: &str, status: u16, body: Value) {
        self.shared.canned.lock().unwrap().insert(key.into(), (status, body, None));
    }

    pub fn respond_retry_after(&self, key: &str, status: u16, body: Value, secs: u64) {
        self.shared.canned.lock().unwrap().insert(key.into(), (status, body, Some(secs)));
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.shared.seen.lock().unwrap().clone()
    }

    pub fn last(&self, key: &str) -> Seen {
        self.seen().into_iter().rev().find(|s| s.key == key).unwrap_or_else(|| panic!("没有收到 {key}"))
    }
}

async fn handle(State(sh): State<Shared>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let key = format!("{method} {}", uri.path());
    sh.seen.lock().unwrap().push(Seen {
        key: key.clone(),
        auth: headers.get("authorization").and_then(|v| v.to_str().ok()).map(String::from),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    let (status, json, retry) =
        sh.canned.lock().unwrap().get(&key).cloned().unwrap_or((404, json!({"error": "not_found"}), None));
    let mut resp = (StatusCode::from_u16(status).unwrap(), axum::Json(json)).into_response();
    if let Some(secs) = retry {
        resp.headers_mut().insert("retry-after", secs.to_string().parse().unwrap());
    }
    resp
}
```

- [ ] **Step 4: 写失败测试 `tests/api.rs`**

```rust
mod common;

use common::FakeCloud;
use octoterm_cloud_client::{ClientInfo, CloudClient, CloudError, CodePurpose, Notify};
use serde_json::json;

#[tokio::test]
async fn login_parses_auth_and_sends_client_info() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 200, json!({"token": "oct_t", "account_id": "a1", "server_id": "s1"}));
    let ok = CloudClient::new(&format!("{}/", cloud.url))
        .login("a@example.com", "pw", &ClientInfo::server("box"))
        .await
        .unwrap();
    assert_eq!((ok.token.as_str(), ok.account_id.as_str(), ok.server_id.as_deref()), ("oct_t", "a1", Some("s1")));
    assert_eq!(
        cloud.last("POST /v1/auth/login").body,
        json!({"email": "a@example.com", "password": "pw", "client": {"kind": "server", "label": "box"}})
    );
}

#[tokio::test]
async fn api_errors_keep_their_code() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 401, json!({"error": "invalid_credentials"}));
    let err = CloudClient::new(&cloud.url).login("a@example.com", "pw", &ClientInfo::server("b")).await.unwrap_err();
    assert_eq!(err, CloudError::Api { status: 401, code: "invalid_credentials".into() });
    assert_eq!(err.user_message(), "邮箱或密码错误");
}

#[tokio::test]
async fn revoked_token_is_unauthorized() {
    let cloud = FakeCloud::start().await;
    cloud.respond("GET /v1/account", 401, json!({"error": "unauthorized"}));
    let err = CloudClient::new(&cloud.url).account("oct_x").await.unwrap_err();
    assert_eq!(err, CloudError::Unauthorized);
    assert_eq!(cloud.last("GET /v1/account").auth.as_deref(), Some("Bearer oct_x"));
}

#[tokio::test]
async fn rate_limit_carries_retry_after() {
    let cloud = FakeCloud::start().await;
    cloud.respond_retry_after("POST /v1/auth/email-code", 429, json!({"error": "rate_limited"}), 42);
    let err = CloudClient::new(&cloud.url).request_code("a@example.com", CodePurpose::Register).await.unwrap_err();
    assert_eq!(err, CloudError::RateLimited { retry_after: Some(42) });
}

#[tokio::test]
async fn unreachable_cloud() {
    let err = CloudClient::new("http://127.0.0.1:1").logout("t").await.unwrap_err();
    assert!(matches!(err, CloudError::Unreachable(_)), "{err:?}");
}

#[tokio::test]
async fn notify_sends_minimal_payload() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/notify", 202, json!({"delivered": 2}));
    let n = Notify { session: 7, agent_id: "claude-code".into(), kind: "waiting".into(), tool_name: None };
    assert_eq!(CloudClient::new(&cloud.url).notify("oct_s", &n).await.unwrap(), 2);
    let seen = cloud.last("POST /v1/notify");
    assert_eq!(seen.body, json!({"session": 7, "agent_id": "claude-code", "kind": "waiting"}));
    assert_eq!(seen.auth.as_deref(), Some("Bearer oct_s"));
}

#[tokio::test]
async fn delete_account_uses_delete_with_body() {
    let cloud = FakeCloud::start().await;
    cloud.respond("DELETE /v1/account", 204, json!(null));
    CloudClient::new(&cloud.url).delete_account("oct_t", "pw", "123456").await.unwrap();
    assert_eq!(cloud.last("DELETE /v1/account").body, json!({"password": "pw", "code": "123456"}));
}
```

- [ ] **Step 5: 运行确认失败**

Run: `cargo test -p octoterm-cloud-client`
Expected: 编译失败,`api` 模块不存在。

- [ ] **Step 6: 写 `src/api.rs`**

```rust
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
```

- [ ] **Step 7: 运行测试**

Run: `cargo test -p octoterm-cloud-client`
Expected: credentials 3 个单测 + api.rs 7 个测试 PASS。

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(cloud-client): Cloud HTTP 客户端与 cloud.json 凭据文件

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: cloud-client 账号操作 `ops`

**Files:**
- Create: `crates/cloud-client/src/ops.rs`, `crates/cloud-client/tests/ops.rs`
- Modify: `crates/cloud-client/src/lib.rs`(加 `pub mod ops;`)

**Interfaces:**
- Consumes: Task 1 全部
- Produces(都在 `octoterm_cloud_client::ops`):
  - `OpsError::{Cloud(CloudError), Local(String), NotLoggedIn}`,`user_message(&self) -> String`,`From<CloudError>`
  - `normalize_email(&str) -> String`
  - `current(path) -> Result<Credentials, OpsError>`
  - `async register(path, url, email, code, password, label) -> Result<Credentials, OpsError>`
  - `async login(path, url, email, password, label) -> Result<Credentials, OpsError>`
  - `async reset_password(path, url, email, code, new_password) -> Result<(), OpsError>`(同邮箱的本地登录态一并删除)
  - `async logout(path) -> Result<Option<CloudError>, OpsError>`(`Some(e)` = 只在本机退出,未能通知 Cloud)
  - `async request_delete_code(path) -> Result<String, OpsError>`(返回邮箱)
  - `async delete_account(path, password, code) -> Result<(), OpsError>`

- [ ] **Step 1: 写失败测试 `tests/ops.rs`**

```rust
mod common;

use common::FakeCloud;
use octoterm_cloud_client::credentials;
use octoterm_cloud_client::ops::{self, OpsError};
use serde_json::json;

fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("cloud.json")
}

#[tokio::test]
async fn register_saves_credentials_with_normalized_email() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/register", 201, json!({"token": "oct_t", "account_id": "a", "server_id": "s"}));
    let dir = tempfile::tempdir().unwrap();
    let c = ops::register(&path(&dir), &format!("{}/", cloud.url), " A@Example.com ", "123456", "pw-pw-pw", "box")
        .await
        .unwrap();
    assert_eq!(c.email, "a@example.com");
    assert_eq!(c.url, cloud.url);
    assert_eq!(credentials::load(&path(&dir)).unwrap(), Some(c));
    assert_eq!(cloud.last("POST /v1/auth/register").body["client"], json!({"kind": "server", "label": "box"}));
}

#[tokio::test]
async fn failed_login_writes_nothing() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 401, json!({"error": "invalid_credentials"}));
    let dir = tempfile::tempdir().unwrap();
    let err = ops::login(&path(&dir), &cloud.url, "a@example.com", "nope", "box").await.unwrap_err();
    assert_eq!(err.user_message(), "邮箱或密码错误");
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

async fn logged_in(cloud: &FakeCloud, dir: &tempfile::TempDir) {
    cloud.respond("POST /v1/auth/login", 200, json!({"token": "oct_t", "account_id": "a", "server_id": "s"}));
    ops::login(&path(dir), &cloud.url, "a@example.com", "pw-pw-pw", "box").await.unwrap();
}

#[tokio::test]
async fn logout_removes_file_even_when_cloud_is_down() {
    let dir = tempfile::tempdir().unwrap();
    credentials::save(
        &path(&dir),
        &credentials::Credentials { url: "http://127.0.0.1:1".into(), email: "a@example.com".into(), token: "t".into(), server_id: None },
    )
    .unwrap();
    let r = ops::logout(&path(&dir)).await.unwrap();
    assert!(r.is_some(), "Cloud 不可达应当如实返回");
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
    assert!(matches!(ops::logout(&path(&dir)).await, Err(OpsError::NotLoggedIn)));
}

#[tokio::test]
async fn logout_clean_when_cloud_accepts_or_already_revoked() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/auth/logout", 401, json!({"error": "unauthorized"}));
    assert_eq!(ops::logout(&path(&dir)).await.unwrap(), None);
    assert_eq!(cloud.last("POST /v1/auth/logout").auth.as_deref(), Some("Bearer oct_t"));
}

#[tokio::test]
async fn reset_password_drops_matching_local_login() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/auth/password-reset", 204, json!(null));
    ops::reset_password(&path(&dir), &cloud.url, "other@example.com", "123456", "new-pass-1").await.unwrap();
    assert!(credentials::load(&path(&dir)).unwrap().is_some(), "别的邮箱不该影响本机登录态");
    ops::reset_password(&path(&dir), &cloud.url, "A@example.com", "123456", "new-pass-1").await.unwrap();
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn delete_account_flow() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/account/delete-code", 202, json!(null));
    assert_eq!(ops::request_delete_code(&path(&dir)).await.unwrap(), "a@example.com");

    cloud.respond("DELETE /v1/account", 400, json!({"error": "wrong_password"}));
    let err = ops::delete_account(&path(&dir), "bad", "123456").await.unwrap_err();
    assert_eq!(err.user_message(), "密码错误");
    assert!(credentials::load(&path(&dir)).unwrap().is_some());

    cloud.respond("DELETE /v1/account", 204, json!(null));
    ops::delete_account(&path(&dir), "pw-pw-pw", "123456").await.unwrap();
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p octoterm-cloud-client --test ops`
Expected: 编译失败,`ops` 模块不存在。

- [ ] **Step 3: 写 `src/ops.rs`,在 `lib.rs` 加 `pub mod ops;`**

```rust
//! 「调 Cloud + 改本地 cloud.json」的组合操作。CLI 与 desktop 共用,保证两边行为一致。

use std::path::Path;

use crate::api::{AuthOk, ClientInfo, CloudClient, CloudError};
use crate::credentials::{self, Credentials};

#[derive(Debug)]
pub enum OpsError {
    Cloud(CloudError),
    Local(String),
    NotLoggedIn,
}

impl OpsError {
    pub fn user_message(&self) -> String {
        match self {
            OpsError::Cloud(e) => e.user_message(),
            OpsError::Local(m) => m.clone(),
            OpsError::NotLoggedIn => "当前未登录 Cloud".into(),
        }
    }
}

impl std::fmt::Display for OpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl From<CloudError> for OpsError {
    fn from(e: CloudError) -> Self {
        OpsError::Cloud(e)
    }
}

fn io(e: std::io::Error) -> OpsError {
    OpsError::Local(format!("读写 cloud.json 失败:{e}"))
}

pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn current(path: &Path) -> Result<Credentials, OpsError> {
    credentials::load(path).map_err(io)?.ok_or(OpsError::NotLoggedIn)
}

fn persist(path: &Path, url: &str, email: String, ok: AuthOk) -> Result<Credentials, OpsError> {
    let creds = Credentials {
        url: url.trim().trim_end_matches('/').to_string(),
        email,
        token: ok.token,
        server_id: ok.server_id,
    };
    credentials::save(path, &creds).map_err(io)?;
    Ok(creds)
}

pub async fn register(
    path: &Path,
    url: &str,
    email: &str,
    code: &str,
    password: &str,
    label: &str,
) -> Result<Credentials, OpsError> {
    let email = normalize_email(email);
    let ok = CloudClient::new(url).register(&email, code.trim(), password, &ClientInfo::server(label)).await?;
    persist(path, url, email, ok)
}

pub async fn login(path: &Path, url: &str, email: &str, password: &str, label: &str) -> Result<Credentials, OpsError> {
    let email = normalize_email(email);
    let ok = CloudClient::new(url).login(&email, password, &ClientInfo::server(label)).await?;
    persist(path, url, email, ok)
}

/// Cloud 会撤销该账号的全部凭据;本机若登录的正是这个邮箱,本地登录态一并清掉。
pub async fn reset_password(path: &Path, url: &str, email: &str, code: &str, new_password: &str) -> Result<(), OpsError> {
    let email = normalize_email(email);
    CloudClient::new(url).reset_password(&email, code.trim(), new_password).await?;
    if let Ok(Some(c)) = credentials::load(path)
        && c.email == email
    {
        credentials::delete(path).map_err(io)?;
    }
    Ok(())
}

/// 本地登录态一定会被删掉。返回 `Some(e)` 表示没能通知 Cloud(凭据在 Cloud 上还有效)。
pub async fn logout(path: &Path) -> Result<Option<CloudError>, OpsError> {
    let c = current(path)?;
    let r = CloudClient::new(&c.url).logout(&c.token).await;
    credentials::delete(path).map_err(io)?;
    Ok(match r {
        Ok(()) | Err(CloudError::Unauthorized) => None,
        Err(e) => Some(e),
    })
}

pub async fn request_delete_code(path: &Path) -> Result<String, OpsError> {
    let c = current(path)?;
    CloudClient::new(&c.url).request_delete_code(&c.token).await?;
    Ok(c.email)
}

pub async fn delete_account(path: &Path, password: &str, code: &str) -> Result<(), OpsError> {
    let c = current(path)?;
    CloudClient::new(&c.url).delete_account(&c.token, password, code.trim()).await?;
    credentials::delete(path).map_err(io)?;
    Ok(())
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p octoterm-cloud-client`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(cloud-client): 账号组合操作(调用 + 落盘)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: server —— `[cloud]` 配置与推送通知器

**Files:**
- Create: `crates/server/src/cloud/mod.rs`, `crates/server/src/cloud/notifier.rs`, `crates/server/tests/cloud_notifier.rs`
- Modify: `crates/server/Cargo.toml`, `crates/server/src/lib.rs`, `crates/server/src/config.rs`, `crates/server/src/main.rs`

**Interfaces:**
- Consumes: `octoterm_cloud_client::{CloudClient, CloudError, Notify, credentials}`;`SessionManager::events() -> broadcast::Receiver<ServerMsg>`;`AgentSessionStore::list_pending() -> Vec<PendingRequest>`(`PendingRequest { id, tool_name: Option<String>, .. }`);`octoterm_protocol::{AgentState, ServerMsg}`
- Produces:
  - `config::CloudConfig { url: Option<String> }`,`Config.cloud: CloudConfig`
  - `cloud::notifier::classify(prev: Option<&(AgentState, Option<String>)>, state: AgentState, pending: &Option<String>) -> Option<&'static str>`
  - `cloud::notifier::spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, cred_path: Option<PathBuf>)`(必须在 tokio runtime 上下文里调用)

- [ ] **Step 1: 依赖与配置**

`crates/server/Cargo.toml` 的 `[dependencies]` 加:

```toml
octoterm-cloud-client = { path = "../cloud-client" }
# `cloud register/login` 在终端里读密码时不回显
rpassword = "7"
```

`crates/server/src/config.rs`:在 `pub struct Config` 前加

```rust
/// `[cloud]` —— 可选的 Octoterm Cloud(账号 + 手机推送中继)。
/// `url` 只是登录时的默认地址;真正的登录态在 `cloud.json`(见 `octoterm_cloud_client::credentials`)。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CloudConfig {
    #[serde(default)]
    pub url: Option<String>,
}
```

`Config` 末尾加字段:

```rust
    #[serde(default)]
    pub cloud: CloudConfig,
```

在 `config.rs` 现有的 `#[cfg(test)] mod tests` 里追加:

```rust
    #[test]
    fn cloud_section_is_optional() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.cloud.url, None);
        let c: Config = toml::from_str("[cloud]\nurl = \"https://cloud.example.com\"\n").unwrap();
        assert_eq!(c.cloud.url.as_deref(), Some("https://cloud.example.com"));
    }
```

(若 `config.rs` 没有 tests 模块,就新建 `#[cfg(test)] mod tests { use super::*; ... }`。)

Run: `cargo test -p octoterm-server --lib config`
Expected: PASS。

- [ ] **Step 2: 写失败测试 `crates/server/tests/cloud_notifier.rs`**

```rust
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_protocol::{AgentState, ServerMsg};
use octoterm_server::agent::store::AgentSessionStore;
use octoterm_server::cloud::notifier;
use octoterm_server::session::manager::SessionManager;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Value>>>;

/// 只实现 /v1/notify 的假 Cloud。`status` 决定返回 202 还是 401。
async fn fake_cloud(status: u16) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let app = Router::new()
        .route(
            "/v1/notify",
            post(move |State(seen): State<Seen>, axum::Json(body): axum::Json<Value>| async move {
                seen.lock().unwrap().push(body);
                if status == 401 {
                    (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "unauthorized"})))
                } else {
                    (StatusCode::ACCEPTED, axum::Json(json!({"delivered": 1})))
                }
            }),
        )
        .with_state(seen.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

fn event(state: AgentState, pending: Option<&str>) -> ServerMsg {
    ServerMsg::AgentEvent {
        agent_id: "claude-code".into(),
        agent_session_id: "s-1".into(),
        session: Some(7),
        state,
        pending: pending.map(String::from),
        detail: Some("secret detail".into()),
    }
}

struct Rig {
    manager: Arc<SessionManager>,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new(url: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        if let Some(url) = url {
            credentials::save(
                &path,
                &Credentials { url: url.into(), email: "a@example.com".into(), token: "oct_s".into(), server_id: Some("srv".into()) },
            )
            .unwrap();
        }
        let manager = SessionManager::new(1 << 20, Default::default());
        notifier::spawn(manager.clone(), Arc::new(AgentSessionStore::new()), Some(path));
        Self { manager, dir }
    }

    fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("cloud.json")
    }
}

async fn wait_for(seen: &Seen, n: usize) {
    for _ in 0..100 {
        if seen.lock().unwrap().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("等了 2 秒仍只收到 {} 条", seen.lock().unwrap().len());
}

#[tokio::test]
async fn entering_waiting_notifies_once() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Working, None));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Waiting, None));
    wait_for(&seen, 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec![json!({"session": 7, "agent_id": "claude-code", "kind": "waiting"})]);
}

#[tokio::test]
async fn new_pending_request_notifies_as_permission() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Waiting, Some("p1")));
    wait_for(&seen, 2).await;
    assert_eq!(seen.lock().unwrap()[1]["kind"], "permission");
}

#[tokio::test]
async fn not_logged_in_sends_nothing() {
    let (_url, seen) = fake_cloud(202).await;
    let rig = Rig::new(None);
    rig.manager.publish(event(AgentState::Waiting, None));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn revoked_credentials_are_deleted() {
    let (url, seen) = fake_cloud(401).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    wait_for(&seen, 1).await;
    for _ in 0..50 {
        if !rig.path().exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("401 之后 cloud.json 仍然存在");
}
```

`crates/server/Cargo.toml` 的 `[dev-dependencies]` 已有 `tempfile`、`tokio`;如缺 `octoterm-protocol` 不需要加(它是普通依赖)。

- [ ] **Step 3: 运行确认失败**

Run: `cargo test -p octoterm-server --test cloud_notifier`
Expected: 编译失败,`octoterm_server::cloud` 不存在。

- [ ] **Step 4: 写 `src/cloud/mod.rs` 与 `src/cloud/notifier.rs`,`lib.rs` 加 `pub mod cloud;`**

`crates/server/src/cloud/mod.rs`:

```rust
//! 可选的 Octoterm Cloud 集成:账号命令行(`cli`)与 agent 推送通知器(`notifier`)。
//! 没登录时两者都是空操作。

pub mod notifier;
```

`crates/server/src/cloud/notifier.rs`:

```rust
//! 把「agent 在等你」转成 Cloud 推送。
//!
//! 订阅的是和 WS 推送同一条 `AgentEvent` 广播,所以不需要改 agent 链路的任何一处。
//! 推送是尽力而为:Cloud 不可达只记 debug 日志,绝不反压到 hook 或 WS。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_cloud_client::{CloudClient, CloudError, Notify};
use octoterm_protocol::{AgentState, ServerMsg};
use tokio::sync::broadcast::error::RecvError;

use crate::agent::store::AgentSessionStore;
use crate::session::manager::SessionManager;

/// 这条事件要不要推,推哪一种。
///
/// 只在**迁移**时推:刚进入 waiting,或 waiting 中冒出一个新的 pending。
/// 同一个 waiting 状态的重复事件(hook 会反复上报)不再打扰用户。
pub fn classify(prev: Option<&(AgentState, Option<String>)>, state: AgentState, pending: &Option<String>) -> Option<&'static str> {
    if state != AgentState::Waiting {
        return None;
    }
    let was_waiting = matches!(prev, Some((AgentState::Waiting, _)));
    let new_pending = pending.is_some() && prev.map(|(_, p)| p) != Some(pending);
    (!was_waiting || new_pending).then_some(if pending.is_some() { "permission" } else { "waiting" })
}

/// 按 mtime 缓存的 cloud.json:desktop 登录/退出后不用重启 server 就能生效。
struct CredCache {
    path: PathBuf,
    cached: Option<(SystemTime, Credentials)>,
}

impl CredCache {
    fn get(&mut self) -> Option<Credentials> {
        let Ok(mtime) = std::fs::metadata(&self.path).and_then(|m| m.modified()) else {
            self.cached = None;
            return None;
        };
        if let Some((t, c)) = &self.cached
            && *t == mtime
        {
            return Some(c.clone());
        }
        match credentials::load(&self.path) {
            Ok(Some(c)) => {
                self.cached = Some((mtime, c.clone()));
                Some(c)
            }
            Ok(None) => {
                self.cached = None;
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "cloud.json 读取失败,本次不推送");
                None
            }
        }
    }
}

pub fn spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, cred_path: Option<PathBuf>) {
    let Some(path) = cred_path else { return };
    let mut rx = manager.events();
    tokio::spawn(async move {
        let mut cache = CredCache { path: path.clone(), cached: None };
        let mut last: HashMap<(String, String), (AgentState, Option<String>)> = HashMap::new();
        loop {
            let msg = match rx.recv().await {
                Ok(m) => m,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            let ServerMsg::AgentEvent { agent_id, agent_session_id, session, state, pending, .. } = msg else {
                continue;
            };
            let key = (agent_id.clone(), agent_session_id);
            let kind = classify(last.get(&key), state, &pending);
            if state == AgentState::Done {
                last.remove(&key);
            } else {
                last.insert(key, (state, pending.clone()));
            }
            let (Some(kind), Some(session)) = (kind, session) else { continue };
            let Some(creds) = cache.get() else { continue };
            let tool_name = pending
                .as_ref()
                .and_then(|id| store.list_pending().into_iter().find(|p| &p.id == id))
                .and_then(|p| p.tool_name);
            let note = Notify { session, agent_id, kind: kind.into(), tool_name };
            tokio::spawn(send(path.clone(), creds, note));
        }
    });
}

async fn send(path: PathBuf, creds: Credentials, note: Notify) {
    match CloudClient::new(&creds.url).notify(&creds.token, &note).await {
        Ok(_) => {}
        Err(CloudError::Unauthorized) => {
            // 只删「还是这把 token」的文件:期间用户可能已经重新登录了。
            if let Ok(Some(cur)) = credentials::load(&path)
                && cur.token == creds.token
            {
                let _ = credentials::delete(&path);
                tracing::warn!("Cloud 凭据已失效(在别处退出、删号或重置了密码),本机已退出 Cloud 登录");
            }
        }
        Err(e) => tracing::debug!(error = %e, "Cloud 推送失败"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentState::*;

    #[test]
    fn classify_transitions() {
        assert_eq!(classify(None, Working, &None), None);
        assert_eq!(classify(None, Waiting, &None), Some("waiting"));
        assert_eq!(classify(Some(&(Working, None)), Waiting, &None), Some("waiting"));
        assert_eq!(classify(Some(&(Waiting, None)), Waiting, &None), None);
        assert_eq!(classify(Some(&(Waiting, None)), Waiting, &Some("p1".into())), Some("permission"));
        assert_eq!(classify(Some(&(Waiting, Some("p1".into()))), Waiting, &Some("p1".into())), None);
        assert_eq!(classify(Some(&(Waiting, Some("p1".into()))), Waiting, &Some("p2".into())), Some("permission"));
        // 回答完、pending 被摘掉但仍在 waiting:不是新的打扰
        assert_eq!(classify(Some(&(Waiting, Some("p1".into()))), Waiting, &None), None);
        assert_eq!(classify(Some(&(Waiting, None)), Done, &None), None);
    }
}
```

- [ ] **Step 5: 在 server `main.rs` 启动通知器**

`main.rs` 中 `let state = AppState { ... };` 之后、`start_index` 之前插入:

```rust
    octoterm_server::cloud::notifier::spawn(
        state.manager.clone(),
        state.agent_sessions.clone(),
        octoterm_cloud_client::credentials::default_path(),
    );
```

- [ ] **Step 6: 运行测试**

Run: `cargo test -p octoterm-server --test cloud_notifier && cargo test -p octoterm-server --lib cloud`
Expected: 4 个集成测试 + `classify_transitions` PASS。

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(server): [cloud] 配置与 agent 等待推送通知器

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: server CLI —— `octoterm-server cloud ...`

**Files:**
- Create: `crates/server/src/cloud/cli.rs`
- Modify: `crates/server/src/cloud/mod.rs`(加 `pub mod cli;`), `crates/server/src/main.rs`

**Interfaces:**
- Consumes: `octoterm_cloud_client::{ops, CodePurpose, CloudClient, CloudError, credentials, default_label}`、`config::Config::load`
- Produces: `cloud::cli::{CloudCmd, run(cmd: CloudCmd, config_path: Option<PathBuf>) -> i32}`;`CloudCmd::{Register, Login, Logout, Status, ResetPassword, DeleteAccount}`

- [ ] **Step 1: 写 `src/cloud/cli.rs`**

```rust
//! `octoterm-server cloud <子命令>`:在没有 GUI 的机器上登录 Cloud。
//! 逻辑都在 `octoterm_cloud_client::ops`,这里只管问答与输出。

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use octoterm_cloud_client::ops::{self, OpsError};
use octoterm_cloud_client::{CloudClient, CloudError, CodePurpose, credentials, default_label};

#[derive(clap::Subcommand, Debug, Clone, Copy)]
pub enum CloudCmd {
    /// 注册 Cloud 账号并让本机登录
    Register,
    /// 登录已有账号
    Login,
    /// 退出登录(撤销本机凭据)
    Logout,
    /// 查看登录状态
    Status,
    /// 通过邮件验证码重置密码(会让所有设备退出登录)
    ResetPassword,
    /// 永久删除 Cloud 账号及其全部数据
    DeleteAccount,
}

pub fn run(cmd: CloudCmd, config_path: Option<PathBuf>) -> i32 {
    let default_url = match crate::config::Config::load(config_path) {
        Ok(c) => c.cloud.url,
        Err(e) => {
            eprintln!("读取配置失败:{e:#}");
            return 1;
        }
    };
    let Some(path) = credentials::default_path() else {
        eprintln!("无法确定配置目录");
        return 1;
    };
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("无法创建 tokio runtime:{e}");
            return 1;
        }
    };
    match rt.block_on(dispatch(cmd, &path, default_url)) {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("{msg}");
            1
        }
    }
}

fn ask(label: &str) -> Result<String, String> {
    print!("{label}");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim().to_string())
}

/// 终端里不回显;stdin 被重定向时(脚本、冒烟测试)按普通行读。
fn ask_secret(label: &str) -> Result<String, String> {
    if io::stdin().is_terminal() {
        rpassword::prompt_password(label).map_err(|e| e.to_string())
    } else {
        ask(label)
    }
}

fn ask_url(default_url: Option<String>) -> Result<String, String> {
    if let Some(u) = default_url.filter(|u| !u.trim().is_empty()) {
        return Ok(u);
    }
    let u = ask("Cloud 地址(如 https://cloud.example.com):")?;
    if u.starts_with("http://") || u.starts_with("https://") { Ok(u) } else { Err("Cloud 地址必须以 http:// 或 https:// 开头".into()) }
}

fn ask_new_password() -> Result<String, String> {
    let a = ask_secret("密码(8–128 位):")?;
    let b = ask_secret("再输一次:")?;
    if a != b {
        return Err("两次输入的密码不一致".into());
    }
    Ok(a)
}

fn msg(e: OpsError) -> String {
    e.user_message()
}

async fn dispatch(cmd: CloudCmd, path: &Path, default_url: Option<String>) -> Result<(), String> {
    match cmd {
        CloudCmd::Register => {
            if let Ok(c) = ops::current(path) {
                return Err(format!("本机已登录为 {};请先 `octoterm-server cloud logout`", c.email));
            }
            let url = ask_url(default_url)?;
            let email = ops::normalize_email(&ask("邮箱:")?);
            CloudClient::new(&url)
                .request_code(&email, CodePurpose::Register)
                .await
                .map_err(|e| e.user_message())?;
            println!("验证码已发送到 {email}(10 分钟内有效)");
            let code = ask("验证码:")?;
            let password = ask_new_password()?;
            let c = ops::register(path, &url, &email, &code, &password, &default_label()).await.map_err(msg)?;
            println!("注册成功,本机已登录为 {}", c.email);
        }
        CloudCmd::Login => {
            if let Ok(c) = ops::current(path) {
                return Err(format!("本机已登录为 {};请先 `octoterm-server cloud logout`", c.email));
            }
            let url = ask_url(default_url)?;
            let email = ask("邮箱:")?;
            let password = ask_secret("密码:")?;
            let c = ops::login(path, &url, &email, &password, &default_label()).await.map_err(msg)?;
            println!("已登录为 {}", c.email);
        }
        CloudCmd::Logout => match ops::logout(path).await {
            Ok(None) => println!("已退出登录"),
            Ok(Some(e)) => println!("已在本机退出登录(未能通知 Cloud:{})", e.user_message()),
            Err(OpsError::NotLoggedIn) => println!("当前未登录"),
            Err(e) => return Err(e.user_message()),
        },
        CloudCmd::Status => {
            let c = match ops::current(path) {
                Ok(c) => c,
                Err(OpsError::NotLoggedIn) => {
                    println!("未登录");
                    return Ok(());
                }
                Err(e) => return Err(e.user_message()),
            };
            println!("Cloud:{}\n账号:{}\nserver_id:{}", c.url, c.email, c.server_id.as_deref().unwrap_or("-"));
            match CloudClient::new(&c.url).account(&c.token).await {
                Ok(info) => println!("状态:有效({} 台 server,{} 台设备)", info.servers.len(), info.devices.len()),
                Err(CloudError::Unauthorized) => {
                    let _ = credentials::delete(path);
                    println!("状态:凭据已失效,已在本机退出登录");
                }
                Err(e) => println!("状态:{}", e.user_message()),
            }
        }
        CloudCmd::ResetPassword => {
            let url = ask_url(default_url.or_else(|| ops::current(path).ok().map(|c| c.url)))?;
            let email = ops::normalize_email(&ask("邮箱:")?);
            CloudClient::new(&url)
                .request_code(&email, CodePurpose::Reset)
                .await
                .map_err(|e| e.user_message())?;
            println!("如果该邮箱已注册,验证码已发送(10 分钟内有效)");
            let code = ask("验证码:")?;
            let password = ask_new_password()?;
            ops::reset_password(path, &url, &email, &code, &password).await.map_err(msg)?;
            println!("密码已重置;所有设备都需要重新登录");
        }
        CloudCmd::DeleteAccount => {
            let c = ops::current(path).map_err(msg)?;
            println!("将永久删除 Cloud 账号 {} 及其全部数据(所有 server、设备、推送登记),无法恢复。", c.email);
            if ops::normalize_email(&ask("输入你的邮箱以确认:")?) != c.email {
                return Err("邮箱不匹配,已取消".into());
            }
            let email = ops::request_delete_code(path).await.map_err(msg)?;
            println!("确认码已发送到 {email}");
            let password = ask_secret("密码:")?;
            let code = ask("确认码:")?;
            ops::delete_account(path, &password, &code).await.map_err(msg)?;
            println!("账号已删除");
        }
    }
    Ok(())
}
```

`crates/server/src/cloud/mod.rs` 加 `pub mod cli;`。

- [ ] **Step 2: 接入 `main.rs`**

`enum Cmd` 末尾加:

```rust
    /// Octoterm Cloud 账号(可选,用于手机推送通知)
    Cloud {
        #[command(subcommand)]
        cmd: octoterm_server::cloud::cli::CloudCmd,
    },
```

`main()` 里 `PurgeHooks` 分支之后加:

```rust
    if let Some(Cmd::Cloud { cmd }) = Args::parse().cmd {
        std::process::exit(octoterm_server::cloud::cli::run(cmd, Args::parse().config));
    }
```

- [ ] **Step 3: 构建并检查帮助输出**

Run: `cargo build -p octoterm-server && ./target/debug/octoterm-server cloud --help && OCTOTERM_CLOUD_FILE=/tmp/octoterm-none.json ./target/debug/octoterm-server cloud status`
Expected: 列出 6 个子命令;`status` 输出 `未登录`,退出码 0。

- [ ] **Step 4: 全量测试**

Run: `cargo test -p octoterm-server`
Expected: 全部 PASS(含已有测试)。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(server): octoterm-server cloud 账号子命令

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: desktop —— 通知器与「Cloud 账号」设置区

**Files:**
- Create: `crates/desktop/src/settings/cloud.rs`, `crates/desktop/src/cloud_ops.rs`
- Modify: `crates/desktop/Cargo.toml`, `crates/desktop/src/lib.rs`, `crates/desktop/src/main.rs`, `crates/desktop/src/supervisor.rs`, `crates/desktop/src/settings/mod.rs`, `crates/desktop/src/settings/ui.rs`, `crates/desktop/src/app.rs`

**Interfaces:**
- Consumes: `octoterm_cloud_client::{ops, CloudClient, CodePurpose, credentials, default_label}`、`notifier::spawn`、`settings::state::Message`
- Produces:
  - `settings::cloud::{CloudForm { url, email, password, code, confirm_delete }, CloudStatus::{LoggedOut, LoggedIn { email, url }}, CloudAction::{SendRegisterCode, Register, Login, SendResetCode, ResetPassword, Logout, SendDeleteCode, DeleteAccount}, CloudAction::keeps_secrets(self) -> bool, CloudJob, prepare(&CloudForm, &CloudStatus, CloudAction) -> Result<CloudJob, String>, status_from(Option<Credentials>) -> CloudStatus}`
  - `cloud_ops::run(job: CloudJob, path: PathBuf, label: String) -> Result<String, String>`(async)
  - `Supervisor::agent_sessions(&self) -> &Arc<AgentSessionStore>`
  - `ui::Outcome::Cloud(CloudAction)`、`UserEvent::CloudDone(CloudAction, Result<String, String>)`

- [ ] **Step 1: 依赖、访问器、启动通知器**

`crates/desktop/Cargo.toml` `[dependencies]` 加 `octoterm-cloud-client = { path = "../cloud-client" }`。

`supervisor.rs` 的 `impl Supervisor` 里、`manager()` 之后加:

```rust
    pub fn agent_sessions(&self) -> &Arc<octoterm_server::agent::store::AgentSessionStore> {
        &self.agent_sessions
    }
```

`main.rs` 在 `let mut sup = Supervisor::new(...)` 之后加:

```rust
    // Cloud 推送通知器跟 SessionManager 同寿命,不随 HTTP 层重建;没登录时它什么也不发。
    {
        let _rt = rt.enter();
        octoterm_server::cloud::notifier::spawn(
            sup.manager().clone(),
            sup.agent_sessions().clone(),
            octoterm_cloud_client::credentials::default_path(),
        );
    }
```

Run: `cargo build -p octoterm-desktop`
Expected: 编译通过。

- [ ] **Step 2: 写失败测试 + 纯逻辑 `settings/cloud.rs`**

`crates/desktop/src/settings/mod.rs` 加 `pub mod cloud;`。新建 `crates/desktop/src/settings/cloud.rs`:

```rust
//! 「Cloud 账号」区的表单与校验。和 state.rs 一样不认识 egui,全部可单测。

use octoterm_cloud_client::{CodePurpose, Credentials};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudForm {
    pub url: String,
    pub email: String,
    pub password: String,
    pub code: String,
    pub confirm_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudStatus {
    LoggedOut,
    LoggedIn { email: String, url: String },
}

pub fn status_from(creds: Option<Credentials>) -> CloudStatus {
    match creds {
        Some(c) => CloudStatus::LoggedIn { email: c.email, url: c.url },
        None => CloudStatus::LoggedOut,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudAction {
    SendRegisterCode,
    Register,
    Login,
    SendResetCode,
    ResetPassword,
    Logout,
    SendDeleteCode,
    DeleteAccount,
}

impl CloudAction {
    /// 发码只是中间一步,用户接下来还要用刚填的密码 —— 这时不清空表单里的密码。
    pub fn keeps_secrets(self) -> bool {
        matches!(self, CloudAction::SendRegisterCode | CloudAction::SendResetCode | CloudAction::SendDeleteCode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudJob {
    RequestCode { url: String, email: String, purpose: CodePurpose },
    Register { url: String, email: String, code: String, password: String },
    Login { url: String, email: String, password: String },
    ResetPassword { url: String, email: String, code: String, password: String },
    Logout,
    RequestDeleteCode,
    DeleteAccount { password: String, code: String },
}

fn url(form: &CloudForm) -> Result<String, String> {
    let u = form.url.trim();
    if u.starts_with("http://") || u.starts_with("https://") {
        Ok(u.to_string())
    } else {
        Err("Cloud 地址必须以 http:// 或 https:// 开头".into())
    }
}

fn email(form: &CloudForm) -> Result<String, String> {
    let e = form.email.trim().to_lowercase();
    if e.contains('@') { Ok(e) } else { Err("请填写邮箱".into()) }
}

fn password(form: &CloudForm) -> Result<String, String> {
    let n = form.password.chars().count();
    if (8..=128).contains(&n) { Ok(form.password.clone()) } else { Err("密码长度需为 8–128 个字符".into()) }
}

fn code(form: &CloudForm) -> Result<String, String> {
    let c = form.code.trim();
    if c.len() == 6 && c.chars().all(|ch| ch.is_ascii_digit()) {
        Ok(c.to_string())
    } else {
        Err("请填写邮件里的 6 位验证码".into())
    }
}

pub fn prepare(form: &CloudForm, status: &CloudStatus, action: CloudAction) -> Result<CloudJob, String> {
    use CloudAction::*;
    let logged_in = matches!(status, CloudStatus::LoggedIn { .. });
    let needs_login = matches!(action, Logout | SendDeleteCode | DeleteAccount);
    if needs_login != logged_in {
        return Err(if logged_in { "已登录;请先退出登录".into() } else { "当前未登录".into() });
    }
    Ok(match action {
        SendRegisterCode => CloudJob::RequestCode { url: url(form)?, email: email(form)?, purpose: CodePurpose::Register },
        SendResetCode => CloudJob::RequestCode { url: url(form)?, email: email(form)?, purpose: CodePurpose::Reset },
        Register => CloudJob::Register { url: url(form)?, email: email(form)?, code: code(form)?, password: password(form)? },
        Login => {
            if form.password.is_empty() {
                return Err("请填写密码".into());
            }
            CloudJob::Login { url: url(form)?, email: email(form)?, password: form.password.clone() }
        }
        ResetPassword => {
            CloudJob::ResetPassword { url: url(form)?, email: email(form)?, code: code(form)?, password: password(form)? }
        }
        Logout => CloudJob::Logout,
        SendDeleteCode => CloudJob::RequestDeleteCode,
        DeleteAccount => {
            if !form.confirm_delete {
                return Err("请先勾选「我明白删除后无法恢复」".into());
            }
            if form.password.is_empty() {
                return Err("请填写密码".into());
            }
            CloudJob::DeleteAccount { password: form.password.clone(), code: code(form)? }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> CloudForm {
        CloudForm {
            url: " https://cloud.example.com ".into(),
            email: " A@Example.com ".into(),
            password: "pw-pw-pw".into(),
            code: " 012345 ".into(),
            confirm_delete: false,
        }
    }

    fn logged_in() -> CloudStatus {
        CloudStatus::LoggedIn { email: "a@example.com".into(), url: "https://cloud.example.com".into() }
    }

    #[test]
    fn register_job_is_normalized() {
        assert_eq!(
            prepare(&form(), &CloudStatus::LoggedOut, CloudAction::Register),
            Ok(CloudJob::Register {
                url: "https://cloud.example.com".into(),
                email: "a@example.com".into(),
                code: "012345".into(),
                password: "pw-pw-pw".into(),
            })
        );
    }

    #[test]
    fn validation_errors() {
        let mut f = form();
        f.url = "cloud.example.com".into();
        assert!(prepare(&f, &CloudStatus::LoggedOut, CloudAction::Login).is_err());
        let mut f = form();
        f.code = "12a456".into();
        assert!(prepare(&f, &CloudStatus::LoggedOut, CloudAction::Register).is_err());
        let mut f = form();
        f.password = "short".into();
        assert!(prepare(&f, &CloudStatus::LoggedOut, CloudAction::ResetPassword).is_err());
    }

    #[test]
    fn actions_require_matching_login_state() {
        assert!(prepare(&form(), &logged_in(), CloudAction::Login).is_err());
        assert!(prepare(&form(), &CloudStatus::LoggedOut, CloudAction::Logout).is_err());
        assert_eq!(prepare(&form(), &logged_in(), CloudAction::Logout), Ok(CloudJob::Logout));
    }

    #[test]
    fn delete_requires_confirmation() {
        assert!(prepare(&form(), &logged_in(), CloudAction::DeleteAccount).is_err());
        let mut f = form();
        f.confirm_delete = true;
        assert_eq!(
            prepare(&f, &logged_in(), CloudAction::DeleteAccount),
            Ok(CloudJob::DeleteAccount { password: "pw-pw-pw".into(), code: "012345".into() })
        );
    }

    #[test]
    fn send_code_keeps_secrets() {
        assert!(CloudAction::SendRegisterCode.keeps_secrets());
        assert!(!CloudAction::Register.keeps_secrets());
    }
}
```

Run: `cargo test -p octoterm-desktop settings::cloud`
Expected: 5 个测试 PASS。

- [ ] **Step 3: 写 `crates/desktop/src/cloud_ops.rs`,`lib.rs` 加 `pub mod cloud_ops;`**

```rust
//! 在 tokio runtime 上执行设置窗口发起的 Cloud 操作,返回给用户看的一句话。

use std::path::PathBuf;

use octoterm_cloud_client::ops::{self, OpsError};
use octoterm_cloud_client::{CloudClient, CodePurpose};

use crate::settings::cloud::CloudJob;

fn msg(e: OpsError) -> String {
    e.user_message()
}

pub async fn run(job: CloudJob, path: PathBuf, label: String) -> Result<String, String> {
    match job {
        CloudJob::RequestCode { url, email, purpose } => {
            CloudClient::new(&url).request_code(&email, purpose).await.map_err(|e| e.user_message())?;
            Ok(match purpose {
                CodePurpose::Register => format!("验证码已发送到 {email},10 分钟内有效"),
                CodePurpose::Reset => "如果该邮箱已注册,验证码已发送(10 分钟内有效)".into(),
            })
        }
        CloudJob::Register { url, email, code, password } => {
            let c = ops::register(&path, &url, &email, &code, &password, &label).await.map_err(msg)?;
            Ok(format!("注册成功,已登录为 {}", c.email))
        }
        CloudJob::Login { url, email, password } => {
            let c = ops::login(&path, &url, &email, &password, &label).await.map_err(msg)?;
            Ok(format!("已登录为 {}", c.email))
        }
        CloudJob::ResetPassword { url, email, code, password } => {
            ops::reset_password(&path, &url, &email, &code, &password).await.map_err(msg)?;
            Ok("密码已重置;所有设备都需要重新登录".into())
        }
        CloudJob::Logout => match ops::logout(&path).await.map_err(msg)? {
            None => Ok("已退出登录".into()),
            Some(e) => Ok(format!("已在本机退出登录(未能通知 Cloud:{})", e.user_message())),
        },
        CloudJob::RequestDeleteCode => {
            let email = ops::request_delete_code(&path).await.map_err(msg)?;
            Ok(format!("确认码已发送到 {email}"))
        }
        CloudJob::DeleteAccount { password, code } => {
            ops::delete_account(&path, &password, &code).await.map_err(msg)?;
            Ok("账号已删除".into())
        }
    }
}
```

- [ ] **Step 4: View 增加 Cloud 状态,ui.rs 画「Cloud 账号」区**

`settings/ui.rs`:

1. 顶部 `use` 改为:

```rust
use crate::settings::cloud::{CloudAction, CloudForm, CloudStatus};
use crate::settings::state::Form;
```

2. `Outcome` 加变体 `Cloud(CloudAction),`。

3. `pub struct View` 末尾加字段:

```rust
    pub cloud: CloudForm,
    pub cloud_status: CloudStatus,
    /// 有 Cloud 请求在路上:按钮置灰,防止连点。
    pub cloud_busy: bool,
    /// 最近一次 Cloud 操作的结果。和 `message`(保存结果)分开,两者互不覆盖。
    pub cloud_message: Option<Message>,
```

4. 在 `draw` 里「启动项」那个 `ScrollArea` 之后、`// 「重新生成」也算改动` 之前插入:

```rust
            ui.add_space(8.0);
            ui.separator();
            if let Some(a) = draw_cloud(ui, view) {
                outcome = Outcome::Cloud(a);
            }
```

5. 文件末尾加:

```rust
/// 「Cloud 账号」区。和 `draw` 一样同一帧可能跑两趟:只改表单文本、只返回意图。
fn draw_cloud(ui: &mut egui::Ui, view: &mut View) -> Option<CloudAction> {
    let mut action = None;
    let status = view.cloud_status.clone();
    ui.horizontal(|ui| {
        ui.label("Cloud 账号");
        ui.small("(可选,用于把 agent 的等待提醒推送到手机)");
        if view.cloud_busy {
            ui.spinner();
        }
    });
    ui.add_enabled_ui(!view.cloud_busy, |ui| match &status {
        CloudStatus::LoggedIn { email, url } => {
            ui.horizontal(|ui| {
                ui.label(format!("已登录:{email}"));
                ui.small(url.as_str());
            });
            if ui.button("退出登录").clicked() {
                action = Some(CloudAction::Logout);
            }
            ui.collapsing("删除账号", |ui| {
                ui.small("永久删除 Cloud 账号及其全部数据,需要密码和发到邮箱的确认码。");
                egui::Grid::new("cloud-delete").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("密码");
                    ui.add(egui::TextEdit::singleline(&mut view.cloud.password).password(true).desired_width(180.0));
                    ui.end_row();
                    ui.label("确认码");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut view.cloud.code).desired_width(80.0));
                        if ui.button("发送确认码").clicked() {
                            action = Some(CloudAction::SendDeleteCode);
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut view.cloud.confirm_delete, "我明白删除后无法恢复");
                if ui.button("永久删除账号").clicked() {
                    action = Some(CloudAction::DeleteAccount);
                }
            });
        }
        CloudStatus::LoggedOut => {
            egui::Grid::new("cloud-login").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Cloud 地址");
                ui.add(egui::TextEdit::singleline(&mut view.cloud.url).desired_width(220.0));
                ui.end_row();
                ui.label("邮箱");
                ui.add(egui::TextEdit::singleline(&mut view.cloud.email).desired_width(220.0));
                ui.end_row();
                ui.label("密码");
                ui.add(egui::TextEdit::singleline(&mut view.cloud.password).password(true).desired_width(220.0));
                ui.end_row();
                ui.label("验证码");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut view.cloud.code).desired_width(80.0));
                    ui.small("注册或重置密码时需要");
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("登录").clicked() {
                    action = Some(CloudAction::Login);
                }
                if ui.button("发送注册验证码").clicked() {
                    action = Some(CloudAction::SendRegisterCode);
                }
                if ui.button("注册").clicked() {
                    action = Some(CloudAction::Register);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("发送重置验证码").clicked() {
                    action = Some(CloudAction::SendResetCode);
                }
                if ui.button("用上面的密码重置").clicked() {
                    action = Some(CloudAction::ResetPassword);
                }
            });
        }
    });
    if let Some(m) = &view.cloud_message {
        match m {
            Message::Ok(t) => ui.colored_label(OK_COLOR, t.as_str()),
            Message::Err(t) => ui.colored_label(ERR_COLOR, t.as_str()),
        };
    }
    action
}
```

- [ ] **Step 5: app.rs 接线**

1. `UserEvent` 加变体:

```rust
    /// 设置窗口发起的 Cloud 操作完成了(在 tokio 线程上跑,结果经 proxy 送回主线程)。
    CloudDone(crate::settings::cloud::CloudAction, Result<String, String>),
```

2. `build_view` 里 `View { ... }` 末尾加:

```rust
            cloud: crate::settings::cloud::CloudForm {
                url: Self::cloud_creds()
                    .map(|c| c.url)
                    .or_else(|| self.startup.config.cloud.url.clone())
                    .unwrap_or_default(),
                ..Default::default()
            },
            cloud_status: crate::settings::cloud::status_from(Self::cloud_creds()),
            cloud_busy: false,
            cloud_message: None,
```

3. `impl App` 里加两个方法:

```rust
    fn cloud_creds() -> Option<octoterm_cloud_client::Credentials> {
        let path = octoterm_cloud_client::credentials::default_path()?;
        octoterm_cloud_client::credentials::load(&path).ok().flatten()
    }

    fn start_cloud(&mut self, action: crate::settings::cloud::CloudAction) {
        use crate::settings::ui::Message;
        let Some(view) = self.view.as_mut() else { return };
        if view.cloud_busy {
            return;
        }
        let job = match crate::settings::cloud::prepare(&view.cloud, &view.cloud_status, action) {
            Ok(job) => job,
            Err(m) => {
                view.cloud_message = Some(Message::Err(m));
                return;
            }
        };
        let Some(path) = octoterm_cloud_client::credentials::default_path() else {
            view.cloud_message = Some(Message::Err("无法确定配置目录".into()));
            return;
        };
        view.cloud_busy = true;
        view.cloud_message = None;
        let proxy = self.proxy.clone();
        self.rt.spawn(async move {
            let result = crate::cloud_ops::run(job, path, octoterm_cloud_client::default_label()).await;
            let _ = proxy.send_event(UserEvent::CloudDone(action, result));
        });
    }
```

4. `RedrawRequested` 里 `match outcome` 加分支:

```rust
                    Outcome::Cloud(action) => self.start_cloud(action),
```

5. `user_event` 的 `match event` 加分支:

```rust
            UserEvent::CloudDone(action, result) => {
                use crate::settings::ui::Message;
                if let Some(view) = self.view.as_mut() {
                    view.cloud_busy = false;
                    view.cloud_status = crate::settings::cloud::status_from(Self::cloud_creds());
                    if result.is_ok() && !action.keeps_secrets() {
                        view.cloud.password.clear();
                        view.cloud.code.clear();
                        view.cloud.confirm_delete = false;
                    }
                    view.cloud_message = Some(match result {
                        Ok(m) => Message::Ok(m),
                        Err(m) => Message::Err(m),
                    });
                }
                if let Some(w) = &self.settings_window {
                    w.request_redraw();
                }
            }
```

- [ ] **Step 6: 构建、测试、Clippy**

Run: `cargo build -p octoterm-desktop && cargo test -p octoterm-desktop && cargo clippy -p octoterm-desktop -p octoterm-server -p octoterm-cloud-client --all-targets -- -D warnings`
Expected: 全部通过。若 `match outcome` / `match event` 报非穷尽,补上对应分支。

- [ ] **Step 7: 人工验证 UI(需要 GUI)**

Run: `cargo run -p octoterm-desktop`,托盘 →「设置…」。
Expected: 设置窗口底部出现「Cloud 账号」区;未填地址点「登录」显示红字「Cloud 地址必须以 http:// 或 https:// 开头」。完整注册/登录在 Task 6 用真实 Cloud 验证。

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(desktop): 设置窗口 Cloud 账号区与推送通知器

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: 端到端冒烟(真实 OctotermCloud)与文档

**Files:**
- Modify: `README.md`, `README-cnzh.md`

- [ ] **Step 1: 起本地 Cloud,用 CLI 走注册之外的全部流程**

注册需要交互式输入中途收到的验证码,这里用 curl 注册,再用 CLI 登录/查看/退出:

```bash
S=$(mktemp -d); CLOUD=~/online/eclosion/OctotermCloud
(cd $CLOUD && DEV_MAIL_LOG=1 LISTEN=127.0.0.1:18090 DATABASE_URL="sqlite://$S/cloud.db?mode=rwc" cargo run -q > $S/cloud.log 2>&1 &)
for i in $(seq 1 90); do curl -fsS http://127.0.0.1:18090/healthz >/dev/null 2>&1 && break; sleep 1; done
curl -s -X POST http://127.0.0.1:18090/v1/auth/email-code -H 'content-type: application/json' \
  -d '{"email":"e2e@example.com","purpose":"register"}'
sleep 1; CODE=$(grep -o '<b>[0-9]\{6\}</b>' $S/cloud.log | tail -1 | tr -dc 0-9)
curl -s -X POST http://127.0.0.1:18090/v1/auth/register -H 'content-type: application/json' \
  -d "{\"email\":\"e2e@example.com\",\"code\":\"$CODE\",\"password\":\"e2e-password\",\"client\":{\"kind\":\"device\",\"label\":\"phone\"}}"; echo
printf '[cloud]\nurl = "http://127.0.0.1:18090"\n' > $S/config.toml
export OCTOTERM_CLOUD_FILE=$S/cloud.json
cd ~/online/eclosion/Octoterm
printf 'e2e@example.com\ne2e-password\n' | ./target/debug/octoterm-server --config $S/config.toml cloud login
./target/debug/octoterm-server --config $S/config.toml cloud status
stat -f '%Lp' $S/cloud.json
./target/debug/octoterm-server --config $S/config.toml cloud logout
./target/debug/octoterm-server --config $S/config.toml cloud status
pkill -f 'target/debug/octoterm-cloud'
```

Expected:
- register 返回带 `"token":"oct_` 的 JSON;
- `cloud login` 输出 `已登录为 e2e@example.com`;
- `cloud status` 输出 `状态:有效(1 台 server,1 台设备)`;
- 权限 `600`;
- `cloud logout` 输出 `已退出登录`,随后 `status` 输出 `未登录`。

- [ ] **Step 2: README 加一节**

`README-cnzh.md` 末尾(或「配置」一节之后)加:

```markdown
## Octoterm Cloud(可选)

登录 Cloud 之后,agent 进入「等你」状态时会通过 Cloud 推送到手机(手机端推送将在后续版本提供)。不登录不影响任何功能。

- Cloud 不转发终端数据,拿不到本机的访问 token;推送只包含会话编号、agent 名称与工具名。
- 登录态保存在配置目录的 `cloud.json`(权限 0600)。

```bash
octoterm-server cloud register        # 注册并登录(邮件验证码)
octoterm-server cloud login
octoterm-server cloud status
octoterm-server cloud logout
octoterm-server cloud reset-password
octoterm-server cloud delete-account  # 永久删除账号及全部数据
```

Cloud 地址可写进 `config.toml`:

```toml
[cloud]
url = "https://cloud.example.com"
```

desktop 用户在「设置…」窗口底部的「Cloud 账号」区操作。
```

`README.md` 加对应英文段落:

```markdown
## Octoterm Cloud (optional)

Signing in to Octoterm Cloud lets agent "waiting for you" events reach your phone as push notifications (mobile push lands in a later release). Everything works without signing in.

- Cloud never relays terminal data and never sees this machine's access token; a push carries only the session number, agent name and tool name.
- The sign-in state lives in `cloud.json` in the config directory (mode 0600).

```bash
octoterm-server cloud register        # sign up and sign in (email code)
octoterm-server cloud login
octoterm-server cloud status
octoterm-server cloud logout
octoterm-server cloud reset-password
octoterm-server cloud delete-account  # permanently delete the account and all its data
```

Set a default Cloud address in `config.toml`:

```toml
[cloud]
url = "https://cloud.example.com"
```

Desktop users use the "Cloud account" section at the bottom of the Settings window.
```

- [ ] **Step 3: 全量验证**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通过。

- [ ] **Step 4: Commit 并合回 dev**

```bash
git add -A && git commit -m "docs: Octoterm Cloud 登录与推送说明

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
git checkout dev && git merge --ff-only feat/cloud-login
```
