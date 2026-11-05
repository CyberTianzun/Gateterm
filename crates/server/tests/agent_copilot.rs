//! GitHub Copilot CLI (`copilot`) adapter 集成测试:
//! 钩子托管、跨文件冲突检测、驼峰 payload、阻塞式权限审批、对话记录与历史会话发现。

mod common;

use gateterm_server::agent::apply::{apply, ApplyError, ApplyOpts};
use gateterm_server::agent::copilot::hooks_source;
use gateterm_server::agent::detect::DetectEnv;
use gateterm_server::agent::edit::InstallCtx;
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
    fn hooks(&self) -> PathBuf {
        self.home.path().join(".copilot/hooks/gateterm.json")
    }
    fn ctx(&self, port: u16, include_blocking: bool) -> InstallCtx {
        InstallCtx {
            home: self.home.path().to_path_buf(),
            port,
            include_blocking,
        }
    }
    fn opts(&self) -> ApplyOpts {
        ApplyOpts {
            enabled: true,
            backup_dir: self.backups.path().to_path_buf(),
            backup_keep: 5,
        }
    }
    fn install(&self, port: u16, include_blocking: bool) -> Result<(), ApplyError> {
        apply(
            &find("copilot")
                .unwrap()
                .plan_install(&self.ctx(port, include_blocking))
                .unwrap(),
            &self.opts(),
        )
        .map(|_| ())
    }
    fn uninstall(&self) {
        apply(
            &find("copilot")
                .unwrap()
                .plan_uninstall(&self.ctx(7683, true))
                .unwrap(),
            &self.opts(),
        )
        .unwrap();
    }
    fn purge(&self) {
        let a = find("copilot").unwrap();
        apply(&plan_purge(a, &self.ctx(0, true)).unwrap(), &self.opts()).unwrap();
    }
    fn detect_env(&self) -> DetectEnv {
        DetectEnv {
            home: self.home.path().to_path_buf(),
            path: None,
        }
    }
    fn integration(&self, port: u16) -> (Integration, Vec<String>) {
        let st = scan(&self.detect_env(), port)
            .into_iter()
            .find(|a| a.id == "copilot")
            .unwrap();
        (st.integration, st.conflicts)
    }
}

#[test]
fn install_writes_gateterm_json_and_uninstall_removes_it() {
    let e = env();
    assert_eq!(e.integration(7683).0, Integration::NotInstalled);
    e.install(7683, true).unwrap();
    assert_eq!(
        fs::read_to_string(e.hooks()).unwrap(),
        hooks_source(7683, true)
    );
    assert_eq!(e.integration(7683).0, Integration::Installed);
    e.install(7683, true).unwrap(); // 幂等
    e.uninstall();
    assert!(!e.hooks().exists());
    assert_eq!(e.integration(7683).0, Integration::NotInstalled);

    e.install(9000, true).unwrap();
    assert_eq!(e.integration(7683).0, Integration::StalePort);
    e.purge();
    assert!(!e.hooks().exists());
}

#[test]
fn sibling_hook_file_with_permission_request_is_reported_as_conflict() {
    let e = env();
    let hooks_dir = e.home.path().join(".copilot/hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    fs::write(
        hooks_dir.join("orca.json"),
        r#"{"version":1,"hooks":{"permissionRequest":[{"type":"command","command":"orca.ps1"}]}}"#,
    )
    .unwrap();

    let (state, conflicts) = e.integration(7683);
    assert_eq!(state, Integration::NotInstalled);
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].contains("orca.json"));

    // 有冲突时按 include_blocking=false 安装,scan 仍应判定为 Installed 而非 Outdated
    e.install(7683, false).unwrap();
    assert_eq!(e.integration(7683).0, Integration::Installed);
}

#[test]
fn config_json_is_evidence_and_hooks_dir_alone_is_not() {
    let e = env();
    let dir = e.home.path().join(".copilot");
    fs::create_dir_all(dir.join("hooks")).unwrap();
    let a = find("copilot").unwrap();
    assert!(!a.detect(&e.detect_env()).installed);

    fs::write(dir.join("config.json"), "{}").unwrap();
    let d = a.detect(&e.detect_env());
    assert_eq!(
        (d.installed, d.confidence, d.reason),
        (true, Confidence::High, "config-file")
    );
}

