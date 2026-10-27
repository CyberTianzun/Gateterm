//! Grok Build(xAI 的 `grok` TUI)adapter。
//!
//! 依据是它随安装附带的文档(`~/.grok/docs/user-guide/10-hooks.md`,grok 1.0.52),
//! 要点和别家的差异:
//!
//! 1. **装在 `~/.grok/hooks/gateterm.json`**,整个文件只放我们的 hook。全局 hook 目录
//!    「Always trusted」—— 没有 Codex 那种逐条审核的闸,装上即对新会话生效。
//! 2. **只能用 command 类 hook**。它也支持 `type: "http"`,但 handler 只认 `type` /
//!    `command` / `url` / `timeout` / `env`,`headers` 会被丢掉,带不上密钥和会话号;
//!    URL 里的 `$VAR` 倒是会展开,可变量为空时它会报 hook 错误 —— 于是在 gateterm 之外
//!    开的每个 grok 会话都会满屏报错。command 走 [`super::hook_cli`]:环境里没有我们的
//!    变量就静默退出,什么都不发。
//! 3. **payload 是驼峰的**:`sessionId`、`toolName`、`notificationType`。另外也带着
//!    Claude 兼容的 `session_id` 和 `transcript_path`(实测),记录路径直接从这里拿。
//! 4. **没有授权审批事件**。`PreToolUse` 回 allow 只表示「不拦」,不会替用户批准,
//!    所以只上报状态和聊天记录(见 [`super::grok_transcript`]),不做阻塞式事件。
//! 5. **子代理有自己的会话**,事件带 `subagentType`。这些事件不上报:它们不是终端前的
//!    那个会话,混进来会在列表上多出一条、聊天视图也会看错对象。
//! 6. **会话结束时还会再来一个 `Stop`**,`reason` 是 `shutdown` / `channel_closed`(正常
//!    回合结束是 `end_turn`),而且 hook 是并发跑的,它可能晚于 `SessionEnd` 到达。不忽略
//!    它的话,刚结束的会话会被它重新建回来、一直挂在列表上(实测发生过)。
//!
//! 它也会读 `~/.claude/settings.json` 里的 hook(Claude Code 兼容)。如果同时装了
//! Claude Code 集成,grok 会以 Claude 的身份打过来,但它丢掉了 `headers`,请求不带
//! 密钥 —— 服务端照规矩安静收下、什么都不做,不会误报成 Claude 会话。

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::detect::{self, DetectEnv};
use super::edit::{is_ours, ConfigEdit, EditOp, InstallCtx};
use super::store::{Decision, Update};
use super::{AgentAdapter, Confidence, Detected, Integration};
use gateterm_protocol::AgentState;

pub struct Grok;

const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "StopCancelled",
    "Notification",
    "SessionEnd",
];

/// 显式写上:grok 给 `Stop` / `PostToolUse` 的默认是 600 秒(它们常用来跑测试),
/// 我们的转发连不上 3 秒就放弃,用不着那么久。
const TIMEOUT_SECS: u64 = 10;

fn home_dir(home: &Path) -> PathBuf {
    home.join(".grok")
}

fn hooks_path(home: &Path) -> PathBuf {
    home_dir(home).join("hooks").join("gateterm.json")
}

fn hook_spec(port: u16, event: &str) -> Value {
    json!({
        "type": "command",
        "command": super::hook_cli::command_for("grok", port, event),
        "timeout": TIMEOUT_SECS,
    })
}

