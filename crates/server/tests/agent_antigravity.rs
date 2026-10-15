//! Antigravity(agy)adapter。依据是 agy 1.3.5 内嵌的 hooks 文档,关键行为本机实测过。
//!
//! 全部用 tempfile 造假 home,不碰真实的 `~/.gemini`。

mod common;

use octoterm_server::agent::apply::{apply, ApplyError, ApplyOpts};
use octoterm_server::agent::antigravity_transcript::{locate, parse};
use octoterm_server::agent::detect::DetectEnv;
use octoterm_server::agent::edit::InstallCtx;
use octoterm_server::agent::store::hook_token;
use octoterm_server::agent::transcript::{Block, Role};
use octoterm_server::agent::{find, plan_purge, scan, Integration};
use octoterm_protocol::AgentState;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const FIXTURE: &str = include_str!("fixtures/antigravity-transcript.jsonl");

struct Env {
    home: tempfile::TempDir,
    backups: tempfile::TempDir,
}

fn env() -> Env {
    Env { home: tempfile::tempdir().unwrap(), backups: tempfile::tempdir().unwrap() }
}

impl Env {
    fn hooks(&self) -> PathBuf {
        self.home.path().join(".gemini/config/hooks.json")
    }
    fn ctx(&self, port: u16) -> InstallCtx {
        InstallCtx { home: self.home.path().to_path_buf(), port, include_blocking: true }
    }
    fn opts(&self) -> ApplyOpts {
        ApplyOpts { enabled: true, backup_dir: self.backups.path().to_path_buf(), backup_keep: 5 }
    }
    fn install(&self, port: u16) -> Result<(), ApplyError> {
        apply(&find("antigravity").unwrap().plan_install(&self.ctx(port)).unwrap(), &self.opts()).map(|_| ())
    }
    fn uninstall(&self) {
        apply(&find("antigravity").unwrap().plan_uninstall(&self.ctx(0)).unwrap(), &self.opts()).unwrap();
    }
    fn integration(&self, port: u16) -> Integration {
        let env = DetectEnv { home: self.home.path().to_path_buf(), path: None };
        scan(&env, port).into_iter().find(|a| a.id == "antigravity").unwrap().integration
    }
    fn seed(&self, v: &Value) {
        fs::create_dir_all(self.hooks().parent().unwrap()).unwrap();
        fs::write(self.hooks(), serde_json::to_string_pretty(v).unwrap()).unwrap();
    }
    fn doc(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.hooks()).unwrap()).unwrap()
    }
}

/// 共用文件里只占 `octoterm` 这一个顶层键,别人的具名 hook 原样不动。
#[test]
fn installs_as_one_named_entry_in_the_shared_file() {
    let e = env();
    let theirs = json!({ "lint": { "PostToolUse": [{ "matcher": "run_command", "hooks": [{ "command": "./lint.sh" }] }] } });
    e.seed(&theirs);
    e.install(7683).unwrap();
    let doc = e.doc();
    assert_eq!(doc["lint"], theirs["lint"]);
    let ours = &doc["octoterm"];
    // 平铺事件直接是处理器列表;工具事件要包 {matcher, hooks}
    assert!(ours["SessionStart"][0]["command"].as_str().unwrap().contains("/hook/antigravity/session-start"));
    assert!(ours["Stop"][0]["command"].is_string());
    assert_eq!(ours["PostToolUse"][0]["matcher"], "*");
    assert!(ours["PostToolUse"][0]["hooks"][0]["command"].as_str().unwrap().contains("/hook/antigravity/post-tool-use"));
}

/// 实测:PreToolUse 的回复里没有 decision 时 agy 直接拒绝工具调用;能填的 decision 又都会
/// 改掉用户自己的授权流程。所以绝不能装。
#[test]
fn never_installs_pre_tool_use() {
    let e = env();
    e.install(7683).unwrap();
    assert!(e.doc()["octoterm"].get("PreToolUse").is_none());
}

/// 同名但不是我们写的条目:拒绝覆盖,文件原样不动。
#[test]
fn foreign_entry_with_our_name_is_never_overwritten() {
    let e = env();
    let foreign = json!({ "octoterm": { "Stop": [{ "command": "./mine.sh" }] } });
    e.seed(&foreign);
    assert!(matches!(e.install(7683), Err(ApplyError::Invalid { .. })));
    assert_eq!(e.doc(), foreign);
    e.uninstall();
    assert_eq!(e.doc(), foreign, "卸载也不能删别人的同名条目");
}

#[test]
fn integration_states_and_uninstall_keeps_others() {
    let e = env();
    e.seed(&json!({ "lint": { "Stop": [{ "command": "./lint.sh" }] } }));
    assert_eq!(e.integration(7683), Integration::NotInstalled);
    e.install(7683).unwrap();
    assert_eq!(e.integration(7683), Integration::Installed);
    assert_eq!(e.integration(9999), Integration::StalePort);
    // 端口变了重装一遍就对齐,不会留两份
    e.install(9999).unwrap();
    assert_eq!(e.integration(9999), Integration::Installed);
    e.uninstall();
    assert_eq!(e.doc(), json!({ "lint": { "Stop": [{ "command": "./lint.sh" }] } }));
}

