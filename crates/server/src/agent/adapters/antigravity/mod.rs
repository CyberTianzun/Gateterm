//! Antigravity CLI(Google 的 `agy`)adapter。
//!
//! 依据是 agy 1.3.5 二进制里内嵌的 hooks 文档,关键行为都在本机实测过(临时探针 hook,
//! 测完删除):
//!
//! 1. **配置在 `~/.gemini/config/hooks.json`**,是几个 Antigravity 产品(CLI、2.0、IDE)
//!    **共用**的文件,不能整份接管。它的顶层是**具名 hook**,事件挂在名字底下,所以我们
//!    整块管理顶层的 `gateterm` 键([`EditOp::SetOwnedKey`])。
//! 2. **事件有两种形状**:`PreToolUse` / `PostToolUse` 要包一层 `{matcher, hooks}`,
//!    其余事件直接是处理器列表。只支持 command 型,所以和 Codex、Grok 一样经
//!    [`crate::agent::hook_cli`] 转发。
//! 3. **绝不装 `PreToolUse`**。实测:它的回复里没有 `decision` 时,agy 直接**拒绝**这次
//!    工具调用;而能填的 decision 里 `allow` 会跳过用户授权,`ask` / `force_ask` 会改掉
//!    原有的授权流程。「正在跑工具」改由 `PostToolUse` 报。
//! 4. **每个 hook 都要在 stdout 输出 JSON 对象**,状态类事件我们回 `{}`
//!    (见 [`AgentAdapter::telemetry_reply`])。
//! 5. **payload 是驼峰的**:会话 id 是 `conversationId`,每个事件都带 `transcriptPath`。
//!    没有「用户提交」和「会话结束」事件:`PreInvocation`(调用模型前)当作思考中,
//!    `Stop`(执行循环结束)当作空闲;已退出的会话靠终端关闭或同终端开新会话收走。
//! 6. 实测 TUI 和非交互模式都是单进程,hook 由 agy 自己执行,环境来自启动它的终端,
//!    没有 Codex 那种共享守护进程的问题(`agy remote-control` 守护进程默认不开)。

pub mod transcript;

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::agent::detect::DetectEnv;
use crate::agent::edit::{collect_hook_handlers, is_ours, ConfigEdit, EditOp, InstallCtx};
use crate::agent::history::{HistoryRoot, ResumeSpec, ResumeTarget, RowMeta};
use crate::agent::hook_cli::{self, CommandWrap};
use crate::agent::kit::{detect_by_spec, str_field, ConfigEvidence, DetectSpec};
use crate::agent::store::{Decision, Update};
use crate::agent::{AgentAdapter, Detected, Integration};
use gateterm_protocol::AgentState;

pub struct Antigravity;

/// 我们在共用 hooks.json 里占的顶层键。改它等于让旧安装变成「不是我们的」—— 不要改。
const KEY: &str = "gateterm";

/// 直接是处理器列表的事件
const FLAT_EVENTS: &[&str] = &["SessionStart", "PreInvocation", "Stop"];
/// 要包 `{matcher, hooks}` 的工具事件。只有 PostToolUse,原因见模块文档第 3 条
const TOOL_EVENTS: &[&str] = &["PostToolUse"];

const TIMEOUT_SECS: u64 = 10;

fn hooks_path(home: &Path) -> PathBuf {
    home.join(".gemini").join("config").join("hooks.json")
}

fn cli_dir(home: &Path) -> PathBuf {
    home.join(".gemini").join("antigravity-cli")
}

fn handler(port: u16, event: &str) -> Value {
    json!({
        "type": "command",
        "command": hook_cli::command_for("antigravity", port, event),
        "timeout": TIMEOUT_SECS,
    })
}

/// 我们那一整块配置。
fn our_block(port: u16) -> Value {
    let mut block = serde_json::Map::new();
    for event in FLAT_EVENTS {
        block.insert(event.to_string(), json!([handler(port, event)]));
    }
    for event in TOOL_EVENTS {
        block.insert(event.to_string(), json!([{ "matcher": "*", "hooks": [handler(port, event)] }]));
    }
    Value::Object(block)
}

