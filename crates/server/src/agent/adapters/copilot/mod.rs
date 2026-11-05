//! GitHub Copilot CLI(`copilot`)adapter。
//!
//! 依据本机 `copilot` 二进制的 hooks schema 与 `~/.copilot` 实测结构:
//!
//! 1. **配置在 `~/.copilot/hooks/gateterm.json`**:Copilot CLI 启动时自动加载
//!    `~/.copilot/hooks/*.json` 下的全部配置文件。我们采用整文件托管
//!    ([`EditOp::WriteOwnedFile`]),首行内嵌 `"_managedBy": "gateterm-managed"` 供所有权识别,
//!    不动用户自己的 `hooks.json` 或其他第三方 hook 文件。
//! 2. **支持远程权限审批(`PermissionRequest`)**:Copilot CLI 的 `PermissionRequest`
//!    与 Claude Code 协议同源,通过 `hookSpecificOutput.decision.behavior = "allow" | "deny"`
//!    返回审批结果;若 `~/.copilot/hooks/` 下其它文件已注册 `PermissionRequest`,则报出冲突。
//! 3. **会话记录与历史恢复**:每个会话位于 `~/.copilot/session-state/<session-id>/`,
//!    包含 `events.jsonl` 与 `workspace.yaml`,恢复命令为 `copilot --resume <session-id>`。

pub mod transcript;

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::agent::detect::DetectEnv;
use crate::agent::edit::{
    collect_hook_handlers, is_ours, is_owned_file, url_in_command, ConfigEdit, EditOp, InstallCtx,
};
use crate::agent::history::{self, HistoryRoot, ResumeSpec, ResumeTarget, RowMeta};
use crate::agent::hook_cli::{self, CommandWrap};
use crate::agent::kit::{
    detect_by_spec, render_permission_decision, str_field, ConfigEvidence, DetectSpec,
};
use crate::agent::store::{Decision, Update};
use crate::agent::{AgentAdapter, Detected, Integration};
use gateterm_protocol::AgentState;

pub struct Copilot;

const TELEMETRY_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "ErrorOccurred",
    "Notification",
    "SessionEnd",
];

const BLOCKING_EVENT: &str = "PermissionRequest";
const TELEMETRY_TIMEOUT_SECS: u64 = 5;
const BLOCKING_TIMEOUT_SECS: u64 = 600;

fn home_dir(home: &Path) -> PathBuf {
    home.join(".copilot")
}

fn hooks_dir(home: &Path) -> PathBuf {
    home_dir(home).join("hooks")
}

fn hooks_path(home: &Path) -> PathBuf {
    hooks_dir(home).join("gateterm.json")
}

fn handler(port: u16, event: &str, timeout_sec: u64) -> Value {
    let cmd = hook_cli::command_for("copilot", port, event);
    json!({
        "type": "command",
        "command": cmd,
        "bash": cmd,
        "powershell": cmd,
        "timeoutSec": timeout_sec,
    })
}