#[test]
fn purge_removes_our_entry() {
    let e = env();
    e.install(7683).unwrap();
    let a = find("antigravity").unwrap();
    apply(&plan_purge(a.as_ref(), &e.ctx(0)).unwrap(), &e.opts()).unwrap();
    assert!(e.doc().get("octoterm").is_none());
}

#[test]
fn uninstall_without_install_creates_nothing() {
    let e = env();
    e.uninstall();
    assert!(!e.hooks().exists());
}

/// payload 是驼峰的:会话 id 是 conversationId,每个事件都带 transcriptPath。
#[test]
fn parses_payloads() {
    let a = find("antigravity").unwrap();
    let body = json!({
        "conversationId": "c-1",
        "workspacePaths": ["/w"],
        "transcriptPath": "/t/transcript_full.jsonl",
        "toolCall": { "name": "view_file", "args": {} },
    });
    assert_eq!(a.agent_session_id(&body).as_deref(), Some("c-1"));
    let up = a.parse("PostToolUse", &body).unwrap();
    assert_eq!(up.state, Some(AgentState::Working));
    assert_eq!(up.detail.as_deref(), Some("view_file"));
    assert_eq!(up.cwd.as_deref(), Some("/w"));
    assert_eq!(up.transcript.as_deref(), Some("/t/transcript_full.jsonl"));
    assert_eq!(a.parse("PreInvocation", &body).unwrap().state, Some(AgentState::Thinking));
    assert_eq!(a.parse("Stop", &body).unwrap().state, Some(AgentState::Idle));
    assert!(a.parse("PostInvocation", &body).is_none());
}

/// agy 要求每个 hook 都在 stdout 输出 JSON 对象:状态事件经路由回 `{}`,不是空 body。
#[tokio::test]
async fn hook_route_replies_with_an_empty_json_object() {
    let addr = common::start_test_server_at("tok", 1 << 16, Default::default(), vec![]).await;
    let r = reqwest::Client::new()
        .post(format!("http://{addr}/hook/antigravity/pre-invocation"))
        .header("Authorization", format!("Bearer {}", hook_token()))
        .header("X-Octoterm-Session", "4")
        .body(json!({ "conversationId": "c-9", "workspacePaths": ["/w"] }).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.text().await.unwrap(), "{}");
}

#[test]
fn real_transcript_parses() {
    let msgs = parse(FIXTURE, 0);
    let shape: Vec<(String, Role, Vec<&str>)> = msgs
        .iter()
        .map(|m| {
            let kinds = m
                .blocks
                .iter()
                .map(|b| match b {
                    Block::Text { .. } => "text",
                    Block::Thinking { .. } => "thinking",
                    Block::ToolUse { .. } => "tool-use",
                    Block::ToolResult { ok: true, .. } => "ok",
                    Block::ToolResult { ok: false, .. } => "failed",
                })
                .collect();
            (m.id.clone(), m.role, kinds)
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("step-0".to_string(), Role::User, vec!["text"]),
            ("step-1".to_string(), Role::Assistant, vec!["tool-use", "tool-use"]),
            ("step-2".to_string(), Role::User, vec!["ok"]),
            ("step-3".to_string(), Role::User, vec!["failed"]),
            ("step-4".to_string(), Role::Assistant, vec!["text"]),
        ]
    );
    // 用户消息只留 <USER_REQUEST> 里面的,agy 附加的元数据去掉
    let Block::Text { text } = &msgs[0].blocks[0] else { panic!() };
    assert!(text.starts_with("Using only your file viewing tool") && !text.contains("ADDITIONAL_METADATA"));
    // 工具入参压成一行:view_file 显示路径
    let Block::ToolUse { name, input } = &msgs[1].blocks[0] else { panic!() };
    assert_eq!((name.as_str(), input.ends_with("README.md")), ("view_file", true));
    // 结果去掉了 Created At / Completed At 元数据
    let Block::ToolResult { text, .. } = &msgs[2].blocks[0] else { panic!() };
    assert!(!text.starts_with("Created At:"));
    assert!(msgs.iter().all(|m| m.ts.is_some()));
}

#[test]
fn locate_by_conversation_id() {
    let home = tempfile::tempdir().unwrap();
    let logs = home.path().join(".gemini/antigravity-cli/brain/c-1/.system_generated/logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("transcript_full.jsonl"), "").unwrap();
    assert_eq!(locate(home.path(), "c-1"), Some(logs.join("transcript_full.jsonl")));
    assert_eq!(locate(home.path(), "../c-1"), None);
}