#[test]
fn parses_copilot_camel_case_events_and_renders_decisions() {
    let a = find("copilot").unwrap();
    let body = json!({
        "sessionId": "0bd6225d",
        "cwd": "D:\\repo",
        "toolName": "powershell",
        "transcriptPath": "D:\\events.jsonl",
    });
    assert_eq!(a.agent_session_id(&body).as_deref(), Some("0bd6225d"));
    let u = a.parse("PreToolUse", &body).unwrap();
    assert_eq!(u.state, Some(gateterm_protocol::AgentState::Working));
    assert_eq!(u.detail.as_deref(), Some("powershell"));
    assert_eq!(u.transcript.as_deref(), Some("D:\\events.jsonl"));

    let recoverable = a
        .parse("ErrorOccurred", &json!({ "recoverable": true }))
        .unwrap();
    assert_eq!(
        recoverable.state,
        Some(gateterm_protocol::AgentState::Working)
    );
    let fatal = a
        .parse("ErrorOccurred", &json!({ "recoverable": false, "error": "boom" }))
        .unwrap();
    assert_eq!(fatal.state, Some(gateterm_protocol::AgentState::Error));

    assert!(a.is_blocking("PermissionRequest"));
    let deny = a.render(&Decision::Deny {
        message: Some("no".into()),
    });
    assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
}

#[test]
fn transcript_and_workspace_yaml_are_parsed_and_located() {
    let e = env();
    let sdir = e
        .home
        .path()
        .join(".copilot/session-state/0bd6225d-12e2-4e6d-aef7-c11dd0753a2e");
    fs::create_dir_all(&sdir).unwrap();
    fs::write(
        sdir.join("workspace.yaml"),
        "id: 0bd6225d-12e2-4e6d-aef7-c11dd0753a2e\ncwd: D:\\online\\Octoterm\nname: \"Copilot Feature\"\nuser_named: true\n",
    )
    .unwrap();
    let events_path = sdir.join("events.jsonl");
    let events = concat!(
        r#"{"type":"session.start","data":{"sessionId":"0bd6225d-12e2-4e6d-aef7-c11dd0753a2e","selectedModel":"gpt-5","context":{"cwd":"D:\\online\\Octoterm"}},"id":"e0","timestamp":"2026-07-31T08:13:02.078Z"}"#, "\n",
        r#"{"type":"user.message","data":{"content":"check status"},"id":"e1","timestamp":"2026-07-31T08:13:18.610Z"}"#, "\n",
        r#"{"type":"assistant.message","data":{"content":"running git status","reasoningText":"need repo state","toolRequests":[{"toolCallId":"c1","name":"powershell","arguments":{"command":"git status"}}]},"id":"e2","timestamp":"2026-07-31T08:13:34.151Z"}"#, "\n",
        r#"{"type":"tool.execution_complete","data":{"toolCallId":"c1","success":true,"result":{"content":"clean"}},"id":"e3","timestamp":"2026-07-31T08:13:35.000Z"}"#, "\n",
    );
    fs::write(&events_path, events).unwrap();

    let a = find("copilot").unwrap();
    assert_eq!(
        a.locate_transcript(e.home.path(), "0bd6225d-12e2-4e6d-aef7-c11dd0753a2e"),
        Some(events_path)
    );
    let msgs = a.parse_transcript(events, 0);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(
        msgs[1].blocks,
        vec![
            Block::Thinking {
                text: "need repo state".into()
            },
            Block::Text {
                text: "running git status".into()
            },
            Block::ToolUse {
                name: "powershell".into(),
                input: "git status".into()
            },
        ]
    );
    assert_eq!(
        msgs[2].blocks,
        vec![Block::ToolResult {
            ok: true,
            text: "clean".into()
        }]
    );
}

#[tokio::test]
async fn camel_case_blocking_permission_request_roundtrips_over_http() {
    let addr = common::start_test_server_at("tok", 1 << 16, Default::default(), vec![]).await;
    let client = reqwest::Client::new();
    let hook_fut = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .post(format!("http://{addr}/hook/copilot/permission-request"))
                .header("Authorization", format!("Bearer {}", hook_token()))
                .header("X-Gateterm-Session", "7")
                .body(
                    json!({
                        "sessionId": "cop-1",
                        "cwd": "D:\\repo",
                        "toolName": "powershell",
                        "toolInput": { "command": "git push" }
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
            assert_eq!(p["pending"][0]["tool_name"], "powershell");
            assert_eq!(p["pending"][0]["tool_input"]["command"], "git push");
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