impl AgentAdapter for Antigravity {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn name(&self) -> &'static str {
        "Antigravity CLI"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = cli_dir(&env.home);
        let install_id = dir.join("installation_id");
        // antigravity-cli/ 整个目录都是 agy 自己写的(我们只碰 config/hooks.json)
        detect_by_spec(
            env,
            &DetectSpec {
                agent_name: self.name(),
                config_path: hooks_path(&env.home),
                evidence: Some(ConfigEvidence::FileExists(&install_id)),
                cli: "agy",
                dir: &dir,
                ignore_in_dir: &[],
            },
        )
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit {
            path: hooks_path(&ctx.home),
            op: EditOp::SetOwnedKey { key: KEY.into(), value: our_block(ctx.port) },
        }])
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit { path: hooks_path(&ctx.home), op: EditOp::RemoveOwnedKey { key: KEY.into() } }])
    }

    /// 新开的 agy 会话直接生效;已经在跑的要重启(或在 `/hooks` 里重新加载)。
    fn activation(&self) -> Option<&'static str> {
        Some("antigravity-restart")
    }

    fn activation_hint(&self) -> Option<&'static str> {
        Some("在 agy 里执行 /hooks 重新加载或重启 agy")
    }

    fn agent_session_id(&self, body: &Value) -> Option<String> {
        str_field(body, "conversationId")
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        // Gemini CLI's session_id payload is a different protocol. A real agy
        // hook carries conversationId on every event; never invent "default".
        self.agent_session_id(body)?;
        let (state, detail) = match event {
            "SessionStart" | "Stop" => (AgentState::Idle, None),
            "PreInvocation" => (AgentState::Thinking, None),
            "PostToolUse" => (
                if str_field(body, "error").is_some() { AgentState::Error } else { AgentState::Working },
                body.pointer("/toolCall/name").and_then(Value::as_str).map(str::to_string),
            ),
            _ => return None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: body.pointer("/workspacePaths/0").and_then(Value::as_str).map(str::to_string),
            title: None,
            transcript: str_field(body, "transcriptPath"),
        })
    }

    fn supports_transcript(&self) -> bool {
        true
    }

    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<crate::agent::transcript::Message> {
        transcript::parse(text, base_offset)
    }

    fn locate_transcript(&self, home: &Path, agent_session_id: &str) -> Option<PathBuf> {
        transcript::locate(home, agent_session_id)
    }

    fn telemetry_reply(&self) -> Option<Value> {
        Some(json!({}))
    }

    fn is_blocking(&self, _event: &str) -> bool {
        false
    }

    fn render(&self, _decision: &Decision) -> Value {
        json!({})
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let doc: Option<Value> = std::fs::read_to_string(hooks_path(&ctx.home))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let Some(block) = doc.as_ref().and_then(|d| d.get(KEY)) else {
            return (Integration::NotInstalled, Vec::new());
        };
        let mut handlers = Vec::new();
        collect_hook_handlers(block, &mut handlers);
        let state = if handlers.iter().any(|h| is_ours(h, ctx.port)) {
            Integration::Installed
        } else if handlers.iter().any(|h| is_our_shape(h)) {
            Integration::StalePort
        } else {
            Integration::NotInstalled
        };
        (state, Vec::new())
    }

    fn command_wrap(&self) -> CommandWrap {
        CommandWrap::WindowsPowerShellEncoded
    }

    fn history_roots(&self, home: &Path) -> Vec<HistoryRoot> {
        vec![HistoryRoot {
            path: home.join(".gemini/antigravity-cli/brain"),
            max_depth: 4,
        }]
    }

    fn accept_history_file(&self, file_name: &str) -> bool {
        file_name == "transcript_full.jsonl"
    }

    fn inspect_history_row(&self, row: &Value, meta: &mut RowMeta) {
        use crate::agent::history::model_name;
        if let Some(model) = model_name(row) {
            meta.model = Some(model);
        }
        if row.get("type").and_then(Value::as_str) == Some("USER_INPUT")
            && let Some(content) = row.get("content").and_then(Value::as_str)
            && let Some(model) = extract_model_selection(content)
        {
            meta.model = Some(model);
        }
    }

    fn finalize_history_entry(&self, path: &Path, meta: &mut RowMeta) {
        meta.id = path
            .ancestors()
            .nth(3)
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned());
        if meta.title.is_none()
            && let (Some(id), Some(cli_root)) = (meta.id.as_deref(), path.ancestors().nth(5))
        {
            let pbtxt_path = cli_root.join("annotations").join(format!("{id}.pbtxt"));
            if let Ok(text) = std::fs::read_to_string(pbtxt_path) {
                meta.title = parse_pbtxt_title(&text);
            }
        }
    }

    fn enrich_history_catalog(&self, home: &Path, entries: &mut Vec<crate::agent::history::Entry>) {
        use crate::agent::history::{prompt, sample_with_limits, short, text};
        use std::collections::HashMap;
        use std::io::Read;

        if !entries.iter().any(|e| e.agent_id == "antigravity") {
            return;
        }
        let dir = cli_dir(home);
        let summaries = std::fs::File::open(dir.join("jetbox_summaries_proto.pb"))
            .ok()
            .and_then(|f| {
                let mut buf = Vec::new();
                f.take(16 * 1024 * 1024).read_to_end(&mut buf).ok()?;
                Some(parse_jetbox_summaries(&buf))
            })
            .unwrap_or_default();

        entries.retain(|e| {
            e.agent_id != "antigravity"
                || !summaries
                    .get(&e.agent_session_id)
                    .is_some_and(|s| s.is_subagent)
        });

        let mut hist_cwds = HashMap::new();
        let mut hist_prompts = HashMap::new();
        if let Ok(index) = sample_with_limits(&dir.join("history.jsonl"), 0, 2 * 1024 * 1024) {
            for row in index
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            {
                let Some(id) = text(&row, "conversationId") else {
                    continue;
                };
                if let Some(ws) = text(&row, "workspace") {
                    hist_cwds.entry(id.clone()).or_insert(ws);
                }
                if row.get("type").and_then(Value::as_str) != Some("slash_command")
                    && let Some(p) = text(&row, "display").and_then(|s| prompt(&s))
                {
                    hist_prompts.entry(id).or_insert(p);
                }
            }
        }

        for entry in entries.iter_mut().filter(|e| e.agent_id == "antigravity") {
            if let Some(summary) = summaries.get(&entry.agent_session_id) {
                if entry.cwd.is_none() {
                    entry.cwd = summary.cwd.clone();
                }
                if let Some(title) = &summary.title {
                    entry.title = short(title);
                }
            }
            if entry.cwd.is_none()
                && let Some(ws) = hist_cwds.get(&entry.agent_session_id)
            {
                entry.cwd = Some(ws.clone());
            }
            if let Some(value) = hist_prompts.get(&entry.agent_session_id) {
                if entry.title.is_empty() {
                    entry.title = value.clone();
                }
                if entry.description.is_none() {
                    entry.description = Some(value.clone());
                }
            }
        }
    }

    fn resume_spec(&self) -> Option<ResumeSpec> {
        // The desktop Antigravity vault is not an agy CLI resume store.
        Some(ResumeSpec {
            cli: "agy",
            flag: "--conversation",
            target: ResumeTarget::SessionId,
        })
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct SummaryMeta {
    title: Option<String>,
    cwd: Option<String>,
    is_subagent: bool,
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() && shift < 64 {
        let b = buf[*pos];
        *pos += 1;
        val |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(val);
        }
        shift += 7;
    }
    None
}

