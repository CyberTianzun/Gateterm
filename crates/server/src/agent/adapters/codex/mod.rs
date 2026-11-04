//! Codex adapter。
//!
//! 与 Claude Code 的三处关键差异,都是实测出来的:
//!
//! 1. **Codex 不支持 `type: "http"`**。它的 `HookHandlerConfig` 只有
//!    command / prompt / agent 三个变体(从 codex 二进制的类型名读出来)。所以这里的
//!    hook 指向 gateterm 自己的二进制(`hook <url>` 子命令,见 [`crate::agent::hook_cli`]),
//!    由它转发到本地 server。
//! 2. **配置在 `~/.codex/hooks.json`**,不在 config.toml 里,而且 group **不带
//!    `matcher`**(本机上被 Codex 接受并信任过的那份就是这个形状)。
//! 3. **装了不等于生效**:Codex 用 `trusted_hash` 逐条门控 ——
//!    `~/.codex/config.toml` 里长这样:
//!
//!    ```toml
//!    [hooks.state."/Users/x/.codex/hooks.json:pre_tool_use:0:0"]
//!    trusted_hash = "sha256:..."
//!    ```
//!
//!    用户必须在 Codex 的 TUI 里跑 `/hooks` review 之后,那些条目才会被写进去。
//!    **我们绝不替它写**:那是 Codex 有意设的闸,防的正是「第三方悄悄让 Codex 执行
//!    任意命令」。我们只负责把 hook 写好,并把这一步告诉用户。
//!
//!    没审核过的 hook 会被 Codex **静默跳过**,所以「装了」≠「生效」,报成
//!    `NeedsActivation`。重装、换端口、换二进制位置都会改 hook 内容,让之前的审核失效。
//!    怎么判断见 [`Codex::activated`]。

pub mod transcript;
pub mod trust;

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::agent::detect::{self, DetectEnv};
use crate::agent::edit::{is_ours, slug_of_event, ConfigEdit, EditOp, InstallCtx};
use crate::agent::history::{self, HistoryRoot, ResumeSpec, ResumeTarget, RowMeta};
use crate::agent::hook_cli::{self, CommandWrap};
use crate::agent::kit::{
    detect_by_spec, json_hooks_integration, render_permission_decision, str_field, ConfigEvidence,
    ConflictPolicy, DetectSpec,
};
use crate::agent::process::ProcessWatchSpec;
use crate::agent::store::{Decision, Update};
use crate::agent::{AgentAdapter, Detected, Integration};
use gateterm_protocol::AgentState;

pub struct Codex;

/// Interrupt 只结束当前回合;SessionEnd 才结束会话。漏装前者会在 Ctrl+C 后卡在
/// working,漏装后者会在退出后继续显示聊天入口。
const TELEMETRY: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "Interrupt",
];
const BLOCKING: &[&str] = &["PermissionRequest"];

const TELEMETRY_TIMEOUT_SECS: u64 = 30;
const BLOCKING_TIMEOUT_SECS: u64 = 600;

fn home_dir(home: &Path) -> PathBuf {
    home.join(".codex")
}

fn hooks_path(home: &Path) -> PathBuf {
    home_dir(home).join("hooks.json")
}

fn hook_spec(port: u16, event: &str, blocking: bool) -> Value {
    // Codex 对退出/中断 hook 的超时上限是 3 秒,不能沿用普通遥测的 30 秒。
    let timeout = match event {
        "SessionEnd" | "Interrupt" => 3,
        _ if blocking => BLOCKING_TIMEOUT_SECS,
        _ => TELEMETRY_TIMEOUT_SECS,
    };
    json!({
        "type": "command",
        "command": hook_cli::command_for("codex", port, event),
        "timeout": timeout,
    })
}

fn managed_events() -> impl Iterator<Item = (&'static str, bool)> {
    TELEMETRY.iter().map(|e| (*e, false)).chain(BLOCKING.iter().map(|e| (*e, true)))
}

