//! OpenCode adapter 集成测试:插件托管、状态转换、阻塞式权限审批、对话记录与历史会话发现。

mod common;

use gateterm_server::agent::apply::{apply, ApplyError, ApplyOpts};
use gateterm_server::agent::detect::DetectEnv;
use gateterm_server::agent::edit::InstallCtx;
use gateterm_server::agent::history;
use gateterm_server::agent::opencode::plugin_source;
use gateterm_server::agent::store::{hook_token, Decision};
use gateterm_server::agent::transcript::{Block, Role};
use gateterm_server::agent::{find, plan_purge, scan, Confidence, Integration};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

struct Env {
    home: tempfile::TempDir,
    backups: tempfile::TempDir,
}

fn env() -> Env {
    Env {
        home: tempfile::tempdir().unwrap(),
        backups: tempfile::tempdir().unwrap(),
    }
}

impl Env {
    fn plugin(&self) -> PathBuf {
        self.home.path().join(".config/opencode/plugins/gateterm.js")
    }
    fn ctx(&self, port: u16) -> InstallCtx {
        InstallCtx {
            home: self.home.path().to_path_buf(),
            port,
            include_blocking: true,
        }
    }
    fn opts(&self) -> ApplyOpts {
        ApplyOpts {
            enabled: true,
            backup_dir: self.backups.path().to_path_buf(),
            backup_keep: 5,
        }
    }
    fn install(&self, port: u16) -> Result<(), ApplyError> {
        apply(
            &find("opencode").unwrap().plan_install(&self.ctx(port)).unwrap(),
            &self.opts(),
        )
        .map(|_| ())
    }
    fn uninstall(&self) {
        apply(
            &find("opencode").unwrap().plan_uninstall(&self.ctx(7683)).unwrap(),
            &self.opts(),
        )
        .unwrap();
    }
    fn purge(&self) {
        let a = find("opencode").unwrap();
        apply(&plan_purge(a, &self.ctx(0)).unwrap(), &self.opts()).unwrap();
    }
    fn detect_env(&self) -> DetectEnv {
        DetectEnv {
            home: self.home.path().to_path_buf(),
            path: None,
        }
    }
    fn integration(&self, port: u16) -> Integration {
        scan(&self.detect_env(), port)
            .into_iter()
            .find(|a| a.id == "opencode")
            .unwrap()
            .integration
    }
}

#[test]
fn install_writes_plugin_and_uninstall_removes_it() {
    let e = env();
    assert_eq!(e.integration(7683), Integration::NotInstalled);
    e.install(7683).unwrap();
    assert_eq!(
        fs::read_to_string(e.plugin()).unwrap(),
        plugin_source(7683, true)
    );
    assert_eq!(e.integration(7683), Integration::Installed);
    e.install(7683).unwrap(); // 幂等
    e.uninstall();
    assert!(!e.plugin().exists());
    assert_eq!(e.integration(7683), Integration::NotInstalled);
}

#[test]
fn port_change_is_stale_port_and_old_content_is_outdated() {
    let e = env();
    e.install(9000).unwrap();
    assert_eq!(e.integration(7683), Integration::StalePort);
    e.install(7683).unwrap();
    assert_eq!(e.integration(7683), Integration::Installed);

    let modified = plugin_source(7683, true).replace("tool.execute.after", "tool.execute.done");
    fs::write(e.plugin(), modified).unwrap();
    assert_eq!(e.integration(7683), Integration::Outdated);
}

#[test]
fn foreign_plugin_with_same_name_is_never_touched() {
    let e = env();
    fs::create_dir_all(e.plugin().parent().unwrap()).unwrap();
    fs::write(e.plugin(), "export default {}\n").unwrap();
    assert!(matches!(e.install(7683), Err(ApplyError::Invalid { .. })));
    e.uninstall();
    e.purge();
    assert_eq!(
        fs::read_to_string(e.plugin()).unwrap(),
        "export default {}\n"
    );
}

#[test]
fn detect_recognizes_config_and_sqlite_db_but_ignores_plugins_dir() {
    let e = env();
    let cfg_dir = e.home.path().join(".config/opencode");
    fs::create_dir_all(cfg_dir.join("plugins")).unwrap();
    let a = find("opencode").unwrap();
    assert!(!a.detect(&e.detect_env()).installed);

    let db_dir = e.home.path().join(".local/share/opencode");
    fs::create_dir_all(&db_dir).unwrap();
    fs::write(db_dir.join("opencode.db"), "sqlite").unwrap();
    let d = a.detect(&e.detect_env());
    assert_eq!(
        (d.installed, d.confidence, d.reason),
        (true, Confidence::High, "config-file")
    );
}

