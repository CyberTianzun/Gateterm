# 手机推送:server 端与 cloud-ffi Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** server 在握手时回带 `cloud_server_id`、通知器发撤回(`resolved`)与过期(`expires_at`);`cloud-client` 支持以设备身份登录与登记推送 token;新 crate `cloud-ffi` 用 UniFFI 把账号操作导出给 Kotlin(iOS 将来复用)。

**Architecture:** `cloud.json` 的读取提为 `cloud::creds::SharedCreds`(按 (mtime, len) 缓存),握手与通知器共用,挂在 `AppState.cloud`。通知器为每个 agent 会话记录「是否已通知」,离开 waiting 时据此发 `resolved`。`cloud-ffi` 是 `ops` 的薄壳,错误只导出错误码。

**Tech Stack:** Rust 2024、tokio、axum 0.8(测试假 Cloud)、UniFFI 0.29(proc-macro,tokio async)、cargo-ndk。

**Spec:** `docs/superpowers/specs/2026-10-19-mobile-push-design.md` §3、§5(下称「推送 spec」)。

## Global Constraints

- `HelloOk.cloud_server_id`:`#[serde(default, skip_serializing_if = "Option::is_none")]`,值来自 `cloud.json` 的 `server_id`;未登录为 `None`;**不 bump proto**。
- 通知器:进入 waiting → `waiting`;waiting 中新 pending → `permission`(带 `expires_at`);**离开 waiting 且此前对该会话真的发出过通知** → `resolved`(不带 tool_name、expires_at)。`expires_at` 取 PendingRequest 的 `expires_at`,并截断到 `now + 3600`(Cloud 的上限)。
- `/v1/notify` 请求体只有 `{session, agent_id, kind, tool_name?, expires_at?}`;不发 detail、工具入参。
- 吊销规则不变:只有 401 + `unauthorized` 才删本地凭据,且只删仍是同一 token 的文件。
- `ops::register` / `ops::login` 增加 `kind: ClientKind` 参数;server CLI 与 desktop 传 `ClientKind::Server`,手机(FFI)传 `ClientKind::Device`。
- FFI 错误枚举 `CloudFfiError { NotLoggedIn, Unauthorized, RateLimited{retry_after}, Api{code}, Unreachable{message}, Local{message} }`;**不导出中文文案**。
- Android 只构建 `arm64-v8a`。NDK 来自 `ANDROID_NDK_HOME`(本机 `/Users/hiro/frameworks/android-ndk-r30`)。
- 工作区有未跟踪的 `clients/ios/`,**永远不要提交**;只按具体路径 `git add`。
- 提交流程:分支 `feat/push-server-ffi`,每 Task 一次提交,完成后 `--ff-only` 合回 dev,不推送。commit 结尾 `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`。
- 门槛:`cargo test --workspace` 全绿无警告;`cargo clippy --workspace --all-targets -- -D warnings` 干净。

## File Structure

```
crates/cloud-client/src/api.rs        register_push_target、Notify.expires_at、ClientInfo::new
crates/cloud-client/src/ops.rs        register/login 带 kind;register_push_target
crates/cloud-client/tests/{api,ops}.rs
crates/protocol/src/messages.rs       HelloOk.cloud_server_id
crates/server/src/cloud/creds.rs      SharedCreds(新)
crates/server/src/cloud/notifier.rs   用 SharedCreds;decide();resolved/expires_at
crates/server/src/app.rs              AppState.cloud;握手回带
crates/server/src/main.rs、cloud/cli.rs
crates/server/tests/{common/mod.rs, ws_auth.rs, ws_cloud_hello.rs, cloud_notifier.rs}
crates/desktop/src/{supervisor.rs, main.rs, cloud_ops.rs}
crates/cloud-ffi/{Cargo.toml, src/lib.rs, src/bin/uniffi-bindgen.rs, tests/ffi.rs}(新)
scripts/build-android-ffi.sh(新)
```

---

### Task 1: cloud-client —— 设备登录与推送 token 登记

**Files:**
- Modify: `crates/cloud-client/src/api.rs`, `crates/cloud-client/src/ops.rs`, `crates/cloud-client/tests/api.rs`, `crates/cloud-client/tests/ops.rs`, `crates/server/src/cloud/cli.rs`, `crates/desktop/src/cloud_ops.rs`, `crates/server/src/cloud/notifier.rs`(仅给 `Notify` 字面量补字段)

**Interfaces:**
- Produces:
  - `ClientInfo::new(kind: ClientKind, label: &str) -> ClientInfo`
  - `CloudClient::register_push_target(&self, token: &str, platform: &str, push_token: &str) -> Result<(), CloudError>`(`PUT /v1/push-target`)
  - `Notify.expires_at: Option<i64>`(`skip_serializing_if = "Option::is_none"`)
  - `ops::register(path, url, email, code, password, kind: ClientKind, label)`、`ops::login(path, url, email, password, kind: ClientKind, label)`
  - `ops::register_push_target(path: &Path, platform: &str, push_token: &str) -> Result<(), OpsError>`(走 `revoke_on_401`)

- [ ] **Step 1: 建分支**