enum WireValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

fn next_proto_field<'a>(buf: &'a [u8], pos: &mut usize) -> Option<(u32, WireValue<'a>)> {
    if *pos >= buf.len() {
        return None;
    }
    let tag = read_varint(buf, pos)?;
    let field_num = u32::try_from(tag >> 3).ok()?;
    let wire_type = (tag & 0x07) as u8;
    match wire_type {
        0 => Some((field_num, WireValue::Varint(read_varint(buf, pos)?))),
        1 => {
            *pos = pos.checked_add(8)?;
            (*pos <= buf.len()).then_some((field_num, WireValue::Fixed))
        }
        2 => {
            let len = usize::try_from(read_varint(buf, pos)?).ok()?;
            let end = pos.checked_add(len)?;
            if end > buf.len() {
                return None;
            }
            let slice = &buf[*pos..end];
            *pos = end;
            Some((field_num, WireValue::Bytes(slice)))
        }
        5 => {
            *pos = pos.checked_add(4)?;
            (*pos <= buf.len()).then_some((field_num, WireValue::Fixed))
        }
        _ => None,
    }
}

fn parse_jetbox_summaries(buf: &[u8]) -> std::collections::HashMap<String, SummaryMeta> {
    let mut out = std::collections::HashMap::new();
    let mut pos = 0;
    while let Some((field_num, val)) = next_proto_field(buf, &mut pos) {
        let (1, WireValue::Bytes(entry_bytes)) = (field_num, val) else {
            continue;
        };
        let mut epos = 0;
        let mut cid: Option<String> = None;
        let mut summary_bytes: Option<&[u8]> = None;
        while let Some((efield, eval)) = next_proto_field(entry_bytes, &mut epos) {
            match (efield, eval) {
                (1, WireValue::Bytes(b)) => {
                    cid = std::str::from_utf8(b)
                        .ok()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                }
                (2, WireValue::Bytes(b)) => summary_bytes = Some(b),
                _ => {}
            }
        }
        let (Some(id), Some(sbytes)) = (cid, summary_bytes) else {
            continue;
        };
        let mut meta = SummaryMeta::default();
        let mut spos = 0;
        while let Some((sfield, sval)) = next_proto_field(sbytes, &mut spos) {
            match (sfield, sval) {
                (1, WireValue::Bytes(b)) => {
                    if let Ok(s) = std::str::from_utf8(b) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            meta.title = Some(trimmed.to_string());
                        }
                    }
                }
                (9, WireValue::Bytes(wbytes)) if meta.cwd.is_none() => {
                    let mut wpos = 0;
                    while let Some((wfield, wval)) = next_proto_field(wbytes, &mut wpos) {
                        if let (1, WireValue::Bytes(u)) = (wfield, wval)
                            && let Ok(uri) = std::str::from_utf8(u)
                            && let Some(path) = file_uri_to_path(uri)
                        {
                            meta.cwd = Some(path);
                            break;
                        }
                    }
                }
                (17, WireValue::Bytes(tbytes)) => {
                    let mut tpos = 0;
                    while let Some((tfield, tval)) = next_proto_field(tbytes, &mut tpos) {
                        match (tfield, tval) {
                            (5, WireValue::Bytes(p)) => {
                                if std::str::from_utf8(p).is_ok_and(|s| !s.trim().is_empty()) {
                                    meta.is_subagent = true;
                                }
                            }
                            (17, WireValue::Varint(depth)) if depth > 0 => {
                                meta.is_subagent = true;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        out.insert(id, meta);
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn file_uri_to_path(uri: &str) -> Option<String> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }
    let Some(rest) = uri.strip_prefix("file://") else {
        return Some(uri.to_string());
    };
    let rest = rest
        .strip_prefix("localhost/")
        .map(|s| format!("/{s}"))
        .unwrap_or_else(|| rest.to_string());
    let decoded = percent_decode(&rest);
    let stripped = decoded.strip_prefix('/').unwrap_or(&decoded);
    let bytes = stripped.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let mut win_path = stripped.to_string();
        if cfg!(windows) {
            win_path = win_path.replace('/', "\\");
        }
        return Some(win_path);
    }
    if decoded.starts_with('/') {
        return Some(decoded);
    }
    if cfg!(windows) {
        Some(format!("\\\\{}", decoded.replace('/', "\\")))
    } else {
        Some(format!("/{decoded}"))
    }
}

fn parse_pbtxt_title(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("title") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    let s = crate::agent::history::short(&out);
                    return (!s.is_empty()).then_some(s);
                }
                '\\' => {
                    if let Some(esc) = chars.next() {
                        out.push(esc);
                    }
                }
                _ => out.push(c),
            }
        }
    }
    None
}