pub fn hooks_source(port: u16, include_blocking: bool) -> String {
    let mut hooks = serde_json::Map::new();
    for &event in TELEMETRY_EVENTS {
        hooks.insert(
            event.to_string(),
            json!([handler(port, event, TELEMETRY_TIMEOUT_SECS)]),
        );
    }
    if include_blocking {
        hooks.insert(
            BLOCKING_EVENT.to_string(),
            json!([handler(port, BLOCKING_EVENT, BLOCKING_TIMEOUT_SECS)]),
        );
    }
    let body = serde_json::to_string_pretty(&json!({
        "version": 1,
        "hooks": hooks,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    let tail = body.strip_prefix('{').unwrap_or(&body);
    format!("{{\"_managedBy\": \"gateterm-managed\",{tail}\n")
}

fn scan_foreign_conflicts(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut conflicts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == "gateterm.json") {
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for key in ["PermissionRequest", "permissionRequest"] {
            if doc
                .get("hooks")
                .and_then(|h| h.get(key))
                .and_then(Value::as_array)
                .is_some_and(|arr| !arr.is_empty())
            {
                conflicts.push(format!(
                    "PermissionRequest 上已有别的程序的 hook({})",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
                break;
            }
        }
    }
    conflicts
}

impl AgentAdapter for Copilot {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn name(&self) -> &'static str {
        "GitHub Copilot"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = home_dir(&env.home);
        let config = dir.join("config.json");
        detect_by_spec(
            env,
            &DetectSpec {
                agent_name: self.name(),
                config_path: hooks_path(&env.home),
                evidence: Some(ConfigEvidence::FileExists(&config)),
                cli: "copilot",
                dir: &dir,
                ignore_in_dir: &["hooks"],
            },
        )
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit {
            path: hooks_path(&ctx.home),
            op: EditOp::WriteOwnedFile {
                content: hooks_source(ctx.port, ctx.include_blocking),
            },
        }])
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit {
            path: hooks_path(&ctx.home),
            op: EditOp::RemoveOwnedFile,
        }])
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let mut conflicts = scan_foreign_conflicts(&hooks_dir(&ctx.home));
        let path = hooks_path(&ctx.home);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (Integration::NotInstalled, conflicts);
        };
        if !is_owned_file(&text) {
            conflicts.push(format!(
                "{} 已存在且不是 gateterm 写的,安装会被拒绝",
                path.display()
            ));
            return (Integration::NotInstalled, conflicts);
        }
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            return (Integration::NotInstalled, conflicts);
        };
        let mut handlers = Vec::new();
        collect_hook_handlers(&doc, &mut handlers);
        let state = if handlers.iter().any(|h| is_ours(h, ctx.port)) {
            Integration::Installed
        } else if handlers.iter().any(|h| is_our_shape(h)) {
            Integration::StalePort
        } else {
            Integration::NotInstalled
        };
        (state, conflicts)
    }

    fn agent_session_id(&self, body: &Value) -> Option<String> {
        str_field(body, "sessionId").or_else(|| str_field(body, "session_id"))
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        let (state, detail) = match event {
            "SessionStart" | "Stop" | "AgentStop" => (AgentState::Idle, None),
            "UserPromptSubmit" | "UserPromptSubmitted" => (AgentState::Thinking, None),
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" | "SubagentStart" | "SubagentStop" => {
                let tool = str_field(body, "toolName")
                    .or_else(|| str_field(body, "tool_name"))
                    .or_else(|| {
                        body.pointer("/toolCalls/0/name")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    });
                (AgentState::Working, tool)
            }
            "Notification" => {
                let kind = str_field(body, "notificationType")
                    .or_else(|| str_field(body, "notification_type"));
                match kind.as_deref() {
                    Some(
                        "permission_prompt"
                        | "idle_prompt"
                        | "elicitation_dialog"
                        | "agent_needs_input",
                    ) => (AgentState::Waiting, kind),
                    _ => return None,
                }
            }
            "ErrorOccurred" => {
                if body.get("recoverable").and_then(Value::as_bool) == Some(true) {
                    (AgentState::Working, None)
                } else {
                    (AgentState::Error, str_field(body, "error"))
                }
            }
            "SessionEnd" => (AgentState::Done, None),
            _ => return None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: str_field(body, "cwd"),
            title: None,
            transcript: str_field(body, "transcriptPath")
                .or_else(|| str_field(body, "transcript_path")),
        })
    }

    fn supports_transcript(&self) -> bool {
        true
    }

    fn locate_transcript(&self, home: &Path, agent_session_id: &str) -> Option<PathBuf> {
        transcript::locate(home, agent_session_id)
    }

    fn parse_transcript(
        &self,
        text: &str,
        base_offset: u64,
    ) -> Vec<crate::agent::transcript::Message> {
        transcript::parse(text, base_offset)
    }

    fn telemetry_reply(&self) -> Option<Value> {
        Some(json!({}))
    }

    fn is_blocking(&self, event: &str) -> bool {
        event == BLOCKING_EVENT
    }

    fn render(&self, decision: &Decision) -> Value {
        render_permission_decision(decision)
    }

    fn command_wrap(&self) -> CommandWrap {
        CommandWrap::WindowsPowerShellEncoded
    }

    fn history_roots(&self, home: &Path) -> Vec<HistoryRoot> {
        vec![HistoryRoot {
            path: home.join(".copilot/session-state"),
            max_depth: 1,
        }]
    }

    fn accept_history_file(&self, file_name: &str) -> bool {
        file_name == "events.jsonl"
    }

    fn inspect_history_row(&self, row: &Value, meta: &mut RowMeta) {
        use history::{model_name, text};
        let Some(data) = row.get("data") else {
            return;
        };
        match row.get("type").and_then(Value::as_str) {
            Some("session.start" | "session.resume") => {
                meta.id = text(data, "sessionId").or(meta.id.take());
                meta.cwd = data
                    .pointer("/context/cwd")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .or(meta.cwd.take());
                meta.model = text(data, "selectedModel").or(meta.model.take());
            }
            Some("session.model_change") => {
                meta.model = text(data, "newModel").or(meta.model.take());
            }
            Some("assistant.message") => {
                meta.model = model_name(data).or(meta.model.take());
            }
            _ => {}
        }
    }

    fn finalize_history_entry(&self, path: &Path, meta: &mut RowMeta) {
        let Some(dir) = path.parent() else {
            return;
        };
        if meta.id.is_none() {
            meta.id = dir.file_name().map(|s| s.to_string_lossy().into_owned());
        }
        if let Ok(yaml) = std::fs::read_to_string(dir.join("workspace.yaml")) {
            let ws = parse_workspace_yaml(&yaml);
            if meta.cwd.is_none() {
                meta.cwd = ws.cwd;
            }
            if let Some(name) = ws.name
                && (ws.user_named || meta.title.is_none())
            {
                meta.title = Some(name);
            }
        }
    }

    fn resume_spec(&self) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            cli: "copilot",
            flag: "--resume",
            target: ResumeTarget::SessionId,
        })
    }
}

