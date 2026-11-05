//! OpenCode 会话记录(`~/.local/share/opencode/gateterm-sessions/<session_id>.jsonl`
//! 或 `~/.config/opencode/sessions/<session_id>.jsonl`)→ 归一化消息。
//!
//! 同时兼容两种行格式:
//! 1. `gateterm.js` 插件实时镜像的 `type: "message"` 行(与 pi 消息结构同构);
//! 2. OpenCode 原生导出的 `{ info: { id, role, time }, parts: [...] }` 行。

use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::agent::transcript::{clamp_text, flatten_tool_input, offset_id, Block, Message, Role};

pub fn parse(text: &str, base: u64) -> Vec<Message> {
    let mut at = base;
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(m) = parse_line(line, at) {
            out.push(m);
        }
        at += line.len() as u64;
    }
    out
}

fn parse_line(line: &str, at: u64) -> Option<Message> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let row: Value = serde_json::from_str(line).ok()?;
    if let Some(kind) = row.get("type").and_then(Value::as_str) {
        return match kind {
            "message" => {
                let m = row.get("message")?;
                let (role, blocks) = parse_mirrored_message(m)?;
                if blocks.is_empty() {
                    return None;
                }
                let id = row
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| offset_id(at));
                let ts = m
                    .get("timestamp")
                    .and_then(Value::as_u64)
                    .map(|ms| if ms > 10_000_000_000 { ms / 1000 } else { ms });
                Some(Message { id, role, ts, blocks })
            }
            _ => None,
        };
    }

    // 兼容 OpenCode 导出的 `{ info: { id, role, time: { created } }, parts: [...] }`
    let info = row.get("info").unwrap_or(&row);
    let role_str = info.get("role").and_then(Value::as_str)?;
    let role = match role_str {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "system" => Role::System,
        _ => return None,
    };
    let parts = row.get("parts").and_then(Value::as_array)?;
    let mut blocks = Vec::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = non_empty(part.get("text")) {
                    blocks.push(Block::Text { text });
                }
            }
            Some("reasoning") => {
                if let Some(text) = non_empty(part.get("text").or_else(|| part.get("thinking"))) {
                    blocks.push(Block::Thinking { text });
                }
            }
            Some("tool") => {
                let name = str_of(part, "tool");
                let state = part.get("state").unwrap_or(&Value::Null);
                let input = state.get("input").unwrap_or(&Value::Null);
                blocks.push(Block::ToolUse {
                    name,
                    input: flatten_tool_input(input),
                });
                if let Some(output) = state.get("output").and_then(Value::as_str) {
                    blocks.push(Block::ToolResult {
                        ok: state.get("status").and_then(Value::as_str) != Some("error"),
                        text: clamp_text(output.to_string()),
                    });
                }
            }
            _ => {}
        }
    }
    if blocks.is_empty() {
        return None;
    }
    let id = info
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| offset_id(at));
    let ts = info
        .pointer("/time/created")
        .and_then(Value::as_u64)
        .map(|ms| if ms > 10_000_000_000 { ms / 1000 } else { ms });
    Some(Message { id, role, ts, blocks })
}

fn parse_mirrored_message(m: &Value) -> Option<(Role, Vec<Block>)> {
    Some(match m.get("role").and_then(Value::as_str)? {
        "user" => (Role::User, texts(m.get("content"))),
        "system" => (Role::System, texts(m.get("content"))),
        "assistant" => (
            Role::Assistant,
            m.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(assistant_block)
                .collect(),
        ),
        "toolResult" => (
            Role::User,
            vec![Block::ToolResult {
                ok: m.get("isError").and_then(Value::as_bool) != Some(true),
                text: clamp_text(join_texts(m.get("content"))),
            }],
        ),
        _ => return None,
    })
}

fn assistant_block(b: &Value) -> Option<Block> {
    match b.get("type").and_then(Value::as_str)? {
        "text" => non_empty(b.get("text")).map(|text| Block::Text { text }),
        "thinking" => non_empty(b.get("thinking")).map(|text| Block::Thinking { text }),
        "toolCall" => Some(Block::ToolUse {
            name: str_of(b, "name"),
            input: flatten_tool_input(b.get("arguments").unwrap_or(&Value::Null)),
        }),
        _ => None,
    }
}

fn non_empty(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(|s| clamp_text(s.to_string()))
}

fn texts(v: Option<&Value>) -> Vec<Block> {
    match v {
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![Block::Text { text: clamp_text(s.clone()) }]
            }
        }
        Some(Value::Array(items)) => items
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| non_empty(b.get("text")))
            .map(|text| Block::Text { text })
            .collect(),
        _ => Vec::new(),
    }
}

fn join_texts(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|x| x.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// 由会话 id 查找对应的对话记录文件。
pub fn locate(home: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains(['/', '\\']) || session_id.contains("..") {
        return None;
    }
    let file_name = format!("{session_id}.jsonl");
    let candidates = [
        home.join(".local")
            .join("share")
            .join("opencode")
            .join("gateterm-sessions")
            .join(&file_name),
        home.join(".config")
            .join("opencode")
            .join("sessions")
            .join(&file_name),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mirrored_and_native_opencode_messages() {
        let sample = concat!(
            r#"{"type":"session","id":"ses_123","cwd":"/repo","title":"Fix bug","timestamp":1710000000000}"#, "\n",
            r#"{"id":"msg_1","type":"message","message":{"role":"user","timestamp":1710000001000,"content":[{"type":"text","text":"hello opencode"}]}}"#, "\n",
            r#"{"id":"msg_2","type":"message","message":{"role":"assistant","timestamp":1710000002000,"content":[{"type":"thinking","thinking":"let me check"},{"type":"toolCall","name":"bash","arguments":{"command":"cargo test"}},{"type":"text","text":"done"}]}}"#, "\n",
            r#"{"id":"msg_3","type":"message","message":{"role":"toolResult","isError":false,"timestamp":1710000003000,"content":"ok"}}"#, "\n",
        );
        let msgs = parse(sample, 0);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[0].ts, Some(1_710_000_001));
        assert_eq!(msgs[1].role, Role::Assistant);
        assert_eq!(msgs[1].blocks.len(), 3);
        assert_eq!(msgs[2].role, Role::User);
    }

    #[test]
    fn locate_rejects_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(locate(tmp.path(), "../escape").is_none());
        assert!(locate(tmp.path(), "a/b").is_none());
    }
}