```bash
cd ~/online/eclosion/Gateterm && git checkout -b feat/push-server-ffi
```

- [ ] **Step 2: 失败测试**

`crates/cloud-client/tests/api.rs` 追加:

```rust
#[tokio::test]
async fn register_push_target_puts_platform_and_token() {
    let cloud = FakeCloud::start().await;
    cloud.respond("PUT /v1/push-target", 204, json!(null));
    CloudClient::new(&cloud.url).register_push_target("oct_d", "fcm", "fcm-token-1").await.unwrap();
    let seen = cloud.last("PUT /v1/push-target");
    assert_eq!(seen.auth.as_deref(), Some("Bearer oct_d"));
    assert_eq!(seen.body, json!({"platform": "fcm", "push_token": "fcm-token-1"}));
}

#[tokio::test]
async fn notify_includes_expires_at_only_when_set() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/notify", 202, json!({"delivered": 1}));
    let n = Notify { session: 1, agent_id: "a".into(), kind: "permission".into(), tool_name: None, expires_at: Some(123) };
    CloudClient::new(&cloud.url).notify("t", &n).await.unwrap();
    assert_eq!(cloud.last("POST /v1/notify").body["expires_at"], 123);
}
```

现有 `notify_sends_minimal_payload` 里的 `Notify { .. }` 字面量补 `expires_at: None`(断言不变:该字段不出现)。

`crates/cloud-client/tests/ops.rs` 追加(并把文件里所有 `ops::register(...)`、`ops::login(...)` 调用补上 `ClientKind::Server` 参数,位置在 label 之前):

```rust
#[tokio::test]
async fn device_login_sends_device_kind() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 200, json!({"token": "oct_d", "account_id": "a"}));
    let dir = tempfile::tempdir().unwrap();
    ops::login(&path(&dir), &cloud.url, "a@example.com", "pw-pw-pw", ClientKind::Device, "Pixel 9").await.unwrap();
    assert_eq!(cloud.last("POST /v1/auth/login").body["client"], json!({"kind": "device", "label": "Pixel 9"}));
}

#[tokio::test]
async fn register_push_target_revokes_on_401() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("PUT /v1/push-target", 204, json!(null));
    ops::register_push_target(&path(&dir), "fcm", "tok").await.unwrap();
    cloud.respond("PUT /v1/push-target", 401, json!({"error": "unauthorized"}));
    assert!(ops::register_push_target(&path(&dir), "fcm", "tok").await.is_err());
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}
```

(`use gateterm_cloud_client::ClientKind;` 加到该文件顶部;`logged_in` 是该文件已有辅助函数,若其内部调用了 `ops::login` 一并补参数。)

Run: `cargo test -p gateterm-cloud-client`
Expected: 编译失败(签名不匹配 / 方法不存在)。

- [ ] **Step 3: 实现**

`api.rs`:
- `Notify` 末尾加:
  ```rust
      /// 待授权请求的过期时刻(unix 秒)。App 用它让通知到点自动消失。
      #[serde(skip_serializing_if = "Option::is_none")]
      pub expires_at: Option<i64>,
  ```
- `impl ClientInfo` 加:
  ```rust
      pub fn new(kind: ClientKind, label: &str) -> Self {
          Self { kind, label: label.to_string() }
      }
  ```
  并把 `server()` 改为 `Self::new(ClientKind::Server, label)`。
- `impl CloudClient` 加:
  ```rust
      /// 登记本设备的推送 token(仅 device 凭据可用)。
      pub async fn register_push_target(&self, token: &str, platform: &str, push_token: &str) -> Result<(), CloudError> {
          let body = json!({"platform": platform, "push_token": push_token});
          self.send(self.http.put(self.url("/v1/push-target")).bearer_auth(token).json(&body)).await.map(drop)
      }
  ```

`ops.rs`:
- `register` 与 `login` 在 `label` 参数前加 `kind: ClientKind`,内部用 `ClientInfo::new(kind, label)` 替换 `ClientInfo::server(label)`;import 加 `ClientKind`。
- 加:
  ```rust
  /// 登记本设备的推送 token。401 unauthorized 时与其它 Bearer 操作一样清掉本地凭据。
  pub async fn register_push_target(path: &Path, platform: &str, push_token: &str) -> Result<(), OpsError> {
      let c = current(path)?;
      let r = CloudClient::new(&c.url).register_push_target(&c.token, platform, push_token).await;
      revoke_on_401(path, &c.token, r)
  }
  ```

调用方:
- `crates/server/src/cloud/cli.rs`:`ops::register(..., &default_label())` 与 `ops::login(...)` 在 label 前补 `ClientKind::Server`(import `gateterm_cloud_client::ClientKind`)。
- `crates/desktop/src/cloud_ops.rs`:同上补 `ClientKind::Server`。
- `crates/server/src/cloud/notifier.rs`:`Notify { .. }` 字面量补 `expires_at: None`(Task 3 再改成真实值)。

- [ ] **Step 4: 运行并提交**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通过。

