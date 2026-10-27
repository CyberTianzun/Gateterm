//! Grok 的 `updates.jsonl` 解析。夹具是 grok 1.0.52 真实跑出来的一次会话(思考、正文、
//! 一次成功的 shell、一次失败的 read_file),只把临时目录路径换成了 `/tmp/grokprobe`。

use gateterm_server::agent::find;
use gateterm_server::agent::grok_transcript::{locate, parse};
use gateterm_server::agent::transcript::{Block, Role};
use std::fs;

const FIXTURE: &str = include_str!("fixtures/grok-updates.jsonl");

#[test]
fn real_session_parses_into_conversation_order() {
    let msgs = parse(FIXTURE, 0);
    let kinds: Vec<(Role, &str)> = msgs
        .iter()
        .map(|m| {
            let k = match &m.blocks[0] {
                Block::Text { .. } => "text",
                Block::Thinking { .. } => "thinking",
                Block::ToolUse { .. } => "tool-use",
                Block::ToolResult { .. } => "tool-result",
            };
            (m.role, k)
        })
        .collect();
    assert_eq!(
        kinds,
        [
            (Role::User, "text"),
            (Role::Assistant, "thinking"),
            (Role::Assistant, "text"),
            (Role::Assistant, "tool-use"),
            (Role::Assistant, "tool-use"),
            (Role::User, "tool-result"),
            (Role::User, "tool-result"),
            (Role::Assistant, "text"),
        ],
        "hook_execution / turn_completed / 不带 status 的 tool_call_update 都不该出现"
    );
}

#[test]
fn tools_and_results_carry_the_useful_bits() {
    let msgs = parse(FIXTURE, 0);
    let uses: Vec<_> = msgs
        .iter()
        .filter_map(|m| match &m.blocks[0] {
            Block::ToolUse { name, input } => Some((name.as_str(), input.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(uses, [("run_terminal_command", "echo gateterm-probe"), ("read_file", "probe.txt")]);

    let results: Vec<_> = msgs
        .iter()
        .filter_map(|m| match &m.blocks[0] {
            Block::ToolResult { ok, text } => Some((*ok, text.clone())),
            _ => None,
        })
        .collect();
    assert!(!results[0].0 && results[0].1.contains("does not exist"), "read_file 失败要标成失败");
    assert!(results[1].0 && results[1].1.trim() == "gateterm-probe");
}

/// id 取 `_meta.eventId`:换个窗口起点再读,同一条还是同一个 id —— 客户端去重靠它。
#[test]
fn ids_are_stable_and_timestamps_kept() {
    let all = parse(FIXTURE, 0);
    let cut = FIXTURE.find("\n").unwrap() + 1;
    let tail = parse(&FIXTURE[cut..], cut as u64);
    assert_eq!(all.last().unwrap().id, tail.last().unwrap().id);
    assert!(all.iter().all(|m| m.id.contains('-') && m.ts.is_some()));
    let mut ids: Vec<_> = all.iter().map(|m| m.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), all.len());
}

#[test]
fn locate_finds_the_session_under_any_cwd_group() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".grok/sessions/%2Fsome%2Fcwd/sid-1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("updates.jsonl"), "").unwrap();
    assert_eq!(locate(home.path(), "sid-1"), Some(dir.join("updates.jsonl")));
    assert_eq!(locate(home.path(), "nope"), None);
    assert_eq!(locate(home.path(), "../sessions"), None, "会话 id 不许跳出目录");
    // adapter 走的是同一条
    assert!(find("grok").unwrap().locate_transcript(home.path(), "sid-1").is_some());
}
