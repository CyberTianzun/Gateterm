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
//!    开的每个 grok 会话都会满屏报错。command 走 [`crate::agent::hook_cli`]:环境里没有我们的
//!    变量就静默退出,什么都不发。
//! 3. **payload 是驼峰的**:`sessionId`、`toolName`、`notificationType`。另外也带着
//!    Claude 兼容的 `session_id` 和 `transcript_path`(实测),记录路径直接从这里拿。
//! 4. **没有授权审批事件**。`PreToolUse` 回 allow 只表示「不拦」,不会替用户批准,
//!    所以只上报状态和聊天记录(见 [`transcript`]),不做阻塞式事件。
//! 5. **子代理有自己的会话**,事件带 `subagentType`。这些事件不上报:它们不是终端前的
//!    那个会话,混进来会在列表上多出一条、聊天视图也会看错对象。
//! 6. **会话结束时还会再来一个 `Stop`**,`reason` 是 `shutdown` / `channel_closed`(正常
//!    回合结束是 `end_turn`),而且 hook 是并发跑的,它可能晚于 `SessionEnd` 到达。不忽略
//!    它的话,刚结束的会话会被它重新建回来、一直挂在列表上(实测发生过)。
//!
//! 它也会读 `~/.claude/settings.json` 里的 hook(Claude Code 兼容)。如果同时装了
//! Claude Code 集成,grok 会以 Claude 的身份打过来:HTTP 那几条丢了 `headers`、不带
//! 密钥;但 SessionStart 是 command hook,继承了终端环境,**带着合法密钥**。所以
//! claude-code adapter 靠 payload 里 grok 独有的 `hookEventName` 拒收它
//! (见 `ClaudeCode::accepts_payload`),不会误报成 Claude 会话。

pub mod transcript;

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::agent::detect::DetectEnv;
use crate::agent::edit::{ConfigEdit, EditOp, InstallCtx};
use crate::agent::history::{self, HistoryRoot, ResumeSpec, ResumeTarget, RowMeta};
use crate::agent::hook_cli;
use crate::agent::kit::{
    detect_by_spec, json_hooks_integration, str_field, ConfigEvidence, ConflictPolicy, DetectSpec,
};
use crate::agent::store::{Decision, Update};
use crate::agent::{AgentAdapter, Detected, Integration};
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
        "command": hook_cli::command_for("grok", port, event),
        "timeout": TIMEOUT_SECS,
    })
}

fn read_hooks(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
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
        detect_by_spec(
            env,
            &DetectSpec {
                agent_name: self.name(),
                config_path: hooks_path(&env.home),
                evidence: Some(ConfigEvidence::FileExists(&config)),
                cli: "grok",
                dir: &dir,
                ignore_in_dir: &["hooks"],
            },
        )
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

    fn activation_hint(&self) -> Option<&'static str> {
        Some("在 grok 里执行 /hooks 重新加载或重启 grok")
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

    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<crate::agent::transcript::Message> {
        transcript::parse(text, base_offset)
    }

    fn locate_transcript(&self, home: &Path, agent_session_id: &str) -> Option<PathBuf> {
        transcript::locate(home, agent_session_id)
    }

    fn is_blocking(&self, _event: &str) -> bool {
        false
    }

    /// 没有阻塞式事件,永远「无决定」。空对象对 grok 是一个合法的、什么都不说的回复。
    fn render(&self, _decision: &Decision) -> Value {
        json!({})
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let doc = read_hooks(&hooks_path(&ctx.home));
        json_hooks_integration(
            doc.as_ref(),
            ctx.port,
            &is_our_shape,
            EVENTS.iter().map(|e| (*e, false)),
            ConflictPolicy::None,
        )
    }

    fn history_roots(&self, home: &Path) -> Vec<HistoryRoot> {
        vec![HistoryRoot {
            path: home.join(".grok/sessions"),
            max_depth: 2,
        }]
    }

    fn accept_history_file(&self, file_name: &str) -> bool {
        file_name == "updates.jsonl"
    }

    fn inspect_history_row(&self, row: &Value, meta: &mut RowMeta) {
        if meta.id.is_none() {
            meta.id = row
                .pointer("/params/sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }

    fn finalize_history_entry(&self, path: &Path, meta: &mut RowMeta) {
        use history::{model_name, sample, text};
        // Grok keeps cwd/title alongside the ACP update log.
        if let Some(dir) = path.parent() {
            for name in ["session.json", "metadata.json", "meta.json"] {
                if let Some(v) = sample(&dir.join(name))
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                {
                    meta.cwd = text(&v, "cwd").or(meta.cwd.take());
                    meta.title = text(&v, "title").or(meta.title.take());
                    meta.model = model_name(&v).or(meta.model.take());
                }
            }
            meta.id = meta
                .id
                .take()
                .or_else(|| dir.file_name().map(|s| s.to_string_lossy().into_owned()));
            // Older Grok stores have no metadata file; the project directory is URL encoded.
            if meta.cwd.is_none() {
                meta.cwd = dir
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|name| decode_project(&name.to_string_lossy()));
            }
        }
    }

    fn resume_spec(&self) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            cli: "grok",
            flag: "--resume",
            target: ResumeTarget::SessionId,
        })
    }
}

fn decode_project(name: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut index = 0;
    while index < name.len() {
        if name.as_bytes()[index] == b'%' {
            let value = u8::from_str_radix(name.get(index + 1..index + 3)?, 16).ok()?;
            bytes.push(value);
            index += 3;
        } else {
            bytes.push(name.as_bytes()[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    Path::new(&path).is_absolute().then_some(path)
}

fn is_our_shape(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("http://127.0.0.1:") && c.contains("/hook/grok/"))
}
