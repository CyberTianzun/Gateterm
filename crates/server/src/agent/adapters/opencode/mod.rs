//! OpenCode(`opencode`)adapter。
//!
//! OpenCode 的扩展点与 pi 类似:在 `~/.config/opencode/plugins/` 下放置一个 JS/TS 插件文件,
//! 启动时由 OpenCode 自动加载。因此这里采用整文件托管([`EditOp::WriteOwnedFile`]),
//! 所有权由文件首行的 `gateterm-managed` 标记识别。
//!
//! 与 pi 的不同点:
//! 1. **支持远程权限审批(`PermissionRequest`)**:OpenCode 在需要工具执行授权时会触发
//!    `permission.asked` 事件及 `permission.ask` 钩子,插件将其转发到
//!    `/hook/opencode/permission-request` 阻塞等待决策并写回 OpenCode。
//! 2. **实时镜像对话记录**:OpenCode 默认把会话存于 SQLite(`~/.local/share/opencode/opencode.db`),
//!    插件在托管终端会话中会把消息增量同步到
//!    `~/.local/share/opencode/gateterm-sessions/<session_id>.jsonl`,供聊天视图和历史会话直接读取。

pub mod transcript;

use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::agent::detect::DetectEnv;
use crate::agent::edit::{is_owned_file, ConfigEdit, EditOp, InstallCtx};
use crate::agent::history::{self, HistoryRoot, ResumeSpec, ResumeTarget, RowMeta};
use crate::agent::kit::{
    detect_by_spec, render_permission_decision, str_field, ConfigEvidence, DetectSpec,
};
use crate::agent::store::{Decision, Update};
use crate::agent::{AgentAdapter, Detected, Integration};
use gateterm_protocol::AgentState;

pub struct OpenCode;

const TEMPLATE: &str = include_str!("plugin.js");
const BASE_PLACEHOLDER: &str = "__GATETERM_HOOK_BASE__";
const BLOCKING_PLACEHOLDER: &str = "__GATETERM_BLOCKING__";
const HOOK_PREFIX: &str = "http://127.0.0.1:";

fn config_dir(home: &Path) -> PathBuf {
    home.join(".config").join("opencode")
}

fn plugin_path(home: &Path) -> PathBuf {
    config_dir(home).join("plugins").join("gateterm.js")
}

pub fn plugin_source(port: u16, include_blocking: bool) -> String {
    TEMPLATE
        .replace(BASE_PLACEHOLDER, &format!("{HOOK_PREFIX}{port}/hook/opencode/"))
        .replace(BLOCKING_PLACEHOLDER, if include_blocking { "true" } else { "false" })
}

fn port_in(text: &str) -> Option<u16> {
    let rest = &text[text.find(HOOK_PREFIX)? + HOOK_PREFIX.len()..];
    let (port, tail) = rest.split_once('/')?;
    tail.starts_with("hook/opencode/").then(|| port.parse().ok()).flatten()
}

