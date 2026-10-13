//! pi(`@mariozechner/pi-coding-agent`)adapter。
//!
//! 与另外两家的根本差异:**pi 没有 hook 配置**。它的扩展点是「往
//! `~/.pi/agent/extensions/` 里放一个 TS 文件」,由 pi 启动时用 jiti 直接加载。所以这里
//! 装的不是 JSON 里的一条条 hook,而是**整个文件**([`EditOp::WriteOwnedFile`]),
//! 所有权靠文件第一行的标记认。文件内容见 `pi_extension.ts`。
//!
//! 由此带来的取舍:
//!
//! 1. **只上报、不拦截**。pi 本身没有权限审批这个概念(工具一律直接执行),扩展里
//!    虽然能用 `tool_call` 拦,但那等于凭空给 pi 加一道它原来没有的闸,行为变化太大。
//!    所以这里没有阻塞式事件,远程授权对 pi 不适用。
//! 2. **事件名借 Claude 的**。扩展里自己拼 payload,事件名和字段名都照 Claude 的来
//!    (`session-start` / `pre-tool-use` / `transcript_path` …),服务端只多认一个 agent id。
//! 3. **装完要重新加载**。扩展在 pi 启动时加载,已经在跑的 pi 要 `/reload` 或重启。

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::detect::{self, DetectEnv};
use super::edit::{is_owned_file, ConfigEdit, EditOp, InstallCtx};
use super::store::{Decision, Update};
use super::{AgentAdapter, Confidence, Detected, Integration};
use octoterm_protocol::AgentState;

pub struct Pi;

const TEMPLATE: &str = include_str!("pi_extension.ts");
const BASE_PLACEHOLDER: &str = "__OCTOTERM_HOOK_BASE__";
const HOOK_PREFIX: &str = "http://127.0.0.1:";

fn agent_dir(home: &Path) -> PathBuf {
    home.join(".pi").join("agent")
}

fn extension_path(home: &Path) -> PathBuf {
    agent_dir(home).join("extensions").join("octoterm.ts")
}

/// 扩展文件全文。端口直接写进去,和另外两家一样:端口变了就是 `StalePort`,由启动
/// 自检报出来,而不是让扩展在运行时去猜。
pub fn extension_source(port: u16) -> String {
    TEMPLATE.replace(BASE_PLACEHOLDER, &format!("{HOOK_PREFIX}{port}/hook/pi/"))
}

/// 从已装的扩展里读回端口。读不出就是 `None`。
fn port_in(text: &str) -> Option<u16> {
    let rest = &text[text.find(HOOK_PREFIX)? + HOOK_PREFIX.len()..];
    let (port, tail) = rest.split_once('/')?;
    tail.starts_with("hook/pi/").then(|| port.parse().ok()).flatten()
}

fn state_of(event: &str) -> Option<AgentState> {
    Some(match event {
        "SessionStart" => AgentState::Idle,
        "SessionEnd" => AgentState::Done,
        "UserPromptSubmit" => AgentState::Thinking,
        "PreToolUse" | "PostToolUse" => AgentState::Working,
        "Stop" => AgentState::Idle,
        _ => return None,
    })
}

fn str_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

impl AgentAdapter for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn name(&self) -> &'static str {
        "pi"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = agent_dir(&env.home);
        let config_path = Some(extension_path(&env.home));
        if let Some(exe) = detect::on_path(env, "pi") {
            return Detected {
                installed: true,
                confidence: Confidence::High,
                reason: "cli-path",
                detail: format!("PATH 上找到 {}", exe.display()),
                config_path,
            };
        }
        // `extensions/` 可能是我们自己建的,不算证据;sessions / settings / auth 是 pi 自己留的
        if dir.is_dir() && detect::dir_has_entries_besides(&dir, &["extensions"]) {
            return Detected {
                installed: true,
                confidence: Confidence::Medium,
                reason: "parent-dir",
                detail: format!("{} 下有使用痕迹", dir.display()),
                config_path,
            };
        }
        Detected {
            installed: false,
            confidence: Confidence::Low,
            reason: "not-found",
            detail: "没有找到 pi 的安装痕迹".into(),
            config_path: None,
        }
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit {
            path: extension_path(&ctx.home),
            op: EditOp::WriteOwnedFile { content: extension_source(ctx.port) },
        }])
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        Ok(vec![ConfigEdit { path: extension_path(&ctx.home), op: EditOp::RemoveOwnedFile }])
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let path = extension_path(&ctx.home);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (Integration::NotInstalled, Vec::new());
        };
        if !is_owned_file(&text) {
            // 同名文件是用户的。不算装了,也不会被我们覆盖 —— 但要说出来,否则安装失败没头绪
            return (
                Integration::NotInstalled,
                vec![format!("{} 已存在且不是 octoterm 写的,安装会被拒绝", path.display())],
            );
        }
        // 「端口对但内容旧」由 scan 里的通用自检降级成 Outdated
        match port_in(&text) {
            Some(p) if p == ctx.port => (Integration::Installed, Vec::new()),
            _ => (Integration::StalePort, Vec::new()),
        }
    }

    fn activation(&self) -> Option<&'static str> {
        Some("pi-reload")
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        let state = state_of(event)?;
        let detail = match event {
            "PreToolUse" | "PostToolUse" => str_field(body, "tool_name"),
            _ => None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: str_field(body, "cwd"),
            title: str_field(body, "session_title"),
            transcript: str_field(body, "transcript_path"),
        })
    }

    fn supports_transcript(&self) -> bool {
        true
    }

    fn locate_transcript(&self, home: &Path, agent_session_id: &str) -> Option<PathBuf> {
        super::pi_transcript::locate(&agent_dir(home).join("sessions"), agent_session_id)
    }

    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<super::transcript::Message> {
        super::pi_transcript::parse(text, base_offset)
    }

    fn is_blocking(&self, _event: &str) -> bool {
        false
    }

    /// 没有阻塞式事件,这里只会在「无决定」时被调用。
    fn render(&self, _decision: &Decision) -> Value {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_roundtrips_through_the_extension_source() {
        let src = extension_source(7683);
        assert!(is_owned_file(&src), "第一行必须带所有权标记");
        assert!(!src.contains(BASE_PLACEHOLDER));
        assert_eq!(port_in(&src), Some(7683));
    }
}