fn read_hooks(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn str_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

impl AgentAdapter for Grok {
    fn id(&self) -> &'static str {
        "grok"
    }

    fn name(&self) -> &'static str {
        "Grok"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = home_dir(&env.home);
        let config = dir.join("config.toml");
        // config.toml 是 grok 自己写的,我们从不碰它;hooks/ 可能是我们建的,不算证据
        if config.is_file() {
            return Detected {
                installed: true,
                confidence: Confidence::High,
                reason: "config-file",
                detail: format!("{} 存在", config.display()),
                config_path: Some(hooks_path(&env.home)),
            };
        }
        if let Some(exe) = detect::on_path(env, "grok") {
            return Detected {
                installed: true,
                confidence: Confidence::High,
                reason: "cli-path",
                detail: format!("PATH 上找到 {}", exe.display()),
                config_path: Some(hooks_path(&env.home)),
            };
        }
        if dir.is_dir() && detect::dir_has_entries_besides(&dir, &["hooks"]) {
            return Detected {
                installed: true,
                confidence: Confidence::Medium,
                reason: "parent-dir",
                detail: format!("{} 下有使用痕迹", dir.display()),
                config_path: Some(hooks_path(&env.home)),
            };
        }
        Detected {
            installed: false,
            confidence: Confidence::Low,
            reason: "not-found",
            detail: "没有找到 Grok 的安装痕迹".into(),
            config_path: None,
        }
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        let path = hooks_path(&ctx.home);
        Ok(EVENTS
            .iter()
            .map(|event| ConfigEdit {
                path: path.clone(),
                op: EditOp::EnsureHook {
                    event: event.to_string(),
                    // 不带 matcher:省略即匹配全部,而 Stop / UserPromptSubmit 上带 matcher 会被警告
                    group: json!({ "hooks": [hook_spec(ctx.port, event)] }),
                },
            })
            .collect())
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        let path = hooks_path(&ctx.home);
        Ok(EVENTS
            .iter()
            .map(|event| ConfigEdit { path: path.clone(), op: EditOp::RemoveOurs { event: event.to_string() } })
            .collect())
    }

    /// 新开的 grok 会话直接生效;已经在跑的要在 `/hooks` 里 Reload 或重启。
    fn activation(&self) -> Option<&'static str> {
        Some("grok-reload")
    }

    fn agent_session_id(&self, body: &Value) -> Option<String> {
        str_field(body, "sessionId").or_else(|| str_field(body, "session_id"))
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        // 子代理的会话:不是终端前的那个,不上报(见模块文档第 5 条)
        if str_field(body, "subagentType").is_some() {
            return None;
        }
        let (state, detail) = match event {
            // 会话关闭时附带的那个 Stop,见模块文档第 6 条
            "Stop" if matches!(str_field(body, "reason").as_deref(), Some("shutdown" | "channel_closed")) => {
                return None;
            }
            "SessionStart" | "Stop" | "StopFailure" | "StopCancelled" => (AgentState::Idle, None),
            "UserPromptSubmit" => (AgentState::Thinking, None),
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                (AgentState::Working, str_field(body, "toolName"))
            }
            "SessionEnd" => (AgentState::Done, None),
            // 只认两种:它在等你批准,或者它闲下来等你输入。别的通知(task_complete 等)
            // 不改变「它在干什么」,忽略
            "Notification" => match str_field(body, "notificationType").as_deref() {
                Some("permission_prompt") => (AgentState::Waiting, Some("permission_prompt".into())),
                Some("idle_prompt") => (AgentState::Idle, None),
                _ => return None,
            },
            _ => return None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: str_field(body, "cwd"),
            title: None,
            transcript: str_field(body, "transcript_path").or_else(|| str_field(body, "transcriptPath")),
        })
    }

    fn supports_transcript(&self) -> bool {
        true
    }

    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<super::transcript::Message> {
        super::grok_transcript::parse(text, base_offset)
    }

    fn locate_transcript(&self, home: &Path, agent_session_id: &str) -> Option<PathBuf> {
        super::grok_transcript::locate(home, agent_session_id)
    }

    fn is_blocking(&self, _event: &str) -> bool {
        false
    }

    /// 没有阻塞式事件,永远「无决定」。空对象对 grok 是一个合法的、什么都不说的回复。
    fn render(&self, _decision: &Decision) -> Value {
        json!({})
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let Some(doc) = read_hooks(&hooks_path(&ctx.home)) else {
            return (Integration::NotInstalled, Vec::new());
        };
        let (mut here, mut elsewhere) = (0usize, 0usize);
        for event in EVENTS {
            for hook in hooks_of(&doc, event) {
                if is_ours(hook, ctx.port) {
                    here += 1;
                } else if is_our_shape(hook) {
                    elsewhere += 1;
                }
            }
        }
        let state = if here > 0 {
            Integration::Installed
        } else if elsewhere > 0 {
            Integration::StalePort
        } else {
            Integration::NotInstalled
        };
        // 没有阻塞式事件,谈不上和别人的 hook 抢决定权
        (state, Vec::new())
    }
}

fn is_our_shape(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("http://127.0.0.1:") && c.contains("/hook/grok/"))
}

fn hooks_of<'a>(doc: &'a Value, event: &str) -> impl Iterator<Item = &'a Value> {
    doc.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("hooks"))
        .filter_map(Value::as_array)
        .flatten()
}
