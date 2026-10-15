//! Grok 的会话记录(`~/.grok/sessions/<编码后的 cwd>/<会话 id>/updates.jsonl`)→ 归一化消息。
//!
//! 格式按 grok 1.0.52 的实际文件(见 `tests/fixtures/grok-updates.jsonl`):每行是一条
//! ACP 会话更新,包在 `{timestamp, method, params: {sessionId, update, _meta: {eventId}}}`
//! 里,`update.sessionUpdate` 区分种类:
//!
//! - `user_message_chunk` / `agent_message_chunk` / `agent_thought_chunk`:正文与思考,
//!   文本在 `content.text`。文件里每行就是完整的一段,不需要拼接。
//! - `tool_call`:发起工具调用,工具名在 `_meta["x.ai/tool"].name`(退回 `title`),
//!   入参在 `rawInput`。
//! - `tool_call_update`:**只有带 `status`(`completed` / `failed`)的才是结果**。中间那些
//!   不带状态的更新只是在改标题、补描述,当成结果会多出一堆假输出。
//! - `hook_execution`、`turn_completed` 等是 grok 自己的运行记录,不是对话内容,跳过。
//!
//! 每行都带 `_meta.eventId`,直接当消息 id —— 同一行读几次都是同一个 id。

use serde_json::Value;

use super::transcript::{clamp_text, flatten_tool_input, offset_id, Block, Message, Role};

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
    let params = row.get("params")?;
    let update = params.get("update")?;
    let (role, blocks) = match update.get("sessionUpdate").and_then(Value::as_str)? {
        "user_message_chunk" => (Role::User, text_block(update, |text| Block::Text { text })),
        "agent_message_chunk" => (Role::Assistant, text_block(update, |text| Block::Text { text })),
        "agent_thought_chunk" => (Role::Assistant, text_block(update, |text| Block::Thinking { text })),
        "tool_call" => {
            let name = update
                .pointer("/_meta/x.ai~1tool/name")
                .or_else(|| update.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let input = update.get("rawInput").cloned().unwrap_or(Value::Null);
            (Role::Assistant, vec![Block::ToolUse { name, input: flatten_tool_input(&input) }])
        }
        // 工具输出归到 user 一侧,和 Claude、Codex 的放法一致
        "tool_call_update" => {
            let ok = match update.get("status").and_then(Value::as_str) {
                Some("completed") => true,
                Some("failed") => false,
                _ => return None,
            };
            (Role::User, vec![Block::ToolResult { ok, text: clamp_text(result_text(update)) }])
        }
        _ => return None,
    };
    if blocks.is_empty() {
        return None;
    }
    let id = params
        .pointer("/_meta/eventId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| offset_id(at));
    let ts = row.get("timestamp").and_then(Value::as_u64);
    Some(Message { id, role, ts, blocks })
}

fn text_block(update: &Value, make: impl FnOnce(String) -> Block) -> Vec<Block> {
    match update.pointer("/content/text").and_then(Value::as_str) {
        Some(t) if !t.trim().is_empty() => vec![make(clamp_text(t.to_string()))],
        _ => Vec::new(),
    }
}

/// 结果文本在 `content[].content.text`;没有的话退回 `rawOutput.output_for_prompt`
/// (shell 类工具总有它)。
fn result_text(update: &Value) -> String {
    let texts: Vec<&str> = update
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.pointer("/content/text").and_then(Value::as_str))
        .collect();
    if !texts.is_empty() {
        return texts.join("\n");
    }
    update
        .pointer("/rawOutput/output_for_prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 由会话 id 找到记录文件。hook 事件里通常直接带着 `transcript_path`,这里是它缺席时的退路。
///
/// 布局是 `sessions/<URL 编码的 cwd>/<会话 id>/updates.jsonl`;cwd 太长时 grok 改用
/// 「slug + 哈希」命名,所以不去重算编码,而是在每个分组下找这个 id —— 深度固定为一层。
pub fn locate(home: &std::path::Path, session_id: &str) -> Option<std::path::PathBuf> {
    // 会话 id 会拼进路径,不许它带分隔符跳出 sessions 目录
    if session_id.is_empty() || session_id.contains(['/', '\\']) || session_id.contains("..") {
        return None;
    }
    let root = home.join(".grok").join("sessions");
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|group| group.path().join(session_id).join("updates.jsonl"))
        .find(|p| p.is_file())
}