fn events_to_install(ctx: &InstallCtx) -> impl Iterator<Item = (&'static str, bool)> + '_ {
    managed_events().filter(move |(_, blocking)| ctx.include_blocking || !blocking)
}

fn read_hooks(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn state_of(event: &str) -> Option<AgentState> {
    Some(match event {
        "SessionStart" => AgentState::Idle,
        "SessionEnd" => AgentState::Done,
        "UserPromptSubmit" => AgentState::Thinking,
        "PreToolUse" | "PostToolUse" => AgentState::Working,
        "Stop" | "Interrupt" => AgentState::Idle,
        _ => return None,
    })
}

impl AgentAdapter for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn name(&self) -> &'static str {
        "Codex"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = home_dir(&env.home);
        let config = dir.join("config.toml");
        // `~/.codex/config.toml` 是 Codex 自己写的,我们从不碰它 —— 它存在就是硬证据。
        // (对比 Claude Code:那边 `~/.claude/` 是我们也会写的,所以不能这么判。)
        detect_by_spec(
            env,
            &DetectSpec {
                agent_name: self.name(),
                config_path: hooks_path(&env.home),
                evidence: Some(ConfigEvidence::FileExists(&config)),
                cli: "codex",
                dir: &dir,
                ignore_in_dir: &["hooks.json"],
            },
        )
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        let path = hooks_path(&ctx.home);
        Ok(events_to_install(ctx)
            .map(|(event, blocking)| ConfigEdit {
                path: path.clone(),
                op: EditOp::EnsureHook {
                    event: event.to_string(),
                    // Codex 的 group 不带 matcher:本机上被它接受并信任过的那份就是这个形状
                    group: json!({ "hooks": [hook_spec(ctx.port, event, blocking)] }),
                },
            })
            .collect())
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        let path = hooks_path(&ctx.home);
        Ok(managed_events()
            .map(|(event, _)| ConfigEdit {
                path: path.clone(),
                op: EditOp::RemoveOurs { event: event.to_string() },
            })
            .collect())
    }

    fn supports_transcript(&self) -> bool {
        true
    }

    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<crate::agent::transcript::Message> {
        transcript::parse(text, base_offset)
    }

    fn locate_transcript(
        &self,
        home: &std::path::Path,
        agent_session_id: &str,
    ) -> Option<std::path::PathBuf> {
        transcript::locate(home, agent_session_id)
    }

    fn activation(&self) -> Option<&'static str> {
        Some("codex-hooks-review")
    }

    fn activation_hint(&self) -> Option<&'static str> {
        Some("在 gateterm 里启动 codex,执行 /hooks 逐条审核通过,再重启 codex")
    }

    /// 先问 Codex 自己(`hooks/list`,见 [`trust`]),那是权威答案。
    /// 问不了(没在 PATH 上、超时、协议变了)才回落到读 config.toml 的粗判断。
    fn activated(&self, env: &DetectEnv, ctx: &InstallCtx) -> bool {
        let hooks = hooks_path(&ctx.home);
        if let (Some(exe), Some(path)) = (detect::on_path(env, "codex"), env.path.as_deref())
            && let Some(ok) = trust::query(&exe, path, &home_dir(&ctx.home), &hooks, ctx.port)
        {
            return ok;
        }
        trusted_by_config(&hooks, &home_dir(&ctx.home).join("config.toml"), ctx.port)
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        let state = state_of(event)?;
        let detail = match event {
            "PreToolUse" | "PostToolUse" => {
                body.get("tool_name").and_then(Value::as_str).map(str::to_string)
            }
            _ => None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: str_field(body, "cwd"),
            title: None,
            // Codex 0.166+ 的 hook payload 可直接带 transcript_path(可能为 null);
            // 为 null 或旧版本缺省时再由 session_id 推导(见 C3)。
            transcript: str_field(body, "transcript_path"),
        })
    }

    fn is_blocking(&self, event: &str) -> bool {
        BLOCKING.contains(&event)
    }

    /// 与 Claude Code 同一个信封 —— 参考实现对这两家用的就是同一个 sanitizer,
    /// 而 `decision` 是**对象**这一点在 Claude Code 上已实测确认。
    fn render(&self, decision: &Decision) -> Value {
        render_permission_decision(decision)
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let doc = read_hooks(&hooks_path(&ctx.home));
        json_hooks_integration(
            doc.as_ref(),
            ctx.port,
            &is_our_shape,
            managed_events(),
            ConflictPolicy::AnyForeign,
        )
    }

    fn command_wrap(&self) -> CommandWrap {
        CommandWrap::WindowsPowerShellCall
    }

    fn process_watch(&self) -> Option<ProcessWatchSpec> {
        Some(ProcessWatchSpec {
            exe_names: &["codex", "codex.exe"],
            exclude_subcommands: &["app-server", "exec-server", "mcp-server"],
        })
    }

    fn history_roots(&self, home: &Path) -> Vec<HistoryRoot> {
        vec![
            HistoryRoot {
                path: home.join(".codex/sessions"),
                max_depth: 3,
            },
            HistoryRoot {
                path: home.join(".codex/archived_sessions"),
                max_depth: 0,
            },
        ]
    }

    fn inspect_history_row(&self, row: &Value, meta: &mut RowMeta) {
        use history::{model_name, prompt, text};
        if meta.id.is_none() && row.get("type").and_then(Value::as_str) == Some("session_meta") {
            let payload = &row["payload"];
            meta.id = text(payload, "id")
                .or_else(|| text(payload, "session_id"))
                .or(meta.id.take());
            meta.cwd = text(payload, "cwd").or(meta.cwd.take());
            meta.title = text(payload, "title")
                .or_else(|| text(payload, "thread_name"))
                .or(meta.title.take());
        }
        if row.get("type").and_then(Value::as_str) == Some("turn_context") {
            meta.model = model_name(&row["payload"]).or(meta.model.take());
        }
        // Older Codex event-only records have no response_item counterpart.
        if row["type"] == "event_msg" {
            let payload = &row["payload"];
            if matches!(
                payload["type"].as_str(),
                Some("user_message" | "agent_message")
            ) && let Some(value) = text(payload, "message").and_then(|s| prompt(&s))
            {
                if payload["type"] == "user_message" && meta.first_prompt.is_none() {
                    meta.first_prompt = Some(value.clone());
                }
                meta.description = Some(value);
            }
        }
    }

    fn enrich_history_catalog(&self, home: &Path, entries: &mut [history::Entry]) {
        use history::{prompt, sample_with_limits, short, text};
        use std::collections::HashMap;
        // Codex /rename is persisted in its index, independently of the rollout transcript.
        if let Ok(index) = sample_with_limits(
            &home.join(".codex/session_index.jsonl"),
            1024 * 1024,
            1024 * 1024,
        ) {
            let titles: HashMap<_, _> = index
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter_map(|v| Some((text(&v, "id")?, text(&v, "thread_name")?)))
                .collect();
            for entry in entries.iter_mut().filter(|e| e.agent_id == "codex") {
                if let Some(title) = titles.get(&entry.agent_session_id) {
                    entry.title = short(title);
                }
            }
        }
        // CLI prompt history can recover an opening request when a very large
        // bootstrap record lies beyond the transcript sampling budget.
        if let Ok(index) =
            sample_with_limits(&home.join(".codex/history.jsonl"), 0, 2 * 1024 * 1024)
        {
            let mut prompts = HashMap::new();
            for row in index
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            {
                if let (Some(id), Some(value)) = (
                    text(&row, "session_id"),
                    text(&row, "text").and_then(|s| prompt(&s)),
                ) {
                    prompts.entry(id).or_insert(value);
                }
            }
            for entry in entries
                .iter_mut()
                .filter(|e| e.agent_id == "codex" && e.title.is_empty())
            {
                if let Some(value) = prompts.get(&entry.agent_session_id) {
                    entry.title = value.clone();
                    if entry.description.is_none() {
                        entry.description = Some(value.clone());
                    }
                }
            }
        }
    }

    fn resume_spec(&self) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            cli: "codex",
            flag: "resume",
            target: ResumeTarget::SessionId,
        })
    }
}