impl AgentAdapter for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn name(&self) -> &'static str {
        "OpenCode"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = config_dir(&env.home);
        let evidence_files = [
            dir.join("opencode.json"),
            dir.join("opencode.jsonc"),
            dir.join("tui.json"),
            env.home.join(".local").join("share").join("opencode").join("opencode.db"),
        ];
        let existing_evidence = evidence_files.iter().find(|p| p.is_file());
        detect_by_spec(
            env,
            &DetectSpec {
                agent_name: self.name(),
                config_path: plugin_path(&env.home),
                evidence: existing_evidence.map(|p| ConfigEvidence::FileExists(p.as_path())),
                cli: "opencode",
                dir: &dir,
                ignore_in_dir: &["plugins", "sessions"],
            },
        )
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit {
            path: plugin_path(&ctx.home),
            op: EditOp::WriteOwnedFile {
                content: plugin_source(ctx.port, ctx.include_blocking),
            },
        }])
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit {
            path: plugin_path(&ctx.home),
            op: EditOp::RemoveOwnedFile,
        }])
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let path = plugin_path(&ctx.home);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (Integration::NotInstalled, Vec::new());
        };
        if !is_owned_file(&text) {
            return (
                Integration::NotInstalled,
                vec![format!("{} 已存在且不是 gateterm 写的,安装会被拒绝", path.display())],
            );
        }
        match port_in(&text) {
            Some(p) if p == ctx.port => (Integration::Installed, Vec::new()),
            _ => (Integration::StalePort, Vec::new()),
        }
    }

    fn agent_session_id(&self, body: &Value) -> Option<String> {
        str_field(body, "session_id")
            .or_else(|| str_field(body, "sessionId"))
            .or_else(|| str_field(body, "sessionID"))
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        let (state, detail) = match event {
            "SessionStart" | "Stop" => (AgentState::Idle, None),
            "UserPromptSubmit" => (AgentState::Thinking, None),
            "PreToolUse" | "PostToolUse" => (
                AgentState::Working,
                str_field(body, "tool_name").or_else(|| str_field(body, "toolName")),
            ),
            "Notification" => {
                let kind = str_field(body, "notification_type")
                    .or_else(|| str_field(body, "notificationType"));
                match kind.as_deref() {
                    Some("permission_prompt" | "elicitation_dialog" | "idle_prompt") => {
                        (AgentState::Waiting, kind)
                    }
                    _ => return None,
                }
            }
            "ErrorOccurred" => (AgentState::Error, str_field(body, "error")),
            "SessionEnd" => (AgentState::Done, None),
            _ => return None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: str_field(body, "cwd"),
            title: str_field(body, "session_title").or_else(|| str_field(body, "title")),
            transcript: str_field(body, "transcript_path")
                .or_else(|| str_field(body, "transcriptPath")),
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

    fn is_blocking(&self, event: &str) -> bool {
        event == "PermissionRequest"
    }

    fn render(&self, decision: &Decision) -> Value {
        render_permission_decision(decision)
    }

    fn history_roots(&self, home: &Path) -> Vec<HistoryRoot> {
        vec![
            HistoryRoot {
                path: home.join(".local/share/opencode/gateterm-sessions"),
                max_depth: 1,
            },
            HistoryRoot {
                path: home.join(".config/opencode/sessions"),
                max_depth: 1,
            },
        ]
    }

    fn inspect_history_row(&self, row: &Value, meta: &mut RowMeta) {
        use history::{model_name, text};
        match row.get("type").and_then(Value::as_str) {
            Some("session") => {
                meta.id = text(row, "id").or(meta.id.take());
                meta.cwd = text(row, "cwd").or(meta.cwd.take());
                meta.title = text(row, "title").or(meta.title.take());
                meta.model = model_name(row).or(meta.model.take());
            }
            Some("session_info") => {
                meta.id = text(row, "id").or(meta.id.take());
                meta.cwd = text(row, "cwd").or(meta.cwd.take());
                meta.title = text(row, "name")
                    .or_else(|| text(row, "title"))
                    .or(meta.title.take());
            }
            Some("message") => {
                meta.model = model_name(&row["message"]).or(meta.model.take());
            }
            None => {
                if let Some(info) = row.get("info") {
                    meta.id = text(info, "sessionID")
                        .or_else(|| text(info, "id"))
                        .or(meta.id.take());
                    meta.cwd = info
                        .pointer("/path/cwd")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| text(info, "directory"))
                        .or(meta.cwd.take());
                    meta.model = model_name(info).or(meta.model.take());
                }
            }
            _ => {}
        }
    }

    fn finalize_history_entry(&self, path: &Path, meta: &mut RowMeta) {
        if meta.id.is_none() {
            meta.id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned());
        }
    }

    fn resume_spec(&self) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            cli: "opencode",
            flag: "--session",
            target: ResumeTarget::SessionId,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_roundtrips_through_the_plugin_source() {
        let src = plugin_source(7683, true);
        assert!(is_owned_file(&src), "第一行必须带所有权标记");
        assert!(!src.contains(BASE_PLACEHOLDER));
        assert!(!src.contains(BLOCKING_PLACEHOLDER));
        assert_eq!(port_in(&src), Some(7683));
    }
}
