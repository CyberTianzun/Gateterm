//! pi adapter:整文件托管的扩展、状态判定、会话文件解析。
//!
//! 全部用 tempfile 造假 home,不碰真实的 `~/.pi`。

use gateterm_server::agent::apply::{apply, ApplyError, ApplyOpts};
use gateterm_server::agent::detect::DetectEnv;
use gateterm_server::agent::edit::InstallCtx;
use gateterm_server::agent::pi::extension_source;
use gateterm_server::agent::transcript::{Block, Role};
use gateterm_server::agent::{find, plan_purge, scan, Integration};
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
    fn ext(&self) -> PathBuf {
        self.home.path().join(".pi/agent/extensions/gateterm.ts")
    }
    fn ctx(&self, port: u16) -> InstallCtx {
        InstallCtx { home: self.home.path().to_path_buf(), port, include_blocking: true }
    }
    fn opts(&self) -> ApplyOpts {
        ApplyOpts { enabled: true, backup_dir: self.backups.path().to_path_buf(), backup_keep: 5 }
    }
    fn install(&self, port: u16) -> Result<(), ApplyError> {
        apply(&find("pi").unwrap().plan_install(&self.ctx(port)).unwrap(), &self.opts()).map(|_| ())
    }
    fn uninstall(&self) {
        apply(&find("pi").unwrap().plan_uninstall(&self.ctx(7683)).unwrap(), &self.opts()).unwrap();
    }
    fn purge(&self) {
        let a = find("pi").unwrap();
        apply(&plan_purge(a.as_ref(), &self.ctx(0)).unwrap(), &self.opts()).unwrap();
    }
    fn integration(&self, port: u16) -> Integration {
        let e = DetectEnv { home: self.home.path().to_path_buf(), path: None };
        scan(&e, port).into_iter().find(|a| a.id == "pi").unwrap().integration
    }
}

#[test]
fn install_writes_the_extension_and_uninstall_removes_it() {
    let e = env();
    assert_eq!(e.integration(7683), Integration::NotInstalled);
    e.install(7683).unwrap();
    assert_eq!(fs::read_to_string(e.ext()).unwrap(), extension_source(7683));
    assert_eq!(e.integration(7683), Integration::Installed);
    e.install(7683).unwrap(); // 幂等
    e.uninstall();
    assert!(!e.ext().exists());
    assert_eq!(e.integration(7683), Integration::NotInstalled);
}

#[test]
fn port_change_is_stale_port_and_reinstall_fixes_it() {
    let e = env();
    e.install(9000).unwrap();
    assert_eq!(e.integration(7683), Integration::StalePort);
    e.install(7683).unwrap();
    assert_eq!(e.integration(7683), Integration::Installed);
}

/// 端口对、内容旧(gateterm 升级改了扩展):要报出来,重装后对齐。
#[test]
fn old_extension_content_is_outdated() {
    let e = env();
    e.install(7683).unwrap();
    let old = extension_source(7683).replace("tool_execution_end", "tool_execution_done");
    fs::write(e.ext(), old).unwrap();
    assert_eq!(e.integration(7683), Integration::Outdated);
}

/// 同名但不是我们写的文件:不覆盖、不删除、不清空。
#[test]
fn foreign_file_with_same_name_is_never_touched() {
    let e = env();
    fs::create_dir_all(e.ext().parent().unwrap()).unwrap();
    fs::write(e.ext(), "export default function () {}\n").unwrap();
    assert!(matches!(e.install(7683), Err(ApplyError::Invalid { .. })));
    e.uninstall();
    e.purge();
    assert_eq!(fs::read_to_string(e.ext()).unwrap(), "export default function () {}\n");
}

#[test]
fn purge_removes_our_extension_and_backs_it_up() {
    let e = env();
    e.install(1234).unwrap();
    e.purge();
    assert!(!e.ext().exists());
    assert_eq!(fs::read_dir(e.backups.path()).unwrap().count(), 1);
}

#[test]
fn uninstall_without_install_creates_nothing() {
    let e = env();
    e.uninstall();
    e.purge();
    assert!(!e.home.path().join(".pi").exists());
}

