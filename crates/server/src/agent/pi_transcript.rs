//! pi 的会话文件(`~/.pi/agent/sessions/--<cwd>--/<时间戳>_<uuid>.jsonl`)→ 归一化消息。
//!
//! 格式见 pi 自带的 `docs/session-format.md`。要点:
//!
//! 1. 第一行是 `type: "session"` 头,不是消息;对话在 `type: "message"` 的 `message` 里,
//!    按 `role` 分:`user` / `assistant` / `toolResult` / `bashExecution` / `custom` …
//! 2. **每条记录自带 8 位十六进制 `id`**,文件内唯一,直接拿来当消息 id。
//! 3. 记录之间是**树**(`parentId`),`/tree` 分叉后,被放弃的那一支也还在文件里。
//!    这里按行顺序平铺,不按树重建当前分支 —— 已知缺口:分叉过的会话,聊天视图里会
//!    看到被放弃的那一支。

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
    // 坏行(窗口首尾的半行)只跳过它自己
    let row: Value = serde_json::from_str(line).ok()?;
    let (role, blocks, ts) = match row.get("type").and_then(Value::as_str)? {
        "message" => {
            let m = row.get("message")?;
            let (role, blocks) = message(m)?;
            (role, blocks, m.get("timestamp").and_then(Value::as_u64).map(|ms| ms / 1000))
        }
        // 压缩 / 分支摘要:是对话的一部分(模型接下来看到的就是它),按系统消息展示
        "compaction" | "branch_summary" => {
            (Role::System, text_block(row.get("summary").and_then(Value::as_str)), None)
        }
        "custom_message" if row.get("display").and_then(Value::as_bool) == Some(true) => {
            (Role::System, texts(row.get("content")), None)
        }
        // session 头、model_change、label、custom(扩展状态)… 都不是对话内容
        _ => return None,
    };
    if blocks.is_empty() {
        return None;
    }
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| offset_id(at));
    Some(Message { id, role, ts, blocks })
}

fn message(m: &Value) -> Option<(Role, Vec<Block>)> {
    Some(match m.get("role").and_then(Value::as_str)? {
        "user" => (Role::User, texts(m.get("content"))),
        "assistant" => (
            Role::Assistant,
            m.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(assistant_block)
                .collect(),
        ),
        // 工具输出归到 user 一侧,和 Claude / Codex 的放法一致
        "toolResult" => (
            Role::User,
            vec![Block::ToolResult {
                ok: m.get("isError").and_then(Value::as_bool) != Some(true),
                text: clamp_text(join_texts(m.get("content"))),
            }],
        ),
        // 用户在 pi 里敲的 `!命令`:一次调用加它的输出
        "bashExecution" => (
            Role::User,
            vec![
                Block::ToolUse {
                    name: "bash".into(),
                    input: clamp_text(str_of(m, "command")),
                },
                Block::ToolResult {
                    ok: m.get("exitCode").and_then(Value::as_i64) == Some(0),
                    text: clamp_text(str_of(m, "output")),
                },
            ],
        ),
        "custom" if m.get("display").and_then(Value::as_bool) == Some(true) => {
            (Role::System, texts(m.get("content")))
        }
        "compactionSummary" | "branchSummary" => {
            (Role::System, text_block(m.get("summary").and_then(Value::as_str)))
        }
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
        // 图片等:认不出的不透传
        _ => None,
    }
}

fn non_empty(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(|s| clamp_text(s.to_string()))
}

fn text_block(s: Option<&str>) -> Vec<Block> {
    s.filter(|s| !s.trim().is_empty())
        .map(|s| vec![Block::Text { text: clamp_text(s.to_string()) }])
        .unwrap_or_default()
}

/// `content` 可以是字符串,也可以是 `[{type:"text"} | {type:"image"}]`。图片跳过。
fn texts(v: Option<&Value>) -> Vec<Block> {
    match v {
        Some(Value::String(s)) => text_block(Some(s)),
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

/// 由会话 id 找文件。扩展每次上报都带 `transcript_path`,这只是兜底:文件名形如
/// `<时间戳>_<uuid>.jsonl`,放在按 cwd 命名的一层子目录里。深度写死,不递归。
pub fn locate(sessions: &std::path::Path, session_id: &str) -> Option<std::path::PathBuf> {
    let suffix = format!("_{session_id}.jsonl");
    std::fs::read_dir(sessions).ok()?.flatten().find_map(|d| {
        std::fs::read_dir(d.path())
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&suffix)))
    })
}