```bash
git add crates/cloud-client crates/server/src/cloud/cli.rs crates/server/src/cloud/notifier.rs crates/desktop/src/cloud_ops.rs
git commit -m "feat(cloud-client): 设备身份登录与推送 token 登记

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: 握手回带 `cloud_server_id`(SharedCreds)

**Files:**
- Create: `crates/server/src/cloud/creds.rs`, `crates/server/tests/ws_cloud_hello.rs`
- Modify: `crates/protocol/src/messages.rs`, `crates/server/src/cloud/mod.rs`, `crates/server/src/cloud/notifier.rs`, `crates/server/src/app.rs`, `crates/server/src/main.rs`, `crates/server/src/agent/history.rs`(测试里的 `AppState` 字面量), `crates/server/tests/common/mod.rs`, `crates/server/tests/ws_auth.rs`, `crates/server/tests/cloud_notifier.rs`, `crates/desktop/src/supervisor.rs`, `crates/desktop/src/main.rs`

**Interfaces:**
- Consumes: `gateterm_cloud_client::credentials::{self, Credentials}`
- Produces:
  - `ServerMsg::HelloOk { proto, os, cloud_server_id: Option<String> }`
  - `cloud::creds::SharedCreds`(`Clone + Default`):`new(Option<PathBuf>)`、`path() -> Option<&Path>`、`get() -> Option<Credentials>`、`server_id() -> Option<String>`
  - `AppState.cloud: SharedCreds`
  - `notifier::spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, creds: SharedCreds)`(未登录/无路径时仍订阅,但每次都 `get()` 为 None 不推)
  - `Supervisor::cloud_creds(&self) -> &SharedCreds`
  - 测试辅助 `common::spawn_server_with_cloud(cloud: SharedCreds, token: &str) -> SocketAddr`

- [ ] **Step 1: 协议失败测试(`crates/protocol/src/messages.rs` tests 模块)**

把 `hello_ok_accepts_old_servers_and_reports_os` 中的两个字面量补 `cloud_server_id: None`,并追加:

```rust
    #[test]
    fn hello_ok_cloud_server_id_is_optional() {
        let none = ServerMsg::HelloOk { proto: 1, os: "macos".into(), cloud_server_id: None };
        assert!(serde_json::to_value(&none).unwrap().get("cloud_server_id").is_none());
        let some = ServerMsg::HelloOk { proto: 1, os: "macos".into(), cloud_server_id: Some("srv".into()) };
        let json = serde_json::to_value(&some).unwrap();
        assert_eq!(json["cloud_server_id"], "srv");
        assert_eq!(serde_json::from_value::<ServerMsg>(json).unwrap(), some);
    }
```

- [ ] **Step 2: 实现协议字段**

`HelloOk` 变体改为:

```rust
    HelloOk {
        proto: u32,
        #[serde(default)]
        os: String,
        /// 本机登录 Gateterm Cloud 后的 server_id。手机收到推送时用它找回本地保存的连接。
        /// 新增可选字段,旧客户端忽略,不 bump proto。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cloud_server_id: Option<String>,
    },
```

Run: `cargo test -p gateterm-protocol`
Expected: PASS。

- [ ] **Step 3: `crates/server/src/cloud/creds.rs`**

把 `notifier.rs` 里的 `CredCache` 及其单测 `cred_cache_busts_on_length_change_even_when_mtime_is_unchanged` 搬到这里,改造成共享版:

```rust
//! `cloud.json` 的共享只读视图。握手(回带 cloud_server_id)与通知器共用,
//! 按 (mtime, 文件长度) 缓存:desktop 登录/退出后不用重启 server 就能生效;
//! 单看 mtime 在粗粒度文件系统上不够 —— 同一个 tick 里内容换了,mtime 可能没变。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use gateterm_cloud_client::credentials::{self, Credentials};

type Cached = Option<((SystemTime, u64), Credentials)>;

#[derive(Clone, Default)]
pub struct SharedCreds {
    inner: Option<Arc<Inner>>,
}

struct Inner {
    path: PathBuf,
    cached: Mutex<Cached>,
}

