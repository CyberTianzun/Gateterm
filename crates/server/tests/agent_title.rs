mod common;

use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use gateterm_protocol::{ClientMsg, ServerMsg, SessionEventKind};
use gateterm_server::agent::store::hook_token;
use gateterm_server::agent::title::{SESSION_NAME_MAX_COLS, display_width};
use gateterm_server::config::AgentsConfig;

fn osc_echo_cmd() -> Option<Vec<String>> {
    #[cfg(unix)]
    return Some(vec![
        "/bin/sh".into(),
        "-c".into(),
        r#"stty -echo 2>/dev/null || true; printf 'OSC_READY\n'; while IFS= read -r line; do line=$(printf '%s' "$line" | tr -d '\r'); case "$line" in OSC:*) b=${line#OSC:}; title=$(printf '%s' "$b" | base64 -d 2>/dev/null || printf '%s' "$b" | base64 -D); printf '\033]2;%s\007ACK_%s\n' "$title" "$b";; esac; done"#
            .into(),
    ]);
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        Some(vec![
            format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"),
            "-NoProfile".into(),
            "-NoLogo".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
             [Console]::Out.Write(\"OSC_READY`n\"); [Console]::Out.Flush(); \
             while (($line = [Console]::In.ReadLine()) -ne $null) { \
               $idx = $line.LastIndexOf('OSC:'); \
               if ($idx -ge 0) { \
                 $b = $line.Substring($idx + 4).Trim(); \
                 $t = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($b)); \
                 [Console]::Out.Write([char]27 + ']2;' + $t + [char]7 + 'ACK_' + $b + \"`n\"); \
                 [Console]::Out.Flush() \
               } \
             }"
                .into(),
        ])
    }
}

struct WsHarness {
    ws: common::Ws,
    renamed: Vec<(u64, String)>,
    data_acc: Vec<u8>,
}

impl WsHarness {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let ws = common::connect(&format!("ws://{addr}/ws")).await;
        Self {
            ws,
            renamed: Vec::new(),
            data_acc: Vec::new(),
        }
    }

    fn record_msg(&mut self, msg: ServerMsg) {
        if let ServerMsg::SessionEvent {
            event: SessionEventKind::Renamed,
            session,
        } = msg
        {
            self.renamed.push((session.id, session.name));
        }
    }

    async fn create_osc_session(&mut self, initial_name: &str) -> u64 {
        self.ws
            .send(common::control(&ClientMsg::NewSession {
                name: Some(initial_name.into()),
                command: osc_echo_cmd(),
                cwd: None,
            }))
            .await
            .unwrap();
        let id = loop {
            match common::next_control(&mut self.ws).await {
                ServerMsg::SessionEvent {
                    event: SessionEventKind::Created,
                    session,
                } => {
                    assert_eq!(session.name, initial_name);
                    break session.id;
                }
                other => self.record_msg(other),
            }
        };

        self.ws
            .send(common::control(&ClientMsg::Attach {
                id,
                channel: 1,
                last_seq: None,
                cols: 80,
                rows: 24,
                colors: None,
            }))
            .await
            .unwrap();

        let mut saw_resync_end = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            if saw_resync_end && String::from_utf8_lossy(&self.data_acc).contains("OSC_READY") {
                return id;
            }
            let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_millis(250), self.ws.next()).await
            else {
                continue;
            };
            match common::parse_server(frame) {
                Some((0, Ok(ServerMsg::ResyncEnd { channel: 1, .. }))) => {
                    saw_resync_end = true;
                }
                Some((0, Ok(msg))) => self.record_msg(msg),
                Some((1, Err(bytes))) => self.data_acc.extend_from_slice(&bytes),
                _ => {}
            }
        }
        panic!(
            "timed out waiting for OSC_READY (resync_end={saw_resync_end}); data={:?}",
            String::from_utf8_lossy(&self.data_acc)
        );
    }

    async fn emit_osc(&mut self, title: &str) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(title.as_bytes());
        let line = format!("OSC:{encoded}\r");
        let ack = format!("ACK_{encoded}");
        self.ws
            .send(common::input_frame(1, line.as_bytes()))
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            if String::from_utf8_lossy(&self.data_acc).contains(&ack) {
                return;
            }
            let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_millis(200), self.ws.next()).await
            else {
                continue;
            };
            match common::parse_server(frame) {
                Some((0, Ok(msg))) => self.record_msg(msg),
                Some((1, Err(bytes))) => self.data_acc.extend_from_slice(&bytes),
                _ => {}
            }
        }
        panic!(
            "timed out waiting for {ack:?}; data={:?}",
            String::from_utf8_lossy(&self.data_acc)
        );
    }

    async fn wait_renamed(&mut self, id: u64, expected: &str) {
        if self
            .renamed
            .iter()
            .any(|(sid, name)| *sid == id && name == expected)
        {
            return;
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_millis(100), self.ws.next()).await
            else {
                continue;
            };
            match common::parse_server(frame) {
                Some((0, Ok(msg))) => {
                    self.record_msg(msg);
                    if self
                        .renamed
                        .iter()
                        .any(|(sid, name)| *sid == id && name == expected)
                    {
                        return;
                    }
                }
                Some((1, Err(bytes))) => self.data_acc.extend_from_slice(&bytes),
                _ => {}
            }
        }
        panic!(
            "timed out waiting for Renamed({id}, {expected:?}); seen={:?}",
            self.renamed
        );
    }

    async fn wait_renamed_matching<F>(&mut self, id: u64, pred: F) -> String
    where
        F: Fn(&str) -> bool,
    {
        if let Some((_, name)) = self
            .renamed
            .iter()
            .find(|(sid, name)| *sid == id && pred(name))
        {
            return name.clone();
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_millis(100), self.ws.next()).await
            else {
                continue;
            };
            match common::parse_server(frame) {
                Some((0, Ok(msg))) => {
                    self.record_msg(msg);
                    if let Some((_, name)) = self
                        .renamed
                        .iter()
                        .find(|(sid, name)| *sid == id && pred(name))
                    {
                        return name.clone();
                    }
                }
                Some((1, Err(bytes))) => self.data_acc.extend_from_slice(&bytes),
                _ => {}
            }
        }
        panic!("timed out waiting for matching Renamed on {id}; seen={:?}", self.renamed);
    }

    async fn session_name(&mut self, id: u64) -> String {
        self.ws
            .send(common::control(&ClientMsg::ListSessions))
            .await
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_millis(100), self.ws.next()).await
            else {
                continue;
            };
            match common::parse_server(frame) {
                Some((0, Ok(ServerMsg::Sessions { sessions }))) => {
                    return sessions
                        .into_iter()
                        .find(|s| s.id == id)
                        .expect("session should exist")
                        .name;
                }
                Some((0, Ok(msg))) => self.record_msg(msg),
                Some((1, Err(bytes))) => self.data_acc.extend_from_slice(&bytes),
                _ => {}
            }
        }
        panic!("timed out waiting for Sessions list");
    }

    async fn rename_session(&mut self, id: u64, name: &str) {
        self.ws
            .send(common::control(&ClientMsg::RenameSession {
                id,
                name: name.into(),
            }))
            .await
            .unwrap();
    }

    async fn kill_session(&mut self, id: u64) {
        self.ws
            .send(common::control(&ClientMsg::KillSession { id }))
            .await
            .unwrap();
    }
}

