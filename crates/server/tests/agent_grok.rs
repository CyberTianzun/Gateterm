//! Grok adapter。依据是 grok 1.0.52 随安装附带的 hooks 文档,差异都在这里钉死。
//!
//! 全部用 tempfile 造假 home,不碰真实的 `~/.grok`。

mod common;

use octoterm_server::agent::apply::{apply, ApplyOpts};
use octoterm_server::agent::detect::DetectEnv;
use octoterm_server::agent::edit::InstallCtx;
use octoterm_server::agent::store::hook_token;
use octoterm_server::agent::{find, plan_purge, scan, Confidence, Integration};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

struct Env {
    home: tempfile::TempDir,
    backups: tempfile::TempDir,
}

fn env() -> Env {
    Env { home: tempfile::tempdir().unwrap(), backups: tempfile::tempdir().unwrap() }
}

impl Env {
    fn hooks(&self) -> PathBuf {
        self.home.path().join(".grok").join("hooks").join("octoterm.json")
    }
    fn ctx(&self, port: u16) -> InstallCtx {
        InstallCtx { home: self.home.path().to_path_buf(), port, include_blocking: true }
    }
    fn opts(&self) -> ApplyOpts {
        ApplyOpts { enabled: true, backup_dir: self.backups.path().to_path_buf(), backup_keep: 5 }
    }
    fn install(&self, port: u16) {
        apply(&find("grok").unwrap().plan_install(&self.ctx(port)).unwrap(), &self.opts()).unwrap();
    }
    fn detect_env(&self) -> DetectEnv {
        DetectEnv { home: self.home.path().to_path_buf(), path: None }
    }
    fn integration(&self, port: u16) -> Integration {
        scan(&self.detect_env(), port).into_iter().find(|a| a.id == "grok").unwrap().integration
    }
    fn doc(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.hooks()).unwrap()).unwrap()
    }
}

/// 整份文件只放我们的 hook,形状是 grok 文档里的 `{"hooks": {Event: [{"hooks": [...]}]}}`。
/// 只能是 command:http 的 `headers` 会被 grok 丢掉,带不上密钥。
#[test]
fn installs_command_hooks_into_its_own_file() {
    let e = env();
    e.install(7683);
    let doc = e.doc();
    for event in ["SessionStart", "UserPromptSubmit", "PreToolUse", "Stop", "Notification", "SessionEnd"] {
        let hook = &doc["hooks"][event][0]["hooks"][0];
        assert_eq!(hook["type"], "command", "{event}");
        let cmd = hook["command"].as_str().unwrap();
        assert!(cmd.contains(" hook http://127.0.0.1:7683/hook/grok/"), "{cmd}");
        assert!(hook["timeout"].as_u64().unwrap() <= 30, "不该沿用 grok 给 Stop 的 600 秒默认");
        assert!(doc["hooks"][event][0].get("matcher").is_none(), "Stop / UserPromptSubmit 上带 matcher 会被警告");
    }
    assert!(doc["hooks"].get("PermissionRequest").is_none(), "grok 没有这个事件");
}

/// 全局 hook 目录「Always trusted」:装上即是 Installed,不像 Codex 那样要审核。
#[test]
fn install_is_installed_and_port_change_is_stale() {
    let e = env();
    assert_eq!(e.integration(7683), Integration::NotInstalled);
    e.install(7683);
    assert_eq!(e.integration(7683), Integration::Installed);
    assert_eq!(e.integration(9999), Integration::StalePort);
}

#[test]
fn purge_leaves_none_of_ours() {
    let e = env();
    e.install(7683);
    let a = find("grok").unwrap();
    apply(&plan_purge(a.as_ref(), &e.ctx(0)).unwrap(), &e.opts()).unwrap();
    assert_eq!(e.integration(7683), Integration::NotInstalled);
}

#[test]
fn config_toml_is_hard_evidence_and_our_hooks_dir_is_not() {
    let e = env();
    let dir = e.home.path().join(".grok");
    fs::create_dir_all(dir.join("hooks")).unwrap();
    let d = find("grok").unwrap().detect(&e.detect_env());
    assert!(!d.installed, "hooks/ 可能是我们自己建的");
    fs::write(dir.join("config.toml"), "").unwrap();
    let d = find("grok").unwrap().detect(&e.detect_env());
    assert_eq!((d.installed, d.confidence, d.reason), (true, Confidence::High, "config-file"));
}

/// payload 是驼峰的(`sessionId`、`toolName`、`notificationType`)。
#[test]
fn parses_camel_case_payloads() {
    let g = find("grok").unwrap();
    let body = json!({ "sessionId": "abc", "cwd": "/w", "toolName": "run_terminal_command" });
    assert_eq!(g.agent_session_id(&body).as_deref(), Some("abc"));
    let up = g.parse("PreToolUse", &body).unwrap();
    assert_eq!(up.detail.as_deref(), Some("run_terminal_command"));
    assert_eq!(up.cwd.as_deref(), Some("/w"));

    let waiting = g.parse("Notification", &json!({ "notificationType": "permission_prompt" })).unwrap();
    assert_eq!(waiting.state, Some(octoterm_protocol::AgentState::Waiting));
    assert!(g.parse("Notification", &json!({ "notificationType": "task_complete" })).is_none());
    assert_eq!(
        g.parse("SessionEnd", &json!({})).unwrap().state,
        Some(octoterm_protocol::AgentState::Done)
    );
}

/// 子代理有自己的会话。不上报:它不是终端前的那个会话。
#[test]
fn subagent_events_are_not_reported() {
    let g = find("grok").unwrap();
    let body = json!({ "sessionId": "child", "toolName": "read_file", "subagentType": "explore" });
    assert!(g.parse("PreToolUse", &body).is_none());
}

/// 端到端:grok 形状的 payload 走 hook 路由,会话 id 从 `sessionId` 取。
#[tokio::test]
async fn hook_route_reads_camel_case_session_id() {
    let addr = common::start_test_server_at("tok", 1 << 16, Default::default(), vec![]).await;
    let r = reqwest::Client::new()
        .post(format!("http://{addr}/hook/grok/user-prompt-submit"))
        .header("Authorization", format!("Bearer {}", hook_token()))
        .header("X-Octoterm-Session", "3")
        .body(json!({ "sessionId": "g-1", "cwd": "/w", "hook_event_name": "UserPromptSubmit" }).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = reqwest::Client::new()
        .get(format!("http://{addr}/api/agents/sessions"))
        .header("Authorization", "Bearer tok")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap();
    let s = &v["sessions"][0];
    assert_eq!((s["agent_id"].as_str(), s["agent_session_id"].as_str()), (Some("grok"), Some("g-1")));
    assert_eq!(s["session"], 3);
    assert_eq!(s["state"], "thinking");
}
