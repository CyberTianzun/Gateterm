//! Codex 的 hook 信任门控:装了、但还没在 Codex 里 `/hooks` 审核过 —— Codex 会静默
//! 跳过这些 hook,octoterm 什么都收不到。必须报成 `NeedsActivation`,而不是 `Installed`。
//!
//! 全部用 tempfile 造假 home,不碰真实的 `~/.codex`。

use octoterm_server::agent::apply::{apply, ApplyOpts};
use octoterm_server::agent::detect::DetectEnv;
use octoterm_server::agent::edit::InstallCtx;
use octoterm_server::agent::{find, scan, startup_warnings, Integration};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

struct Env {
    home: tempfile::TempDir,
    backups: tempfile::TempDir,
}

fn env() -> Env {
    Env { home: tempfile::tempdir().unwrap(), backups: tempfile::tempdir().unwrap() }
}

impl Env {
    fn hooks(&self) -> PathBuf {
        self.home.path().join(".codex").join("hooks.json")
    }
    fn config(&self) -> PathBuf {
        self.home.path().join(".codex").join("config.toml")
    }
    fn install(&self, port: u16) {
        let ctx = InstallCtx { home: self.home.path().to_path_buf(), port, include_blocking: true };
        let opts =
            ApplyOpts { enabled: true, backup_dir: self.backups.path().to_path_buf(), backup_keep: 5 };
        apply(&find("codex").unwrap().plan_install(&ctx).unwrap(), &opts).unwrap();
    }
    fn detect_env(&self) -> DetectEnv {
        DetectEnv { home: self.home.path().to_path_buf(), path: None }
    }
    fn integration(&self, port: u16) -> Integration {
        scan(&self.detect_env(), port).into_iter().find(|a| a.id == "codex").unwrap().integration
    }
    /// 模拟用户在 Codex 里跑过 `/hooks`:hooks.json 里每一条都记一个 trusted_hash,
    /// `skip` 里的事件除外。键的形状照抄 Codex 写进 config.toml 的样子。
    fn trust_all_except(&self, skip: &[&str]) {
        let doc: Value = serde_json::from_str(&fs::read_to_string(self.hooks()).unwrap()).unwrap();
        let mut out = String::from("model = \"x\"\n\n[hooks.state]\n");
        for (event, groups) in doc["hooks"].as_object().unwrap() {
            if skip.contains(&event.as_str()) {
                continue;
            }
            let snake = snake(event);
            for (gi, g) in groups.as_array().unwrap().iter().enumerate() {
                for hi in 0..g["hooks"].as_array().unwrap().len() {
                    out += &format!(
                        "\n[hooks.state.\"{}:{snake}:{gi}:{hi}\"]\ntrusted_hash = \"sha256:00\"\n",
                        self.hooks().display()
                    );
                }
            }
        }
        fs::write(self.config(), out).unwrap();
    }
    fn set_mtime(&self, path: &PathBuf, t: SystemTime) {
        fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }
}

fn snake(event: &str) -> String {
    let mut s = String::new();
    for (i, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() && i != 0 {
            s.push('_');
        }
        s.push(c.to_ascii_lowercase());
    }
    s
}

#[test]
fn freshly_installed_without_review_needs_activation() {
    let e = env();
    e.install(7683);
    assert_eq!(e.integration(7683), Integration::NeedsActivation);
    let w = startup_warnings(&e.detect_env(), 7683);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("/hooks"), "{}", w[0]);
}

#[test]
fn reviewed_after_install_is_installed() {
    let e = env();
    e.install(7683);
    e.trust_all_except(&[]);
    let now = SystemTime::now();
    e.set_mtime(&e.hooks(), now - Duration::from_secs(60));
    e.set_mtime(&e.config(), now);
    assert_eq!(e.integration(7683), Integration::Installed);
    assert!(startup_warnings(&e.detect_env(), 7683).is_empty());
}

/// 用户本机的真实情形:以前审核过(键都在),之后我们重装改了命令内容 ——
/// 旧的 trusted_hash 对不上新内容,Codex 照样跳过。hooks.json 比 config.toml 新就是信号。
#[test]
fn hooks_rewritten_after_review_needs_activation() {
    let e = env();
    e.install(7683);
    e.trust_all_except(&[]);
    let now = SystemTime::now();
    e.set_mtime(&e.config(), now - Duration::from_secs(60));
    e.set_mtime(&e.hooks(), now);
    assert_eq!(e.integration(7683), Integration::NeedsActivation);
}

#[test]
fn one_unreviewed_event_needs_activation() {
    let e = env();
    e.install(7683);
    e.trust_all_except(&["PermissionRequest"]);
    let now = SystemTime::now();
    e.set_mtime(&e.hooks(), now - Duration::from_secs(60));
    e.set_mtime(&e.config(), now);
    assert_eq!(e.integration(7683), Integration::NeedsActivation);
}

/// 端口 / 路径对不上时要的是重装,不是审核 —— `Outdated` / `StalePort` 优先。
#[test]
fn stale_port_wins_over_needs_activation() {
    let e = env();
    e.install(1111);
    assert_eq!(e.integration(7683), Integration::StalePort);
}