async fn post_hook(
    addr: std::net::SocketAddr,
    path: &str,
    session_id: u64,
    body: serde_json::Value,
) {
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .header("Authorization", format!("Bearer {}", hook_token()))
        .header("X-Gateterm-Session", session_id.to_string())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

async fn agent_sessions(addr: std::net::SocketAddr) -> Vec<serde_json::Value> {
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("http://{addr}/api/agents/sessions"))
        .header("Authorization", "Bearer t")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body["sessions"].as_array().unwrap().clone()
}

fn find_agent_session<'a>(
    sessions: &'a [serde_json::Value],
    agent_id: &str,
    agent_session_id: &str,
) -> &'a serde_json::Value {
    sessions
        .iter()
        .find(|s| s["agent_id"] == agent_id && s["agent_session_id"] == agent_session_id)
        .unwrap_or_else(|| panic!("missing agent session ({agent_id}, {agent_session_id}) in {sessions:?}"))
}

#[tokio::test]
async fn hook_prompt_then_osc_then_explicit_summary_upgrades_and_turn2_prompt_is_ignored() {
    let addr = common::start_test_server_at("t", 1 << 20, Default::default(), vec![]).await;
    let mut ws = WsHarness::connect(addr).await;
    let id = ws.create_osc_session("osc-shell").await;

    let prompt = "Implement remote clipboard sync across browsers";
    post_hook(
        addr,
        "/hook/claude-code/user-prompt-submit",
        id,
        serde_json::json!({
            "session_id": "s1",
            "prompt": prompt,
        }),
    )
    .await;

    let concise = ws
        .wait_renamed_matching(id, |name| name != "osc-shell")
        .await;
    assert!(display_width(&concise) <= SESSION_NAME_MAX_COLS, "{concise:?}");
    assert_eq!(ws.session_name(id).await, concise);
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s1")["title"].as_str(),
        Some(prompt)
    );

    // Turn 2 UserPromptSubmit must NOT overwrite write-once FirstPrompt.
    post_hook(
        addr,
        "/hook/claude-code/user-prompt-submit",
        id,
        serde_json::json!({
            "session_id": "s1",
            "prompt": "yes",
        }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(ws.session_name(id).await, concise);
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s1")["title"].as_str(),
        Some(prompt)
    );

    // PreToolUse without session_title must NOT promote FirstPrompt to ExplicitSummary.
    post_hook(
        addr,
        "/hook/claude-code/pre-tool-use",
        id,
        serde_json::json!({
            "session_id": "s1",
            "tool_name": "Bash",
        }),
    )
    .await;

    // OSC 0/2 upgrades FirstPrompt -> OscTitle and emits SessionEventKind::Renamed.
    ws.emit_osc("✳ Claude Code: Clipboard Sync").await;
    ws.wait_renamed(id, "Clipboard Sync").await;
    assert_eq!(ws.session_name(id).await, "Clipboard Sync");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s1")["title"].as_str(),
        Some("Clipboard Sync")
    );

    // Explicit hook session_title upgrades OscTitle -> ExplicitSummary.
    post_hook(
        addr,
        "/hook/claude-code/stop",
        id,
        serde_json::json!({
            "session_id": "s1",
            "session_title": "Remote Clipboard Architecture",
        }),
    )
    .await;
    ws.wait_renamed(id, "Remote Clipboard Architecture").await;
    assert_eq!(ws.session_name(id).await, "Remote Clipboard Architecture");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s1")["title"].as_str(),
        Some("Remote Clipboard Architecture")
    );

    // Subsequent lower-rank OSC title must NOT downgrade ExplicitSummary.
    ws.emit_osc("Lower Rank Title").await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(ws.session_name(id).await, "Remote Clipboard Architecture");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s1")["title"].as_str(),
        Some("Remote Clipboard Architecture")
    );

    ws.kill_session(id).await;
}