#[test]
fn hook_events_and_permission_decision_work() {
    let a = find("opencode").unwrap();
    let body = json!({
        "session_id": "ses_abc",
        "cwd": "/repo",
        "tool_name": "bash",
        "transcript_path": "/tmp/ses_abc.jsonl",
        "session_title": "Fix build",
    });
    assert_eq!(a.agent_session_id(&body).as_deref(), Some("ses_abc"));
    let u = a.parse("PreToolUse", &body).unwrap();
    assert_eq!(u.state, Some(gateterm_protocol::AgentState::Working));
    assert_eq!(u.detail.as_deref(), Some("bash"));
    assert_eq!(u.title.as_deref(), Some("Fix build"));

    assert!(a.is_blocking("PermissionRequest"));
    let allow = a.render(&Decision::Allow {
        message: None,
        updated_input: None,
    });
    assert_eq!(
        allow["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );
}

#[test]
fn transcript_and_history_are_parsed_and_located() {
    let e = env();
    let sessions_dir = e
        .home
        .path()
        .join(".local/share/opencode/gateterm-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let path = sessions_dir.join("ses_demo1.jsonl");
    let content = concat!(
        r#"{"type":"session","id":"ses_demo1","cwd":"/work/app","title":"OpenCode Task","model":"claude-sonnet-4","timestamp":1710000000000}"#, "\n",
        r#"{"id":"m1","type":"message","message":{"role":"user","timestamp":1710000001000,"content":[{"type":"text","text":"run tests"}]}}"#, "\n",
        r#"{"id":"m2","type":"message","message":{"role":"assistant","timestamp":1710000002000,"content":[{"type":"thinking","thinking":"running cargo test"},{"type":"toolCall","name":"bash","arguments":{"command":"cargo test"}},{"type":"text","text":"all green"}]}}"#, "\n",
    );
    fs::write(&path, content).unwrap();

    let a = find("opencode").unwrap();
    assert_eq!(a.locate_transcript(e.home.path(), "ses_demo1"), Some(path.clone()));
    let msgs = a.parse_transcript(content, 0);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(
        msgs[1].blocks,
        vec![
            Block::Thinking {
                text: "running cargo test".into()
            },
            Block::ToolUse {
                name: "bash".into(),
                input: "cargo test".into()
            },
            Block::Text {
                text: "all green".into()
            },
        ]
    );

    let mut meta = history::RowMeta::default();
    for line in content.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        a.inspect_history_row(&row, &mut meta);
    }
    a.finalize_history_entry(&path, &mut meta);
    assert_eq!(meta.id.as_deref(), Some("ses_demo1"));
    assert_eq!(meta.title.as_deref(), Some("OpenCode Task"));
    assert_eq!(meta.cwd.as_deref(), Some("/work/app"));
    assert_eq!(meta.model.as_deref(), Some("claude-sonnet-4"));
}

#[tokio::test]
async fn blocking_permission_request_roundtrips_over_http() {
    let addr = common::start_test_server_at("tok", 1 << 16, Default::default(), vec![]).await;
    let client = reqwest::Client::new();
    let hook_fut = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .post(format!("http://{addr}/hook/opencode/permission-request"))
                .header("Authorization", format!("Bearer {}", hook_token()))
                .header("X-Gateterm-Session", "5")
                .body(
                    json!({
                        "session_id": "ses_1",
                        "cwd": "/w",
                        "tool_name": "bash",
                        "tool_input": { "command": "rm -rf dist" }
                    })
                    .to_string(),
                )
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    });

    let pending_id = loop {
        let p: Value = client
            .get(format!("http://{addr}/api/agents/pending"))
            .header("Authorization", "Bearer tok")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(id) = p["pending"].get(0).and_then(|x| x["id"].as_str()) {
            assert_eq!(p["pending"][0]["tool_name"], "bash");
            break id.to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };

    let ans = client
        .post(format!("http://{addr}/api/agents/answer"))
        .header("Authorization", "Bearer tok")
        .json(&json!({ "pending_id": pending_id, "decision": "allow" }))
        .send()
        .await
        .unwrap();
    assert_eq!(ans.status(), 200);

    let reply = hook_fut.await.unwrap();
    assert_eq!(
        reply["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );
}