fn is_our_shape(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .and_then(url_in_command)
        .is_some_and(|u| u.contains("http://127.0.0.1:") && u.contains("/hook/copilot/"))
}

#[derive(Default)]
struct WorkspaceMeta {
    cwd: Option<String>,
    name: Option<String>,
    user_named: bool,
}

fn parse_workspace_yaml(yaml: &str) -> WorkspaceMeta {
    let mut out = WorkspaceMeta::default();
    for line in yaml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("cwd:") {
            let val = unquote_yaml(rest.trim());
            if !val.is_empty() {
                out.cwd = Some(val);
            }
        } else if let Some(rest) = line.strip_prefix("name:") {
            let val = unquote_yaml(rest.trim());
            if !val.is_empty() {
                out.name = Some(val);
            }
        } else if let Some(rest) = line.strip_prefix("user_named:") {
            out.user_named = rest.trim() == "true";
        }
    }
    out
}

fn unquote_yaml(s: &str) -> String {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_source_is_valid_owned_json() {
        let src = hooks_source(7683, true);
        assert!(is_owned_file(&src), "第一行必须带所有权标记");
        let doc: Value = serde_json::from_str(&src).expect("必须是合法 JSON");
        assert!(doc.pointer("/hooks/SessionStart").is_some());
        assert!(doc.pointer("/hooks/PermissionRequest").is_some());

        let no_blocking = hooks_source(7683, false);
        let doc2: Value = serde_json::from_str(&no_blocking).unwrap();
        assert!(doc2.pointer("/hooks/PermissionRequest").is_none());
    }

    #[test]
    fn parses_workspace_yaml_fields() {
        let yaml = "id: 0bd6225d\ncwd: D:\\online\\Octoterm\nname: \"Fix tests\"\nuser_named: true\n";
        let ws = parse_workspace_yaml(yaml);
        assert_eq!(ws.cwd.as_deref(), Some("D:\\online\\Octoterm"));
        assert_eq!(ws.name.as_deref(), Some("Fix tests"));
        assert!(ws.user_named);
    }
}
