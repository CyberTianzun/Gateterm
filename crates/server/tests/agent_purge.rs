//! 清空(purge)与启动自检。
//!
//! 全部用 tempfile 造假 home,不碰真实的 `~/.claude` / `~/.codex`。

use octoterm_server::agent::apply::{apply, ApplyOpts};
use octoterm_server::agent::detect::DetectEnv;
use octoterm_server::agent::edit::InstallCtx;
use octoterm_server::agent::{find, plan_purge, scan, startup_warnings, Integration};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

struct Env {
    home: tempfile::TempDir,
    backups: tempfile::TempDir,
}

fn env() -> Env {
    Env { home: tempfile::tempdir().unwrap(), backups: tempfile::tempdir().unwrap() }
}

impl Env {
    fn settings(&self) -> PathBuf {
        self.home.path().join(".claude").join("settings.json")
    }
    fn codex_hooks(&self) -> PathBuf {
        self.home.path().join(".codex").join("hooks.json")
    }
    fn seed(&self, path: &Path, v: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
    }
    fn ctx(&self, port: u16) -> InstallCtx {
        InstallCtx { home: self.home.path().to_path_buf(), port, include_blocking: true }
    }
    fn detect_env(&self) -> DetectEnv {
        DetectEnv { home: self.home.path().to_path_buf(), path: None }
    }
    fn opts(&self) -> ApplyOpts {
        ApplyOpts { enabled: true, backup_dir: self.backups.path().to_path_buf(), backup_keep: 5 }
    }
    fn install(&self, id: &str, port: u16) {
        let a = find(id).unwrap();
        apply(&a.plan_install(&self.ctx(port)).unwrap(), &self.opts()).unwrap();
    }
    fn purge(&self, id: &str) {
        let a = find(id).unwrap();
        apply(&plan_purge(a.as_ref(), &self.ctx(0)).unwrap(), &self.opts()).unwrap();
    }
    fn integration(&self, id: &str, port: u16) -> Integration {
        scan(&self.detect_env(), port).into_iter().find(|a| a.id == id).unwrap().integration
    }
    fn read(&self, path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }
}

fn ours(port: u16, event: &str) -> Value {
    json!({ "type": "http", "url": format!("http://127.0.0.1:{port}/hook/claude-code/{event}") })
}

/// 卸载够不着的残留:旧端口、当前版本已不托管的事件 —— 清空都得清掉,
/// 而用户自己的 hook 一条都不能碰。
#[test]
fn purge_removes_ours_on_any_event_and_port_but_keeps_foreign() {
    let e = env();
    let user_hook = json!({ "type": "command", "command": "echo hi" });
    e.seed(
        &e.settings(),
        &json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "matcher": "", "hooks": [ours(1111, "stop"), user_hook.clone()] }],
                // 当前版本不托管的事件,卸载不会看这里
                "PreCompact": [{ "matcher": "", "hooks": [ours(2222, "pre-compact")] }],
                "SubagentStop": [{ "matcher": "", "hooks": [ours(7683, "subagent-stop")] }],
            }
        }),
    );
    e.purge("claude-code");
    assert_eq!(
        e.read(&e.settings()),
        json!({ "model": "opus", "hooks": { "Stop": [{ "matcher": "", "hooks": [user_hook] }] } })
    );
}

#[test]
fn purge_after_install_leaves_no_trace() {
    let e = env();
    e.seed(&e.settings(), &json!({ "model": "opus" }));
    e.install("claude-code", 7683);
    e.install("claude-code", 9000);
    e.purge("claude-code");
    assert_eq!(e.read(&e.settings()), json!({ "model": "opus" }));
}

/// 没装过的 agent 上点「清空」/「卸载」,不能凭空造出它的配置文件。
#[test]
fn purge_and_uninstall_never_create_missing_files() {
    let e = env();
    e.purge("codex");
    let a = find("codex").unwrap();
    apply(&a.plan_uninstall(&e.ctx(7683)).unwrap(), &e.opts()).unwrap();
    assert!(!e.codex_hooks().exists());
    assert!(!e.codex_hooks().parent().unwrap().exists());
}

#[test]
fn fresh_install_is_current() {
    let e = env();
    e.install("claude-code", 7683);
    assert_eq!(e.integration("claude-code", 7683), Integration::Installed);
    assert!(startup_warnings(&e.detect_env(), 7683).is_empty());
}

#[test]
fn whole_install_on_another_port_is_stale_port_and_warns() {
    let e = env();
    e.install("claude-code", 9000);
    assert_eq!(e.integration("claude-code", 7683), Integration::StalePort);
    let w = startup_warnings(&e.detect_env(), 7683);
    assert_eq!(w.len(), 1);
    assert!(w[0].contains("Claude Code") && w[0].contains("7683"), "{w:?}");
}

/// 当前端口上有,但缺了事件 —— 以前会被报成「已安装」,实际部分事件静默失灵。
#[test]
fn missing_event_is_outdated() {
    let e = env();
    e.install("claude-code", 7683);
    let mut doc = e.read(&e.settings());
    doc["hooks"].as_object_mut().unwrap().shift_remove("Stop");
    e.seed(&e.settings(), &doc);
    assert_eq!(e.integration("claude-code", 7683), Integration::Outdated);
    assert_eq!(startup_warnings(&e.detect_env(), 7683).len(), 1);
}

/// 当前端口装好了,但还混着旧端口的残留:那些条目会打到别的端口上去。
#[test]
fn stray_hook_on_old_port_is_outdated() {
    let e = env();
    e.install("claude-code", 7683);
    let mut doc = e.read(&e.settings());
    doc["hooks"]["PreCompact"] = json!([{ "matcher": "", "hooks": [ours(1111, "pre-compact")] }]);
    e.seed(&e.settings(), &doc);
    assert_eq!(e.integration("claude-code", 7683), Integration::Outdated);
}

/// 别的程序在我们之后往同一个事件追加了 hook —— 顺序变了,但我们的那条完好,
/// 不能误报成不一致。
#[test]
fn foreign_hook_appended_after_ours_is_still_current() {
    let e = env();
    e.install("claude-code", 7683);
    let mut doc = e.read(&e.settings());
    doc["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "matcher": "", "hooks": [{ "type": "command", "command": "say done" }] }));
    e.seed(&e.settings(), &doc);
    assert_eq!(e.integration("claude-code", 7683), Integration::Installed);
}

/// Codex 的命令里带着二进制路径。路径换了(比如从 desktop 换成独立 server),
/// 端口对得上也算不一致。
#[test]
fn codex_command_with_other_binary_is_outdated() {
    let e = env();
    e.install("codex", 7683);
    // 没在 Codex 里审核过,所以是「待放行」而不是「已装」(见 agent_codex_trust.rs)
    assert_eq!(e.integration("codex", 7683), Integration::NeedsActivation);
    let text = fs::read_to_string(e.codex_hooks()).unwrap();
    let exe = std::env::current_exe().unwrap().to_string_lossy().replace('\\', "\\\\");
    fs::write(e.codex_hooks(), text.replace(&exe, "/old/place/octoterm-server")).unwrap();
    assert_eq!(e.integration("codex", 7683), Integration::Outdated);
}

#[test]
fn not_installed_does_not_warn() {
    let e = env();
    e.seed(&e.settings(), &json!({ "model": "opus" }));
    assert!(startup_warnings(&e.detect_env(), 7683).is_empty());
}