#[tokio::test]
async fn manual_rename_locks_session_name_while_agent_title_still_updates() {
    let addr = common::start_test_server_at("t", 1 << 20, Default::default(), vec![]).await;
    let mut ws = WsHarness::connect(addr).await;
    let id = ws.create_osc_session("osc-shell").await;

    ws.rename_session(id, "My Custom Tab").await;
    ws.wait_renamed(id, "My Custom Tab").await;

    post_hook(
        addr,
        "/hook/claude-code/stop",
        id,
        serde_json::json!({
            "session_id": "s-lock",
            "session_title": "Auto Title From Agent",
        }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(ws.session_name(id).await, "My Custom Tab");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s-lock")["title"].as_str(),
        Some("Auto Title From Agent")
    );

    // Lower-rank OSC must neither unlock SessionInfo.name nor downgrade AgentSession.title.
    ws.emit_osc("Lower Rank Locked OSC").await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(ws.session_name(id).await, "My Custom Tab");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s-lock")["title"].as_str(),
        Some("Auto Title From Agent")
    );

    ws.kill_session(id).await;
}

#[tokio::test]
async fn plain_shell_osc_ignored_and_stale_osc_after_clear_does_not_poison_new_agent() {
    let addr = common::start_test_server_at("t", 1 << 20, Default::default(), vec![]).await;
    let mut ws = WsHarness::connect(addr).await;
    let id = ws.create_osc_session("osc-shell").await;

    // Plain shell with no active agent emits OSC -> ignored.
    ws.emit_osc("vim main.rs").await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(ws.session_name(id).await, "osc-shell");

    // Agent A starts and emits OSC "Task Alpha" -> renamed to "Task Alpha".
    post_hook(
        addr,
        "/hook/claude-code/session-start",
        id,
        serde_json::json!({ "session_id": "agent-a" }),
    )
    .await;
    ws.emit_osc("Task Alpha").await;
    ws.wait_renamed(id, "Task Alpha").await;
    assert_eq!(ws.session_name(id).await, "Task Alpha");

    // Agent B starts in the same terminal (simulating /clear) and gets UserPromptSubmit.
    post_hook(
        addr,
        "/hook/claude-code/session-start",
        id,
        serde_json::json!({ "session_id": "agent-b" }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    post_hook(
        addr,
        "/hook/claude-code/user-prompt-submit",
        id,
        serde_json::json!({
            "session_id": "agent-b",
            "prompt": "Task Beta",
        }),
    )
    .await;
    ws.wait_renamed(id, "Task Beta").await;
    assert_eq!(ws.session_name(id).await, "Task Beta");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "agent-b")["title"].as_str(),
        Some("Task Beta")
    );

    ws.kill_session(id).await;
}

