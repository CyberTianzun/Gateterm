//! Claude Code adapter。
//!
//! SessionStart 不支持 HTTP hook,经自身的 hook 子命令转发;其余事件直接 POST
//! 到 gateterm-server。不需要随包携带任何脚本或 node 运行时。
//!
//! 鉴权与会话关联走同一个机制:hook 的 `headers` 支持 `$VAR` 插值,插值发生在
//! hook 触发那一刻、取自 Claude 进程的环境,而那份环境是 gateterm spawn 这个会话
//! 时给的。于是**环境变量就是能力本身** —— 在 gateterm 之外启动的 Claude 拿不到
//! 这两个变量,hook 照样触发,但没有 `Authorization` 头,一律 401 拒收。

pub mod transcript;

use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::agent::detect::DetectEnv;
use crate::agent::edit::{slug_of_event, ConfigEdit, EditOp, InstallCtx};
use crate::agent::history::{self, HistoryRoot, ResumeSpec, ResumeTarget, RowMeta};
use crate::agent::hook_cli::{self, CommandWrap};
use crate::agent::kit::{
    detect_by_spec, json_hooks_integration, render_permission_decision, ConfigEvidence,
    ConflictPolicy, DetectSpec,
};
use crate::agent::store::{Decision, Update};
use crate::agent::{AgentAdapter, Detected, Integration};
use gateterm_protocol::AgentState;

pub struct ClaudeCode;

/// 遥测类:只用来推进会话状态,不参与决策。短超时限制服务不可用时的等待。
const TELEMETRY: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "Notification",
];

/// 决策类:Claude 会阻塞在 socket 上等我们写响应,最长 `timeout` 秒。
const BLOCKING: &[&str] = &["PermissionRequest"];

const TELEMETRY_TIMEOUT_SECS: u64 = 5;
/// 给人类反应时间。这个值同时是「一个挂起请求最长活多久」的上界。
const BLOCKING_TIMEOUT_SECS: u64 = 600;

fn settings_path(home: &std::path::Path) -> PathBuf {
    // 已知缺口:Claude Code 支持用 $CLAUDE_CONFIG_DIR 换掉 ~/.claude。P1 不处理,
    // 因为它会让「装到哪、卸哪个」多出一条要跟着环境变量走的路径。
    home.join(".claude").join("settings.json")
}

fn hook_spec(port: u16, event: &str, blocking: bool) -> Value {
    // Claude skips HTTP SessionStart hooks. Keep this synchronous so startup
    // cannot arrive after UserPromptSubmit and overwrite Thinking with Idle.
    if event == "SessionStart" {
        return json!({
            "type": "command",
            "command": hook_cli::command_for("claude-code", port, event),
            "timeout": TELEMETRY_TIMEOUT_SECS,
        });
    }
    let url = format!("http://127.0.0.1:{port}/hook/claude-code/{}", slug_of_event(event));
    let mut spec = json!({
        "type": "http",
        "url": url,
        "headers": {
            "Authorization": "Bearer $GATETERM_HOOK_TOKEN",
            "X-Gateterm-Session": "$GATETERM_SESSION_ID",
        },
        "allowedEnvVars": ["GATETERM_HOOK_TOKEN", "GATETERM_SESSION_ID"],
    });
    let obj = spec.as_object_mut().expect("字面量就是对象");
    if blocking {
        obj.insert("timeout".into(), json!(BLOCKING_TIMEOUT_SECS));
    } else {
        obj.insert("timeout".into(), json!(TELEMETRY_TIMEOUT_SECS));
        // async is supported only for command hooks. HTTP telemetry returns
        // immediately from our route, without waiting for Cloud delivery.
    }
    spec
}

fn managed_events() -> impl Iterator<Item = (&'static str, bool)> {
    TELEMETRY.iter().map(|e| (*e, false)).chain(BLOCKING.iter().map(|e| (*e, true)))
}