impl SharedCreds {
    /// `None` 表示没有可用的配置目录:永远视为未登录。
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { inner: path.map(|path| Arc::new(Inner { path, cached: Mutex::new(None) })) }
    }

    pub fn path(&self) -> Option<&Path> {
        self.inner.as_ref().map(|i| i.path.as_path())
    }

    pub fn get(&self) -> Option<Credentials> {
        let inner = self.inner.as_ref()?;
        let mut cached = inner.cached.lock().unwrap();
        let Ok(meta) = std::fs::metadata(&inner.path) else {
            *cached = None;
            return None;
        };
        let Ok(mtime) = meta.modified() else {
            *cached = None;
            return None;
        };
        let key = (mtime, meta.len());
        if let Some((k, c)) = cached.as_ref()
            && *k == key
        {
            return Some(c.clone());
        }
        match credentials::load(&inner.path) {
            Ok(Some(c)) => {
                *cached = Some((key, c.clone()));
                Some(c)
            }
            Ok(None) => {
                *cached = None;
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "cloud.json 读取失败,视为未登录");
                None
            }
        }
    }

    pub fn server_id(&self) -> Option<String> {
        self.get().and_then(|c| c.server_id)
    }
}
```

单测改为用 `SharedCreds::new(Some(path))` 调 `get()`,断言不变;再加一个 `server_id_none_when_logged_out`(无文件 → `None`;写入带 `server_id: Some("s")` 的凭据 → `Some("s")`)。

`cloud/mod.rs` 加 `pub mod creds;`。

- [ ] **Step 4: notifier 改用 SharedCreds**

`notifier::spawn` 签名改为 `pub fn spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, creds: SharedCreds)`;删除本文件的 `CredCache`;循环里 `cache.get()` 改为 `creds.get()`;`send()` 的 401 分支需要的路径改从 `creds.path()` 取(`let Some(path) = creds.path().map(Path::to_path_buf) else { return }` 在 spawn 开头取一次即可;`path()` 为 None 时直接 return,与旧行为「无路径不启动」一致)。

`crates/server/tests/cloud_notifier.rs` 的 `Rig::new` 中 `notifier::spawn(..., Some(path))` 改为 `notifier::spawn(..., SharedCreds::new(Some(path)))`(import `gateterm_server::cloud::creds::SharedCreds`)。

- [ ] **Step 5: AppState 与握手**

`crates/server/src/app.rs`:
- `AppState` 末尾加:
  ```rust
      /// Cloud 登录态(只读)。握手时回带 `cloud_server_id`;未登录时为空。
      pub cloud: crate::cloud::creds::SharedCreds,
  ```
- 握手处:
  ```rust
              let _ = socket.send(control_msg(&ServerMsg::HelloOk {
                  proto: PROTO_VERSION,
                  os: std::env::consts::OS.to_string(),
                  cloud_server_id: state.cloud.server_id(),
              })).await;
  ```

各 `AppState` 构造处补字段:
- `crates/server/src/main.rs`:
  ```rust
      let cloud = gateterm_server::cloud::creds::SharedCreds::new(gateterm_cloud_client::credentials::default_path());
      let state = AppState { manager, token, launchers, listen_port, agents, agent_sessions, cloud };
  ```
  notifier 调用改为 `notifier::spawn(state.manager.clone(), state.agent_sessions.clone(), state.cloud.clone());`
- `crates/server/src/agent/history.rs` 测试里的 `fn state()`:加 `cloud: Default::default(),`
- `crates/server/tests/common/mod.rs` 现有构造:加 `cloud: Default::default(),`;并新增(复制现有 spawn 函数体,只换 cloud 与参数):
  ```rust
  /// 带 Cloud 登录态的 server,用于验证握手回带 cloud_server_id。
  pub async fn spawn_server_with_cloud(cloud: gateterm_server::cloud::creds::SharedCreds, token: &str) -> std::net::SocketAddr
  ```
  其余参数取现有测试最常用的默认值(缓冲 `1 << 20`、默认窗口策略、空 launcher、默认 `AgentsConfig`)。
- `crates/desktop/src/supervisor.rs`:`Supervisor` 加字段 `cloud: gateterm_server::cloud::creds::SharedCreds`,在 `new()` 里 `SharedCreds::new(gateterm_cloud_client::credentials::default_path())`;`restart()` 构造 `AppState` 时 `cloud: self.cloud.clone()`;加访问器 `pub fn cloud_creds(&self) -> &gateterm_server::cloud::creds::SharedCreds { &self.cloud }`。
- `crates/desktop/src/main.rs`:notifier 调用第三个参数改为 `sup.cloud_creds().clone()`。

`crates/server/tests/ws_auth.rs` 里精确比较 `HelloOk` 的字面量补 `cloud_server_id: None`。

- [ ] **Step 6: 握手集成测试 `crates/server/tests/ws_cloud_hello.rs`**

照 `tests/ws_auth.rs` 的连接与 hello 写法(复用 `common` 里的 `control` 等辅助):

```rust
mod common;

use gateterm_cloud_client::credentials::{self, Credentials};
use gateterm_protocol::ServerMsg;
use gateterm_server::cloud::creds::SharedCreds;

// 发 hello 并取回第一条控制消息;具体写法照搬 ws_auth.rs 里的同名步骤。
async fn hello(addr: std::net::SocketAddr, token: &str) -> ServerMsg {
    common::hello_and_first_control(addr, token).await
}