/// 回落用的粗判断:只读 Codex 自己写的 config.toml。
///
/// 两个条件都满足才算放行:我们的每一条 hook 在 `hooks.state` 里都有带 `trusted_hash`
/// 的键;hooks.json 没有在 config.toml 最后一次写入之后被改过(改过 = 旧哈希大概率对不上)。
/// **会漏报**:审核之后 Codex 又因为别的原因(比如换模型)重写了 config.toml,内容已变
/// 的 hook 就看不出来 —— 所以它只是问不到 Codex 时的退路。
fn trusted_by_config(hooks: &Path, config: &Path, port: u16) -> bool {
    let Some(doc) = read_hooks(hooks) else { return true };
    // 审核之后 hooks.json 又被改过:旧的 trusted_hash 大概率已经对不上
    if let (Some(h), Some(c)) = (mtime(hooks), mtime(config))
        && h > c
    {
        return false;
    }
    let trusted = read_trusted_keys(config);
    let prefix = hooks.to_string_lossy();
    managed_events().all(|(event, _)| {
        let snake = slug_of_event(event).replace('-', "_");
        groups_of(&doc, event).enumerate().all(|(gi, group)| {
            group.iter().enumerate().filter(|(_, h)| is_ours(h, port)).all(|(hi, _)| {
                trusted.contains(&format!("{prefix}:{snake}:{gi}:{hi}"))
            })
        })
    })
}

fn is_our_shape(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("http://127.0.0.1:") && c.contains("/hook/codex/"))
}

/// 每个 group 的 hook 列表,**保留 group 下标** —— Codex 的信任键就是按
/// `<event>:<group 下标>:<hook 下标>` 记的。坏掉的 group 记成空列表,不吞掉下标。
fn groups_of<'a>(doc: &'a Value, event: &str) -> impl Iterator<Item = &'a Vec<Value>> {
    static EMPTY: Vec<Value> = Vec::new();
    doc.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|g| g.get("hooks").and_then(Value::as_array).unwrap_or(&EMPTY))
}

fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// `~/.codex/config.toml` 里 `[hooks.state."<键>"]` 带 `trusted_hash` 的那些键。只读。
fn read_trusted_keys(config: &Path) -> Vec<String> {
    let Some(doc) = std::fs::read_to_string(config)
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
    else {
        return Vec::new();
    };
    doc.get("hooks")
        .and_then(|h| h.get("state"))
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.get("trusted_hash").is_some())
        .map(|(k, _)| k.clone())
        .collect()
}