#[tokio::test]
async fn transcript_enabled_false_skips_first_prompt_and_disk_summary_but_allows_hook_title_and_osc() {
    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("s-priv.jsonl");
    std::fs::write(
        &transcript,
        "{\"sessionId\":\"s-priv\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Secret user prompt\"}}\n\
         {\"type\":\"custom-title\",\"customTitle\":\"Secret Disk Title\"}\n",
    )
    .unwrap();

    let agents_cfg = AgentsConfig {
        transcript_enabled: false,
        ..Default::default()
    };
    let addr = common::start_test_server_with_agents(
        "t",
        1 << 20,
        Default::default(),
        vec![],
        agents_cfg,
    )
    .await;
    let mut ws = WsHarness::connect(addr).await;
    let id = ws.create_osc_session("osc-shell").await;

    post_hook(
        addr,
        "/hook/claude-code/user-prompt-submit",
        id,
        serde_json::json!({
            "session_id": "s-priv",
            "prompt": "Secret user prompt",
            "transcript_path": transcript,
        }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(ws.session_name(id).await, "osc-shell");
    let agents = agent_sessions(addr).await;
    assert!(
        find_agent_session(&agents, "claude-code", "s-priv")["title"].is_null(),
        "transcript_enabled=false must leave AgentSession.title as None: {agents:?}"
    );

    ws.emit_osc("✳ Safe Terminal Title").await;
    ws.wait_renamed(id, "Safe Terminal Title").await;
    assert_eq!(ws.session_name(id).await, "Safe Terminal Title");

    post_hook(
        addr,
        "/hook/claude-code/stop",
        id,
        serde_json::json!({
            "session_id": "s-priv",
            "session_title": "Safe Hook Title",
        }),
    )
    .await;
    ws.wait_renamed(id, "Safe Hook Title").await;
    assert_eq!(ws.session_name(id).await, "Safe Hook Title");

    ws.kill_session(id).await;
}

#[tokio::test]
async fn multi_agent_terminal_attributes_osc_to_most_recent_agent() {
    let addr = common::start_test_server_at("t", 1 << 20, Default::default(), vec![]).await;
    let mut ws = WsHarness::connect(addr).await;
    let id = ws.create_osc_session("osc-shell").await;

    // Agent A enters Error state on session `id`.
    post_hook(
        addr,
        "/hook/opencode/error-occurred",
        id,
        serde_json::json!({
            "session_id": "agent-a",
            "error": "boom",
        }),
    )
    .await;

    // Agent B subsequently sends SessionStart (Idle) on the same session `id`.
    post_hook(
        addr,
        "/hook/claude-code/session-start",
        id,
        serde_json::json!({
            "session_id": "agent-b",
        }),
    )
    .await;

    ws.emit_osc("Agent B Task").await;
    ws.wait_renamed(id, "Agent B Task").await;
    assert_eq!(ws.session_name(id).await, "Agent B Task");

    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "agent-b")["title"].as_str(),
        Some("Agent B Task")
    );
    assert!(
        find_agent_session(&agents, "opencode", "agent-a")["title"].is_null(),
        "Agent A must not receive Agent B's OSC title: {agents:?}"
    );

    ws.kill_session(id).await;
}

#[tokio::test]
async fn resolve_live_title_upgrades_from_transcript_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("s-live.jsonl");
    std::fs::write(
        &transcript,
        "{\"sessionId\":\"s-live\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Initial prompt\"}}\n",
    )
    .unwrap();

    let addr = common::start_test_server_at("t", 1 << 20, Default::default(), vec![]).await;
    let mut ws = WsHarness::connect(addr).await;
    let id = ws.create_osc_session("osc-shell").await;

    post_hook(
        addr,
        "/hook/claude-code/user-prompt-submit",
        id,
        serde_json::json!({
            "session_id": "s-live",
            "prompt": "Initial prompt",
            "transcript_path": transcript,
        }),
    )
    .await;
    ws.wait_renamed(id, "Initial prompt").await;

    // Now write an explicit custom-title to disk and send Stop without session_title.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    writeln!(
        f,
        "{{\"type\":\"custom-title\",\"customTitle\":\"✳ Claude Code: Disk Custom Summary\"}}"
    )
    .unwrap();
    f.flush().unwrap();

    post_hook(
        addr,
        "/hook/claude-code/stop",
        id,
        serde_json::json!({
            "session_id": "s-live",
            "transcript_path": transcript,
        }),
    )
    .await;

    ws.wait_renamed(id, "Disk Custom Summary").await;
    assert_eq!(ws.session_name(id).await, "Disk Custom Summary");
    let agents = agent_sessions(addr).await;
    assert_eq!(
        find_agent_session(&agents, "claude-code", "s-live")["title"].as_str(),
        Some("Disk Custom Summary")
    );

    ws.kill_session(id).await;
}