fn extract_model_selection(content: &str) -> Option<String> {
    let mut rest = content;
    let mut latest = None;
    while let Some((_, after_open)) = rest.split_once("<USER_SETTINGS_CHANGE>") {
        let (block, tail) = after_open
            .split_once("</USER_SETTINGS_CHANGE>")
            .unwrap_or((after_open, ""));
        rest = tail;
        if let Some((_, after_marker)) =
            block.split_once("changed setting `Model Selection` from ")
            && let Some((_, after_to)) = after_marker.split_once(" to ")
        {
            let line = after_to.lines().next().unwrap_or(after_to);
            let raw = line.split_once(". ").map_or(line, |(m, _)| m);
            let model = raw.trim().trim_end_matches('.').trim();
            if !model.is_empty() {
                latest = Some(crate::agent::history::short(model));
            }
        }
    }
    latest
}

fn is_our_shape(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .and_then(crate::agent::edit::url_in_command)
        .is_some_and(|url| url.contains("/hook/antigravity/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::history;
    use std::fs;

    fn proto_varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let mut b = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
    }

    fn proto_bytes(field: u32, payload: &[u8], out: &mut Vec<u8>) {
        proto_varint(u64::from((field << 3) | 2), out);
        proto_varint(payload.len() as u64, out);
        out.extend_from_slice(payload);
    }

    fn proto_varint_field(field: u32, val: u64, out: &mut Vec<u8>) {
        proto_varint(u64::from(field << 3), out);
        proto_varint(val, out);
    }

    fn build_jetbox_entry(
        id: &str,
        title: &str,
        workspace_uri: &str,
        parent_id: &str,
        nesting_depth: u64,
    ) -> Vec<u8> {
        let mut ws = Vec::new();
        proto_bytes(1, workspace_uri.as_bytes(), &mut ws);

        let mut traj = Vec::new();
        if !parent_id.is_empty() {
            proto_bytes(5, parent_id.as_bytes(), &mut traj);
        }
        if nesting_depth > 0 {
            proto_varint_field(17, nesting_depth, &mut traj);
        }

        let mut summary = Vec::new();
        if !title.is_empty() {
            proto_bytes(1, title.as_bytes(), &mut summary);
        }
        proto_bytes(9, &ws, &mut summary);
        if !traj.is_empty() {
            proto_bytes(17, &traj, &mut summary);
        }

        let mut entry = Vec::new();
        proto_bytes(1, id.as_bytes(), &mut entry);
        proto_bytes(2, &summary, &mut entry);

        let mut root = Vec::new();
        proto_bytes(1, &entry, &mut root);
        root
    }

    #[test]
    fn decodes_unix_and_windows_file_uris() {
        assert_eq!(
            file_uri_to_path("file:///Users/hiro/online/Gateterm").as_deref(),
            Some("/Users/hiro/online/Gateterm")
        );
        assert_eq!(
            file_uri_to_path("file:///Users/hiro/My%20Project").as_deref(),
            Some("/Users/hiro/My Project")
        );
        let win1 = file_uri_to_path("file:///C:/Users/hiro/Gateterm").unwrap();
        let win2 = file_uri_to_path("file:///c%3A/Users/hiro/My%20Repo").unwrap();
        if cfg!(windows) {
            assert_eq!(win1, r"C:\Users\hiro\Gateterm");
            assert_eq!(win2, r"c:\Users\hiro\My Repo");
        } else {
            assert_eq!(win1, "C:/Users/hiro/Gateterm");
            assert_eq!(win2, "c:/Users/hiro/My Repo");
        }
    }

    #[test]
    fn enriches_antigravity_history_and_filters_subagents() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let bin = home.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(
            bin.join(if cfg!(windows) { "agy.exe" } else { "agy" }),
            "stub",
        )
        .unwrap();

        let main_log = home
            .path()
            .join(".gemini/antigravity-cli/brain/main-1/.system_generated/logs/transcript_full.jsonl");
        fs::create_dir_all(main_log.parent().unwrap()).unwrap();
        fs::write(
            &main_log,
            concat!(
                "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"<USER_REQUEST>\\nFix resume bug\\n</USER_REQUEST>\\n<USER_SETTINGS_CHANGE>\\nThe user changed setting `Model Selection` from None to Gemini 4 Argon (High). No need to comment.\\n</USER_SETTINGS_CHANGE>\"}\n",
                "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"Fixed it.\"}\n"
            ),
        )
        .unwrap();

        let sub_log = home
            .path()
            .join(".gemini/antigravity-cli/brain/sub-1/.system_generated/logs/transcript_full.jsonl");
        fs::create_dir_all(sub_log.parent().unwrap()).unwrap();
        fs::write(
            &sub_log,
            "{\"step_index\":0,\"type\":\"SYSTEM_MESSAGE\",\"content\":\"subagent\"}\n",
        )
        .unwrap();

        let ws_slash = workspace.to_string_lossy().replace('\\', "/");
        let ws_uri = if ws_slash.starts_with('/') {
            format!("file://{ws_slash}")
        } else {
            format!("file:///{ws_slash}")
        };
        let mut pb = build_jetbox_entry("main-1", "Antigravity Resume Fix", &ws_uri, "", 0);
        pb.extend_from_slice(&build_jetbox_entry("sub-1", "", &ws_uri, "main-1", 1));
        fs::write(
            home.path()
                .join(".gemini/antigravity-cli/jetbox_summaries_proto.pb"),
            pb,
        )
        .unwrap();

        let env = DetectEnv {
            home: home.path().to_path_buf(),
            path: Some(std::env::join_paths([&bin]).unwrap()),
        };
        let catalog = history::scan(&env);
        let agy_entries: Vec<_> = catalog
            .entries
            .iter()
            .filter(|e| e.agent_id == "antigravity")
            .collect();
        assert_eq!(agy_entries.len(), 1);
        let entry = agy_entries[0];
        assert_eq!(entry.agent_session_id, "main-1");
        assert_eq!(entry.title, "Antigravity Resume Fix");
        assert_eq!(entry.model.as_deref(), Some("Gemini 4 Argon (High)"));
        assert_eq!(entry.description.as_deref(), Some("Fixed it."));
        assert_eq!(
            Path::new(entry.cwd.as_deref().unwrap()),
            workspace.as_path()
        );
        assert!(
            entry.can_resume,
            "expected can_resume=true, got unavailable_reason={:?}",
            entry.unavailable_reason
        );
    }
}