#[test]
fn hook_events_map_to_states() {
    let a = find("pi").unwrap();
    let body = serde_json::json!({
        "session_id": "s1", "cwd": "/w", "tool_name": "bash",
        "transcript_path": "/x.jsonl", "session_title": "",
    });
    let u = a.parse("PreToolUse", &body).unwrap();
    assert_eq!(u.detail.as_deref(), Some("bash"));
    assert_eq!(u.transcript.as_deref(), Some("/x.jsonl"));
    assert_eq!(u.title, None, "空标题不算标题");
    assert!(a.parse("SessionStart", &body).is_some());
    assert!(a.parse("Whatever", &body).is_none());
    assert!(!a.is_blocking("PermissionRequest"));
}

const SESSION: &str = r#"{"type":"session","version":3,"id":"u-1","timestamp":"2026-10-14T00:00:00.000Z","cwd":"/w"}
{"type":"model_change","id":"m0000001","parentId":null,"timestamp":"2026-10-14T00:00:00.000Z","provider":"anthropic","modelId":"x"}
{"type":"message","id":"a0000001","parentId":"m0000001","timestamp":"2026-10-14T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"列一下文件"}],"timestamp":1791936001000}}
{"type":"message","id":"a0000002","parentId":"a0000001","timestamp":"2026-10-14T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"用 ls"},{"type":"text","text":"好的"},{"type":"toolCall","id":"c1","name":"bash","arguments":{"command":"ls -la"}}],"stopReason":"toolUse","timestamp":1791936002000}}
{"type":"message","id":"a0000003","parentId":"a0000002","timestamp":"2026-10-14T00:00:03.000Z","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[{"type":"text","text":"a.txt"}],"isError":false,"timestamp":1791936003000}}
{"type":"message","id":"a0000004","parentId":"a0000003","timestamp":"2026-10-14T00:00:04.000Z","message":{"role":"bashExecution","command":"git status","output":"clean","exitCode":0,"cancelled":false,"truncated":false,"timestamp":1791936004000}}
{"type":"compaction","id":"a0000005","parentId":"a0000004","timestamp":"2026-10-14T00:00:05.000Z","summary":"之前聊了文件","firstKeptEntryId":"a0000003","tokensBefore":100}
{"type":"custom","id":"a0000006","parentId":"a0000005","timestamp":"2026-10-14T00:00:06.000Z","customType":"x","data":{}}
{"type":"message","id":"a0000007","parentId":"a0000006","timestamp":"2026-10-14T00:00:07.000Z","message":{"role":"toolResult","toolCallId":"c2","toolName":"read","content":[{"type":"text","text":"ENOENT"}],"isError":true,"timestamp":1791936007000}}
{"type":"message","id":"half
"#;

#[test]
fn transcript_is_normalized() {
    let msgs = find("pi").unwrap().parse_transcript(SESSION, 0);
    let ids: Vec<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
    // 头、model_change、custom、半行都跳过
    assert_eq!(ids, ["a0000001", "a0000002", "a0000003", "a0000004", "a0000005", "a0000007"]);

    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(msgs[0].ts, Some(1791936001));
    assert_eq!(msgs[0].blocks, [Block::Text { text: "列一下文件".into() }]);

    assert_eq!(msgs[1].role, Role::Assistant);
    assert_eq!(
        msgs[1].blocks,
        [
            Block::Thinking { text: "用 ls".into() },
            Block::Text { text: "好的".into() },
            Block::ToolUse { name: "bash".into(), input: "ls -la".into() },
        ]
    );
    assert_eq!(msgs[2].blocks, [Block::ToolResult { ok: true, text: "a.txt".into() }]);
    assert_eq!(
        msgs[3].blocks,
        [
            Block::ToolUse { name: "bash".into(), input: "git status".into() },
            Block::ToolResult { ok: true, text: "clean".into() },
        ]
    );
    assert_eq!(msgs[4].role, Role::System);
    assert_eq!(msgs[5].blocks, [Block::ToolResult { ok: false, text: "ENOENT".into() }]);
}

#[test]
fn transcript_is_located_by_session_id() {
    let e = env();
    let dir = e.home.path().join(".pi/agent/sessions/--Users-x-w--");
    fs::create_dir_all(&dir).unwrap();
    let f = dir.join("2026-10-14T00-00-00-000Z_0f0e-uuid.jsonl");
    fs::write(&f, "").unwrap();
    let a = find("pi").unwrap();
    assert_eq!(a.locate_transcript(e.home.path(), "0f0e-uuid"), Some(f));
    assert_eq!(a.locate_transcript(e.home.path(), "nope"), None);
}