#[tokio::test]
async fn hello_ok_carries_cloud_server_id_when_logged_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cloud.json");
    credentials::save(&path, &Credentials { url: "https://c".into(), email: "a@b.c".into(), token: "t".into(), server_id: Some("srv-1".into()) }).unwrap();
    let addr = common::spawn_server_with_cloud(SharedCreds::new(Some(path)), "tok").await;
    match hello(addr, "tok").await {
        ServerMsg::HelloOk { cloud_server_id, .. } => assert_eq!(cloud_server_id.as_deref(), Some("srv-1")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn hello_ok_has_no_cloud_server_id_when_logged_out() {
    let dir = tempfile::tempdir().unwrap();
    let addr = common::spawn_server_with_cloud(SharedCreds::new(Some(dir.path().join("cloud.json"))), "tok").await;
    match hello(addr, "tok").await {
        ServerMsg::HelloOk { cloud_server_id, .. } => assert_eq!(cloud_server_id, None),
        other => panic!("{other:?}"),
    }
}
```

若 `common` 里还没有 `hello_and_first_control`,把 `ws_auth.rs` 里「连接 → 发 Hello → 读第一条控制消息并解析为 ServerMsg」的代码提成这个 pub 辅助函数(`ws_auth.rs` 改为调用它),不要复制粘贴两份。

- [ ] **Step 7: 运行并提交**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/protocol crates/server crates/desktop
git commit -m "feat(server): 握手回带 cloud_server_id,cloud.json 读取共享缓存

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: 通知器发撤回与过期

**Files:**
- Modify: `crates/server/src/cloud/notifier.rs`, `crates/server/tests/cloud_notifier.rs`

**Interfaces:**
- Produces:
  - `notifier::Track { pub state: Option<AgentState>, pub pending: Option<String>, pub notified: bool }`(`Default`)
  - `notifier::decide(prev: &Track, state: AgentState, pending: &Option<String>) -> Option<&'static str>`

- [ ] **Step 1: 单测(notifier.rs 的 tests 模块)**

```rust
    fn track(state: Option<AgentState>, pending: Option<&str>, notified: bool) -> Track {
        Track { state, pending: pending.map(String::from), notified }
    }

    #[test]
    fn decide_transitions() {
        assert_eq!(decide(&track(None, None, false), Waiting, &None), Some("waiting"));
        assert_eq!(decide(&track(Some(Waiting), None, true), Waiting, &Some("p".into())), Some("permission"));
        assert_eq!(decide(&track(Some(Waiting), None, true), Working, &None), Some("resolved"));
        assert_eq!(decide(&track(Some(Waiting), None, true), Done, &None), Some("resolved"));
        // 从没真的发出过通知(比如当时没登录)就不撤回
        assert_eq!(decide(&track(Some(Waiting), None, false), Working, &None), None);
        assert_eq!(decide(&track(Some(Working), None, false), Idle, &None), None);
    }
```

- [ ] **Step 2: 集成测试(`tests/cloud_notifier.rs` 追加)**

`Rig` 增加字段 `store: Arc<AgentSessionStore>`(spawn 时 clone 进去),新增方法 `login(&self, url: &str)`(写 `cloud.json`,与 `new(Some(url))` 相同逻辑,提成共用函数)。

```rust
#[tokio::test]
async fn leaving_waiting_sends_resolved() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Working, None));
    wait_for(&seen, 2).await;
    let seen = seen.lock().unwrap().clone();
    let resolved: Vec<_> = seen.iter().filter(|b| b["kind"] == "resolved").collect();
    assert_eq!(resolved.len(), 1);
    assert_eq!(*resolved[0], json!({"session": 7, "agent_id": "claude-code", "kind": "resolved"}));
}

#[tokio::test]
async fn no_resolved_for_a_wait_that_was_never_notified() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(None);
    rig.manager.publish(event(AgentState::Waiting, None));
    tokio::time::sleep(Duration::from_millis(100)).await;
    rig.login(&url);
    rig.manager.publish(event(AgentState::Working, None));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn permission_carries_expires_at() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    let expires = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 590) as u64;
    let _rx = rig.store.insert_pending(gateterm_server::agent::store::PendingRequest {
        id: "p1".into(),
        agent_id: "claude-code".into(),
        agent_session_id: "s-1".into(),
        session: Some(7),
        tool_name: Some("Bash".into()),
        tool_input: json!({"command": "rm -rf /tmp/x"}),
        created_at: expires - 590,
        expires_at: expires,
    });
    rig.manager.publish(event(AgentState::Waiting, Some("p1")));
    wait_for(&seen, 1).await;
    let body = seen.lock().unwrap()[0].clone();
    assert_eq!(body["kind"], "permission");
    assert_eq!(body["tool_name"], "Bash");
    assert_eq!(body["expires_at"], expires as i64);
    assert!(body.get("tool_input").is_none());
}
```

(`PendingRequest` 若还有本计划未列出的字段,按其定义补齐默认值。)

Run: `cargo test -p gateterm-server --test cloud_notifier`
Expected: 新测试失败。

- [ ] **Step 3: 实现**

在 `classify` 之后加:

```rust
/// 每个 agent 会话上次的状态,以及是否真的为它发出过「在等你」——
/// 决定离开 waiting 时要不要撤回:从没通知过的等待,撤回只会是噪音。
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub state: Option<AgentState>,
    pub pending: Option<String>,
    pub notified: bool,
}

/// 在 `classify`(进入/新 pending)之上补「离开 waiting → resolved」。
pub fn decide(prev: &Track, state: AgentState, pending: &Option<String>) -> Option<&'static str> {
    if state == AgentState::Waiting {
        let p = prev.state.map(|s| (s, prev.pending.clone()));
        return classify(p.as_ref(), state, pending);
    }
    (prev.state == Some(AgentState::Waiting) && prev.notified).then_some("resolved")
}
```

循环体替换为:

```rust
            let key = (agent_id.clone(), agent_session_id);
            let prev = last.get(&key).cloned().unwrap_or_default();
            let kind = decide(&prev, state, &pending);
            // 只有真要发时才读 cloud.json;login 为 None = 未登录
            let login = kind.and(session).and_then(|_| creds.get());
            let sending = kind.is_some() && login.is_some();
            let notified = match kind {
                Some("waiting" | "permission") => sending || prev.notified,
                _ if state != AgentState::Waiting => false,
                _ => prev.notified,
            };
            if state == AgentState::Done {
                last.remove(&key);
            } else {
                last.insert(key, Track { state: Some(state), pending: pending.clone(), notified });
            }
            let (Some(kind), Some(session), Some(login)) = (kind, session, login) else { continue };
            let pending_req = (kind == "permission")
                .then(|| pending.as_ref().and_then(|id| store.list_pending().into_iter().find(|p| &p.id == id)))
                .flatten();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let note = Notify {
                session,
                agent_id,
                kind: kind.into(),
                tool_name: pending_req.as_ref().and_then(|p| p.tool_name.clone()),
                // Cloud 只接受一小时内的过期时刻;pending 超时可配置得更长,截断即可
                expires_at: pending_req.map(|p| (p.expires_at as i64).min(now + 3600)),
            };
```

(其后的 client 复用与 `tokio::spawn(send(...))` 不变,只是把原来的 `creds` 实参换成 `login`。注意 `waiting` 推送不再带 tool_name——旧代码在 waiting 时 pending 为 None,行为一致。)

- [ ] **Step 4: 运行并提交**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`;另外 `cargo test -p gateterm-server --test cloud_notifier` 连跑 5 次确认稳定。

```bash
git add crates/server/src/cloud/notifier.rs crates/server/tests/cloud_notifier.rs
git commit -m "feat(server): 通知器在离开等待时撤回,待授权推送带过期时间

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `crates/cloud-ffi`(UniFFI)与 Android 构建脚本

**Files:**
- Create: `crates/cloud-ffi/Cargo.toml`, `crates/cloud-ffi/src/lib.rs`, `crates/cloud-ffi/src/bin/uniffi-bindgen.rs`, `crates/cloud-ffi/tests/ffi.rs`, `scripts/build-android-ffi.sh`
- Modify: `Cargo.toml`(workspace members), `.gitignore`

**Interfaces:**
- Consumes: `ops::{current, normalize_email, register, login, reset_password, logout, request_delete_code, delete_account, register_push_target, OpsError}`、`CloudClient::request_code`、`ClientKind::Device`、`CodePurpose`
- Produces(Kotlin 包 `uniffi.gateterm_cloud_ffi`):
  - `class CloudAccount(credPath: String, label: String)`:`status(): CloudStatus?`;suspend:`requestCode(url, email, purpose: Purpose)`、`register(url, email, code, password): CloudStatus`、`login(url, email, password): CloudStatus`、`resetPassword(url, email, code, newPassword)`、`logout(): Boolean`(true = Cloud 也已撤销)、`requestDeleteCode(): String`(邮箱)、`deleteAccount(password, code)`、`registerPushTarget(platform, token)`
  - `data class CloudStatus(email: String, url: String)`、`enum class Purpose { REGISTER, RESET }`
  - `sealed class CloudFfiException`:`NotLoggedIn`、`Unauthorized`、`RateLimited(retryAfter: ULong?)`、`Api(code: String)`、`Unreachable(message: String)`、`Local(message: String)`
  - 构建产物:`libgateterm_cloud_ffi.so`(arm64-v8a)与 `uniffi/gateterm_cloud_ffi/gateterm_cloud_ffi.kt`
  - `scripts/build-android-ffi.sh <jniLibs 输出目录> <Kotlin 输出目录>`

- [ ] **Step 1: 前置检查**

```bash
command -v cargo-ndk && rustup target list --installed | grep aarch64-linux-android && echo "$ANDROID_NDK_HOME"
```

Expected: 三项都有输出。`cargo-ndk` 缺失或 `ANDROID_NDK_HOME` 为空时,报告 BLOCKED(由 hiro 安装 / 设置 `ANDROID_NDK_HOME=/Users/hiro/frameworks/android-ndk-r30`),不要自行安装。

- [ ] **Step 2: crate 骨架**

workspace `Cargo.toml` members 末尾加 `"crates/cloud-ffi"`。

`crates/cloud-ffi/Cargo.toml`:

```toml
[package]
name = "gateterm-cloud-ffi"
version = "0.2.1"
edition.workspace = true
license.workspace = true

[lib]
name = "gateterm_cloud_ffi"
# cdylib 给 Android(JNA 加载),staticlib 留给 iOS,lib 给本 crate 的 Rust 测试
crate-type = ["cdylib", "staticlib", "lib"]

[[bin]]
name = "uniffi-bindgen"
path = "src/bin/uniffi-bindgen.rs"
required-features = ["cli"]

[features]
cli = ["uniffi/cli"]

[dependencies]
gateterm-cloud-client = { path = "../cloud-client" }
thiserror = "2"
uniffi = { version = "0.29", features = ["tokio"] }

[dev-dependencies]
axum = "0.8"
serde_json = "1"
tempfile = "3"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
```

`crates/cloud-ffi/src/bin/uniffi-bindgen.rs`:

```rust
fn main() {
    uniffi::uniffi_bindgen_main()
}
```

`.gitignore` 追加:

```
clients/android/app/src/main/jniLibs/
```

- [ ] **Step 3: 失败测试 `crates/cloud-ffi/tests/ffi.rs`**

```rust
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, put};
use gateterm_cloud_ffi::{CloudAccount, CloudFfiError};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<(String, Option<String>, Value)>>>;

/// 最小假 Cloud:登录成功;推送登记在 token 为 "revoked" 时回 401 unauthorized。
async fn fake() -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s1 = seen.clone();
    let s2 = seen.clone();
    let app = Router::new()
        .route(
            "/v1/auth/login",
            post(move |axum::Json(b): axum::Json<Value>| {
                let s = s1.clone();
                async move {
                    s.lock().unwrap().push(("login".into(), None, b));
                    axum::Json(json!({"token": "oct_dev", "account_id": "a1"}))
                }
            }),
        )
        .route(
            "/v1/push-target",
            put(move |h: HeaderMap, axum::Json(b): axum::Json<Value>| {
                let s = s2.clone();
                async move {
                    let auth = h.get("authorization").map(|v| v.to_str().unwrap().to_string());
                    let revoked = b["push_token"] == "revoked";
                    s.lock().unwrap().push(("push".into(), auth, b));
                    if revoked {
                        (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "unauthorized"})))
                    } else {
                        (StatusCode::NO_CONTENT, axum::Json(json!(null)))
                    }
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

#[tokio::test]
async fn login_as_device_then_register_push_target() {
    let (url, seen) = fake().await;
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "Pixel 9".into());
    assert!(acct.status().is_none());

    let st = acct.login(url.clone(), " A@Example.com ".into(), "pw-pw-pw".into()).await.unwrap();
    assert_eq!(st.email, "a@example.com");
    assert_eq!(acct.status().unwrap().url, url);

    acct.register_push_target("fcm".into(), "fcm-tok".into()).await.unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[0].2["client"], json!({"kind": "device", "label": "Pixel 9"}));
    assert_eq!(seen[1].1.as_deref(), Some("Bearer oct_dev"));
    assert_eq!(seen[1].2, json!({"platform": "fcm", "push_token": "fcm-tok"}));
}

#[tokio::test]
async fn unauthorized_maps_to_error_code_and_clears_login() {
    let (url, _seen) = fake().await;
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "p".into());
    acct.login(url, "a@example.com".into(), "pw-pw-pw".into()).await.unwrap();
    let err = acct.register_push_target("fcm".into(), "revoked".into()).await.unwrap_err();
    assert!(matches!(err, CloudFfiError::Unauthorized), "{err:?}");
    assert!(acct.status().is_none());
}

#[tokio::test]
async fn not_logged_in_is_its_own_error() {
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "p".into());
    assert!(matches!(acct.request_delete_code().await, Err(CloudFfiError::NotLoggedIn)));
}
```

Run: `cargo test -p gateterm-cloud-ffi`
Expected: 编译失败(lib 尚未实现)。

- [ ] **Step 4: `crates/cloud-ffi/src/lib.rs`**

```rust
//! 把 `gateterm-cloud-client::ops` 用 UniFFI 导出给移动端(Android 现在、iOS 将来)。
//!
//! 只做类型转换:账号逻辑、吊销规则、凭据文件都在 `ops` 里,三端行为因此一致。
//! 错误只给错误码,文案由 App 按自己的语言生成。

uniffi::setup_scaffolding!();

use std::path::PathBuf;
use std::sync::Arc;

use gateterm_cloud_client::ops::{self, OpsError};
use gateterm_cloud_client::{ClientKind, CloudClient, CloudError, CodePurpose};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CloudFfiError {
    #[error("not logged in")]
    NotLoggedIn,
    #[error("unauthorized")]
    Unauthorized,
    #[error("rate limited")]
    RateLimited { retry_after: Option<u64> },
    #[error("api error: {code}")]
    Api { code: String },
    #[error("unreachable: {message}")]
    Unreachable { message: String },
    #[error("local error: {message}")]
    Local { message: String },
}

impl From<CloudError> for CloudFfiError {
    fn from(e: CloudError) -> Self {
        match e {
            CloudError::Unauthorized => Self::Unauthorized,
            CloudError::RateLimited { retry_after } => Self::RateLimited { retry_after },
            CloudError::Api { code, .. } => Self::Api { code },
            CloudError::Unreachable(message) => Self::Unreachable { message },
        }
    }
}

impl From<OpsError> for CloudFfiError {
    fn from(e: OpsError) -> Self {
        match e {
            OpsError::NotLoggedIn => Self::NotLoggedIn,
            OpsError::Local(message) => Self::Local { message },
            OpsError::Cloud(e) => e.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Purpose {
    Register,
    Reset,
}

impl From<Purpose> for CodePurpose {
    fn from(p: Purpose) -> Self {
        match p {
            Purpose::Register => CodePurpose::Register,
            Purpose::Reset => CodePurpose::Reset,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CloudStatus {
    pub email: String,
    pub url: String,
}

/// 一台设备上的 Cloud 账号。`cred_path` 由 App 给(Android:`filesDir/cloud.json`)。
#[derive(uniffi::Object)]
pub struct CloudAccount {
    path: PathBuf,
    label: String,
}

#[uniffi::export(async_runtime = "tokio")]
impl CloudAccount {
    #[uniffi::constructor]
    pub fn new(cred_path: String, label: String) -> Arc<Self> {
        Arc::new(Self { path: PathBuf::from(cred_path), label })
    }

    /// 只读本地凭据文件,不联网。
    pub fn status(&self) -> Option<CloudStatus> {
        ops::current(&self.path).ok().map(|c| CloudStatus { email: c.email, url: c.url })
    }

    pub async fn request_code(&self, url: String, email: String, purpose: Purpose) -> Result<(), CloudFfiError> {
        CloudClient::new(&url).request_code(&ops::normalize_email(&email), purpose.into()).await?;
        Ok(())
    }

    pub async fn register(&self, url: String, email: String, code: String, password: String) -> Result<CloudStatus, CloudFfiError> {
        let c = ops::register(&self.path, &url, &email, &code, &password, ClientKind::Device, &self.label).await?;
        Ok(CloudStatus { email: c.email, url: c.url })
    }

    pub async fn login(&self, url: String, email: String, password: String) -> Result<CloudStatus, CloudFfiError> {
        let c = ops::login(&self.path, &url, &email, &password, ClientKind::Device, &self.label).await?;
        Ok(CloudStatus { email: c.email, url: c.url })
    }

    pub async fn reset_password(&self, url: String, email: String, code: String, new_password: String) -> Result<(), CloudFfiError> {
        ops::reset_password(&self.path, &url, &email, &code, &new_password).await?;
        Ok(())
    }

    /// 本地登录态一定清掉。返回 true 表示 Cloud 上的凭据也已撤销。
    pub async fn logout(&self) -> Result<bool, CloudFfiError> {
        Ok(ops::logout(&self.path).await?.is_none())
    }

    pub async fn request_delete_code(&self) -> Result<String, CloudFfiError> {
        Ok(ops::request_delete_code(&self.path).await?)
    }

    pub async fn delete_account(&self, password: String, code: String) -> Result<(), CloudFfiError> {
        ops::delete_account(&self.path, &password, &code).await?;
        Ok(())
    }

    pub async fn register_push_target(&self, platform: String, token: String) -> Result<(), CloudFfiError> {
        ops::register_push_target(&self.path, &platform, &token).await?;
        Ok(())
    }
}
```

> 若 UniFFI 0.29 的宏或 `async_runtime` 属性写法与上面不符(版本差异),按该版本文档做最小改动,并在报告中写明;Kotlin 侧接口名(`CloudAccount`、`CloudStatus`、`Purpose`、`CloudFfiException.*`)必须保持。

Run: `cargo test -p gateterm-cloud-ffi`
Expected: 3 个测试 PASS。

- [ ] **Step 5: `scripts/build-android-ffi.sh`**

```bash
#!/usr/bin/env bash
# 构建 Android 用的 Rust 库(arm64-v8a)与 UniFFI Kotlin 绑定。
# 由 clients/android 的 Gradle 任务 buildCloudFfi 调用,也可手动运行。
# 用法:scripts/build-android-ffi.sh <jniLibs 输出目录> <Kotlin 输出目录>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_JNI="${1:?需要 jniLibs 输出目录}"
OUT_KT="${2:?需要 Kotlin 输出目录}"
: "${ANDROID_NDK_HOME:?需要 ANDROID_NDK_HOME(本机:/Users/hiro/frameworks/android-ndk-r30)}"

cd "$ROOT"
cargo ndk -t arm64-v8a -o "$OUT_JNI" build --release -p gateterm-cloud-ffi
rm -rf "$OUT_KT"
cargo run -q -p gateterm-cloud-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$OUT_JNI/arm64-v8a/libgateterm_cloud_ffi.so" \
  --language kotlin --out-dir "$OUT_KT" --no-format
```

```bash
chmod +x scripts/build-android-ffi.sh
```

- [ ] **Step 6: 验证脚本**

```bash
T=$(mktemp -d) && ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-/Users/hiro/frameworks/android-ndk-r30} scripts/build-android-ffi.sh "$T/jni" "$T/kt" \
  && ls -la "$T/jni/arm64-v8a/libgateterm_cloud_ffi.so" && find "$T/kt" -name '*.kt' \
  && grep -c "class CloudAccount" $(find "$T/kt" -name '*.kt')
```

Expected:`.so` 存在(记下大小写进报告);生成 `uniffi/gateterm_cloud_ffi/gateterm_cloud_ffi.kt`;`class CloudAccount` 计数 ≥ 1。

- [ ] **Step 7: 全量并提交**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add Cargo.toml Cargo.lock .gitignore crates/cloud-ffi scripts/build-android-ffi.sh
git commit -m "feat: cloud-ffi(UniFFI 导出账号操作)与 Android 构建脚本

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 8: 合回 dev(由 controller 在终审后执行)**

```bash
git checkout dev && git merge --ff-only feat/push-server-ffi
```
