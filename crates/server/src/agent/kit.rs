//! 通用 Agent 协议族套件 (Protocol Family Kit)。
//!
//! 提炼多个 CLI Agent 适配器共享的三层安装检测、`hooks.<Event>` JSON 挂钩对账、
//! 以及 Claude/Codex 兼容的 `PermissionRequest` 决策信封渲染。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::detect::{self, DetectEnv};
use super::edit::is_ours;
use super::store::Decision;
use super::{Confidence, Detected, Integration};

/// 配置文件层的「他方证据」。
pub enum ConfigEvidence<'a> {
    /// 只要该文件存在即可视为高置信度证据(必须是 Agent 自己生成且我们不写的文件,如 `~/.codex/config.toml`)。
    FileExists(&'a Path),
    /// 该 JSON 文件是我们也会写的配置文件,只有文件里包含非我方的键值/条目时才算安装证据。
    JsonForeign {
        path: &'a Path,
        has_foreign: &'a dyn Fn(&Value) -> bool,
    },
}

/// 三层安装检测规格:`config-file` -> `cli-path` -> `parent-dir` -> `not-found`。
pub struct DetectSpec<'a> {
    pub agent_name: &'static str,
    pub config_path: PathBuf,
    pub evidence: Option<ConfigEvidence<'a>>,
    pub cli: &'static str,
    pub dir: &'a Path,
    pub ignore_in_dir: &'a [&'a str],
}

pub fn detect_by_spec(env: &DetectEnv, spec: &DetectSpec<'_>) -> Detected {
    if let Some(ev) = &spec.evidence {
        match ev {
            ConfigEvidence::FileExists(p) if p.is_file() => {
                return Detected {
                    installed: true,
                    confidence: Confidence::High,
                    reason: "config-file",
                    detail: format!("{} 存在", p.display()),
                    config_path: Some(spec.config_path.clone()),
                };
            }
            ConfigEvidence::JsonForeign { path, has_foreign } => {
                if let Some(doc) = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    && has_foreign(&doc)
                {
                    return Detected {
                        installed: true,
                        confidence: Confidence::High,
                        reason: "config-file",
                        detail: format!("{} 存在", path.display()),
                        config_path: Some(spec.config_path.clone()),
                    };
                }
            }
            _ => {}
        }
    }
    if let Some(exe) = detect::on_path(env, spec.cli) {
        return Detected {
            installed: true,
            confidence: Confidence::High,
            reason: "cli-path",
            detail: format!("PATH 上找到 {}", exe.display()),
            config_path: Some(spec.config_path.clone()),
        };
    }
    if spec.dir.is_dir() && detect::dir_has_entries_besides(spec.dir, spec.ignore_in_dir) {
        return Detected {
            installed: true,
            confidence: Confidence::Medium,
            reason: "parent-dir",
            detail: format!("{} 下有使用痕迹", spec.dir.display()),
            config_path: Some(spec.config_path.clone()),
        };
    }
    Detected {
        installed: false,
        confidence: Confidence::Low,
        reason: "not-found",
        detail: format!("没有找到 {} 的安装痕迹", spec.agent_name),
        config_path: None,
    }
}

/// 阻塞事件上发现他方 hook 时的冲突判定策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// 不报告冲突(如无阻塞事件的遥测型 Agent)。
    None,
    /// 仅当他方 hook 是 `type == "http"` 时报告 URL 冲突(Claude Code)。
    ForeignHttpWithUrl,
    /// 只要阻塞事件上有任何他方 hook 即报告冲突(Codex)。
    AnyForeign,
}

/// 遍历 `{"hooks": {"<event>": [{"hooks": [...]}]}}` 结构下某个事件的所有内层 hook 对象。
pub fn hooks_of<'a>(doc: &'a Value, event: &str) -> impl Iterator<Item = &'a Value> {
    doc.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("hooks").and_then(Value::as_array))
        .flatten()
}

/// 针对 `hooks.<Event>` JSON 结构的统一集成状态与冲突扫描。
pub fn json_hooks_integration<'a>(
    doc: Option<&Value>,
    port: u16,
    is_our_shape: &dyn Fn(&Value) -> bool,
    events: impl IntoIterator<Item = (&'a str, bool)>,
    conflict_policy: ConflictPolicy,
) -> (Integration, Vec<String>) {
    let Some(doc) = doc else {
        return (Integration::NotInstalled, Vec::new());
    };
    let mut here = 0usize;
    let mut elsewhere = 0usize;
    let mut conflicts = Vec::new();
    for (event, blocking) in events {
        for hook in hooks_of(doc, event) {
            if is_ours(hook, port) {
                here += 1;
            } else if is_our_shape(hook) {
                elsewhere += 1;
            } else if blocking {
                match conflict_policy {
                    ConflictPolicy::None => {}
                    ConflictPolicy::ForeignHttpWithUrl => {
                        if hook.get("type").and_then(Value::as_str) == Some("http") {
                            let url = hook.get("url").and_then(Value::as_str).unwrap_or("?");
                            conflicts.push(format!("{event} 上已有别的 HTTP hook: {url}"));
                        }
                    }
                    ConflictPolicy::AnyForeign => {
                        conflicts.push(format!("{event} 上已有别的程序的 hook"));
                    }
                }
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
    (state, conflicts)
}

/// Claude Code 与 Codex 共用的 `PermissionRequest` 决策响应信封。
pub fn render_permission_decision(decision: &Decision) -> Value {
    let d = match decision {
        Decision::NoDecision => return json!({}),
        Decision::Allow { message, updated_input } => {
            let mut o = json!({ "behavior": "allow" });
            if let Some(m) = message {
                o["message"] = json!(m);
            }
            if let Some(u) = updated_input {
                o["updatedInput"] = u.clone();
            }
            o
        }
        Decision::Deny { message } => {
            let mut o = json!({ "behavior": "deny" });
            if let Some(m) = message {
                o["message"] = json!(m);
            }
            o
        }
    };
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": d,
        }
    })
}

/// 从 JSON 对象中提取非空字符串字段。
pub fn str_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_by_spec_prioritizes_config_then_cli_then_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let dir = home.join(".demo");
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        let hooks = dir.join("hooks.json");
        let env = DetectEnv { home: home.to_path_buf(), path: None };

        let spec = DetectSpec {
            agent_name: "Demo",
            config_path: hooks.clone(),
            evidence: Some(ConfigEvidence::FileExists(&cfg)),
            cli: "demo-cli",
            dir: &dir,
            ignore_in_dir: &["hooks.json"],
        };
        assert!(!detect_by_spec(&env, &spec).installed);

        std::fs::write(dir.join("session.jsonl"), "{}").unwrap();
        let d = detect_by_spec(&env, &spec);
        assert_eq!((d.installed, d.confidence, d.reason), (true, Confidence::Medium, "parent-dir"));

        std::fs::write(&cfg, "").unwrap();
        let d = detect_by_spec(&env, &spec);
        assert_eq!((d.installed, d.confidence, d.reason), (true, Confidence::High, "config-file"));
    }

    #[test]
    fn render_permission_decision_matches_claude_and_codex_envelope() {
        assert_eq!(render_permission_decision(&Decision::NoDecision), json!({}));
        let allow = render_permission_decision(&Decision::Allow {
            message: Some("ok".into()),
            updated_input: Some(json!({"a": 1})),
        });
        assert_eq!(
            allow,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow", "message": "ok", "updatedInput": {"a": 1} }
                }
            })
        );
    }
}
