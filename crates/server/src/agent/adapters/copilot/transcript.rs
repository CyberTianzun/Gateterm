//! GitHub Copilot CLI 的会话记录(`~/.copilot/session-state/<session-id>/events.jsonl`)
//! → 归一化消息。
//!
//! 实测 `events.jsonl` 每行是一个事件信封 `{type, data, id, timestamp, parentId}`:
//! - `user.message`:`data.content` 是用户输入正文;
//! - `assistant.message`:`data.reasoningText` 是思考过程,`data.content` 是回复正文,
//!   `data.toolRequests` 是本轮发起的工具调用列表(`{toolCallId, name, arguments}`);
//! - `tool.execution_complete`:工具执行结果,`data.success` 表示成败,输出在
//!   `data.result.content`(或 `data.error`)。

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
    let data = row.get("data")?;
    let (role, blocks) = match row.get("type").and_then(Value::as_str)? {
        "user.message" => {
            let content = data.get("content").and_then(Value::as_str).unwrap_or_default().trim();
            if content.is_empty() {
                return None;
            }
            (Role::User, vec![Block::Text { text: clamp_text(content.to_string()) }])
        }
        "assistant.message" => {
            let mut blocks = Vec::new();
            if let Some(thinking) = data
                .get("reasoningText")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                blocks.push(Block::Thinking {
                    text: clamp_text(thinking.to_string()),
                });
            }
            if let Some(content) = data
                .get("content")
                .and_then(Value::as_str)
                .map(strip_dsml_calls)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                blocks.push(Block::Text {
                    text: clamp_text(content.to_string()),
                });
            }
            for req in data
                .get("toolRequests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = req
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let args = req.get("arguments").unwrap_or(&Value::Null);
                let parsed_args = match args {
                    Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or_else(|_| args.clone()),
                    other => other.clone(),
                };
                blocks.push(Block::ToolUse {
                    name,
                    input: flatten_tool_input(&parsed_args),
                });
            }
            (Role::Assistant, blocks)
        }
        "tool.execution_complete" => {
            let ok = data.get("success").and_then(Value::as_bool) != Some(false);
            let text = data
                .pointer("/result/content")
                .and_then(Value::as_str)
                .or_else(|| data.pointer("/error/message").and_then(Value::as_str))
                .or_else(|| data.get("error").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            (
                Role::User,
                vec![Block::ToolResult {
                    ok,
                    text: clamp_text(text),
                }],
            )
        }
        _ => return None,
    };
    if blocks.is_empty() {
        return None;
    }
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| offset_id(at));
    let ts = row
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(unix_from_rfc3339);
    Some(Message { id, role, ts, blocks })
}

fn strip_dsml_calls(s: &str) -> &str {
    s.find("<｜DSML｜").map_or(s, |idx| &s[..idx])
}

fn unix_from_rfc3339(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.splitn(3, '-').map(str::parse::<i64>);
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let time = time.split('.').next()?;
    let mut t = time.splitn(3, ':').map(str::parse::<i64>);
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

pub fn locate(home: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains(['/', '\\']) || session_id.contains("..") {
        return None;
    }
    let path = home
        .join(".copilot")
        .join("session-state")
        .join(session_id)
        .join("events.jsonl");
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_copilot_events_jsonl() {
        let sample = concat!(
            r#"{"type":"session.start","data":{"sessionId":"0bd6225d","context":{"cwd":"D:\\repo"}},"id":"1","timestamp":"2026-07-31T08:13:02.078Z"}"#, "\n",
            r#"{"type":"user.message","data":{"content":"hello copilot"},"id":"msg-1","timestamp":"2026-07-31T08:13:18.610Z"}"#, "\n",
            r#"{"type":"assistant.message","data":{"content":"checking","reasoningText":"let me think","toolRequests":[{"toolCallId":"call_1","name":"powershell","arguments":{"command":"git status"}}]},"id":"msg-2","timestamp":"2026-07-31T08:13:34.151Z"}"#, "\n",
            r#"{"type":"tool.execution_complete","data":{"toolCallId":"call_1","success":true,"result":{"content":"On branch main"}},"id":"msg-3","timestamp":"2026-07-31T08:13:35.000Z"}"#, "\n",
        );
        let msgs = parse(sample, 0);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].id, "msg-1");
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[1].id, "msg-2");
        assert_eq!(msgs[1].role, Role::Assistant);
        assert_eq!(msgs[1].blocks.len(), 3);
        assert_eq!(msgs[2].id, "msg-3");
        assert_eq!(msgs[2].role, Role::User);
    }

    #[test]
    fn locate_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(locate(tmp.path(), "../bad").is_none());
    }
}
