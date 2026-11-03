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

    fn finalize_history_entry(&self, path: &Path, meta: &mut RowMeta) {
        meta.id = path
            .ancestors()
            .nth(3)
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned());
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

fn is_our_shape(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .and_then(crate::agent::edit::url_in_command)
        .is_some_and(|url| url.contains("/hook/antigravity/"))
}