/// 装的时候按开关过滤;**卸载永远覆盖全部事件** —— 否则关掉开关再卸载会留下残留。
fn events_to_install(ctx: &InstallCtx) -> impl Iterator<Item = (&'static str, bool)> + '_ {
    managed_events().filter(move |(_, blocking)| ctx.include_blocking || !blocking)
}

fn read_settings(path: &std::path::Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

impl AgentAdapter for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, env: &DetectEnv) -> Detected {
        let dir = env.home.join(".claude");
        let settings = settings_path(&env.home);
        detect_by_spec(
            env,
            &DetectSpec {
                agent_name: self.name(),
                config_path: settings.clone(),
                evidence: Some(ConfigEvidence::JsonForeign {
                    path: &settings,
                    has_foreign: &has_foreign_content,
                }),
                cli: "claude",
                dir: &dir,
                ignore_in_dir: &["settings.json"],
            },
        )
    }

    fn plan_install(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        let path = settings_path(&ctx.home);
        Ok(events_to_install(ctx)
            .map(|(event, blocking)| ConfigEdit {
                path: path.clone(),
                op: EditOp::EnsureHook {
                    event: event.to_string(),
                    // Claude Code 的 group 带 matcher(空串 = 匹配全部)
                    group: json!({ "matcher": "", "hooks": [hook_spec(ctx.port, event, blocking)] }),
                },
            })
            .collect())
    }

    fn plan_uninstall(&self, ctx: &InstallCtx) -> Result<Vec<ConfigEdit>> {
        let path = settings_path(&ctx.home);
        Ok(managed_events()
            .map(|(event, _)| ConfigEdit {
                path: path.clone(),
                op: EditOp::RemoveOurs { event: event.to_string() },
            })
            .collect())
    }

    fn parse(&self, event: &str, body: &Value) -> Option<Update> {
        let state = state_of(event)?;
        let detail = match event {
            "PreToolUse" | "PostToolUse" => str_field(body, "tool_name"),
            // notification_type 就是 matcher 的取值:permission_prompt / idle_prompt /
            // agent_needs_input / elicitation_dialog ...
            "Notification" => str_field(body, "notification_type").or(Some("需要你的输入".into())),
            _ => None,
        };
        Some(Update {
            state: Some(state),
            detail,
            cwd: str_field(body, "cwd"),
            title: str_field(body, "session_title"),
            // 每个事件都带着它。之前一直读到就扔,现在存下来 —— 聊天视图靠它。
            transcript: str_field(body, "transcript_path"),
        })
    }

    fn supports_transcript(&self) -> bool {
        true
    }

    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<crate::agent::transcript::Message> {
        transcript::parse(text, base_offset)
    }

    fn is_blocking(&self, event: &str) -> bool {
        BLOCKING.contains(&event)
    }

    /// **`decision` 是对象,不是字符串。**
    ///
    /// 调研阶段拿到的二手资料说它是 `"allow"|"deny"|"escalate"|"ask"` 字符串,实测
    /// 不生效 —— 字符串形态下审批弹窗照常出现,换成对象形态后 TUI 立刻打出
    /// `Allowed by PermissionRequest hook`。以实测为准。
    ///
    /// 「无决定」返回空对象:官方文档写明「2xx + 空 body = 成功且无输出」,Claude 会
    /// 回落到它自己的审批弹窗。这正是我们要的降级 —— 把选择权交还给终端前的人。
    fn render(&self, decision: &Decision) -> Value {
        render_permission_decision(decision)
    }

    fn integration(&self, ctx: &InstallCtx) -> (Integration, Vec<String>) {
        let doc = read_settings(&settings_path(&ctx.home));
        json_hooks_integration(
            doc.as_ref(),
            ctx.port,
            &is_our_shape,
            managed_events(),
            ConflictPolicy::ForeignHttpWithUrl,
        )
    }

    fn command_wrap(&self) -> CommandWrap {
        CommandWrap::WindowsPowerShellEncoded
    }

    fn history_roots(&self, home: &std::path::Path) -> Vec<HistoryRoot> {
        vec![HistoryRoot {
            path: home.join(".claude/projects"),
            max_depth: 1,
        }]
    }

    fn inspect_history_row(&self, row: &Value, meta: &mut RowMeta) {
        use history::{model_name, text};
        meta.id = meta
            .id
            .take()
            .or_else(|| text(row, "sessionId"))
            .or_else(|| text(row, "session_id"));
        if row.get("type").and_then(Value::as_str) == Some("custom-title") {
            meta.title = text(row, "customTitle").or(meta.title.take());
        }
        meta.model = model_name(&row["message"]).or(meta.model.take());
    }

    fn finalize_history_entry(&self, path: &std::path::Path, meta: &mut RowMeta) {
        if meta.id.is_none() {
            meta.id = Some(
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }

    fn resume_spec(&self) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            cli: "claude",
            flag: "--resume",
            target: ResumeTarget::SessionId,
        })
    }
}

/// 事件 → 状态。
///
/// `Stop` 映射成 `Idle` 而不是「等你」:一个回合结束不等于它要你做什么。真正的
/// 「在等人」只从 `Notification` 来(它的 matcher 里有 `permission_prompt` /
/// `idle_prompt` / `agent_needs_input`),以及 Task 6 接上的挂起请求。这样
/// `Waiting` 才是个有信息量的状态,而不是每轮都亮一次的噪声。
fn state_of(event: &str) -> Option<AgentState> {
    Some(match event {
        "SessionStart" => AgentState::Idle,
        "SessionEnd" => AgentState::Done,
        "UserPromptSubmit" => AgentState::Thinking,
        "PreToolUse" | "PostToolUse" => AgentState::Working,
        "Stop" => AgentState::Idle,
        "Notification" => AgentState::Waiting,
        _ => return None,
    })
}

fn str_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).map(str::to_string)
}

/// 我方形状但端口对不上 —— 用来识别「改过监听端口之后的残留」。
fn is_our_shape(hook: &Value) -> bool {
    let url = match hook.get("type").and_then(Value::as_str) {
        Some("http") => hook.get("url").and_then(Value::as_str).map(str::to_string),
        Some("command") => hook
            .get("command")
            .and_then(Value::as_str)
            .and_then(crate::agent::edit::url_in_command),
        _ => None,
    };
    url.is_some_and(|u| u.starts_with("http://127.0.0.1:") && u.contains("/hook/claude-code/"))
}

/// 配置里有没有「不是我们写的」内容。只有我方 hook 的文件证明不了 Claude Code 存在。
fn has_foreign_content(doc: &Value) -> bool {
    let Some(obj) = doc.as_object() else { return false };
    if obj.keys().any(|k| k != "hooks") {
        return true;
    }
    let Some(hooks) = obj.get("hooks").and_then(Value::as_object) else { return false };
    hooks.values().any(|groups| {
        groups
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g.get("hooks"))
            .filter_map(Value::as_array)
            .flatten()
            .any(|h| !is_our_shape(h))
    })
}
