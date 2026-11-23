//! Integration tests for `gateterm_server::agent::cli` subcommands and startup onboarding.

use std::fs;
use std::io::Cursor;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use gateterm_server::agent::cli::{
    resolve_port, run_startup_onboarding, run_with_io, should_prompt_on_startup, AgentsCmd,
};
use gateterm_server::agent::detect::DetectEnv;
use gateterm_server::agent::{scan, Integration};
use serde_json::{json, Value};

struct TestEnv {
    home: tempfile::TempDir,
    backups: tempfile::TempDir,
}

impl TestEnv {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            backups: tempfile::tempdir().unwrap(),
        }
    }

    fn detect_env(&self) -> DetectEnv {
        DetectEnv { home: self.home.path().to_path_buf(), path: None }
    }

    fn backup_dir(&self) -> &Path {
        self.backups.path()
    }

    fn claude_settings(&self) -> PathBuf {
        self.home.path().join(".claude").join("settings.json")
    }

    fn codex_config(&self) -> PathBuf {
        self.home.path().join(".codex").join("config.toml")
    }

    fn codex_hooks(&self) -> PathBuf {
        self.home.path().join(".codex").join("hooks.json")
    }

    fn seed_claude_user_config(&self) {
        let path = self.claude_settings();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\n  \"model\": \"opus\"\n}\n").unwrap();
    }

    fn seed_claude_hook(&self, port: u16) {
        let path = self.claude_settings();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let doc = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{
                    "matcher": "",
                    "hooks": [{
                        "type": "http",
                        "url": format!("http://127.0.0.1:{port}/hook/claude-code/stop")
                    }]
                }]
            }
        });
        fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    }

    fn seed_codex_user_config(&self) {
        let path = self.codex_config();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "model = \"gpt-5\"\n").unwrap();
    }

    fn integration(&self, id: &str, port: u16) -> Integration {
        scan(&self.detect_env(), port)
            .into_iter()
            .find(|a| a.id == id)
            .unwrap()
            .integration
    }

    fn backups(&self) -> Vec<PathBuf> {
        fs::read_dir(self.backup_dir())
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn resolve_port_priority_and_rejects_port_zero() {
    let listen: SocketAddr = "127.0.0.1:8003".parse().unwrap();
    assert_eq!(resolve_port(Some(8001), Some(8002), Some(listen)), Ok(8001));
    assert_eq!(resolve_port(None, Some(8002), Some(listen)), Ok(8002));
    assert_eq!(resolve_port(None, None, Some(listen)), Ok(8003));
    assert_eq!(resolve_port(None, None, None), Ok(7683));

    assert!(resolve_port(Some(0), Some(8002), Some(listen)).is_err());
    assert!(resolve_port(None, Some(0), Some(listen)).is_err());
    let zero_listen: SocketAddr = "0.0.0.0:0".parse().unwrap();
    assert!(resolve_port(None, None, Some(zero_listen)).is_err());
}

#[test]
fn should_prompt_on_startup_honors_flag_env_config_and_tty() {
    assert!(should_prompt_on_startup(false, None, true, true, true));
    assert!(should_prompt_on_startup(false, Some("0"), true, true, true));
    assert!(should_prompt_on_startup(false, Some("false"), true, true, true));
    assert!(should_prompt_on_startup(false, Some(""), true, true, true));

    assert!(!should_prompt_on_startup(true, None, true, true, true));
    assert!(!should_prompt_on_startup(false, Some("1"), true, true, true));
    assert!(!should_prompt_on_startup(false, Some(" 1 "), true, true, true));
    assert!(!should_prompt_on_startup(false, Some("true"), true, true, true));
    assert!(!should_prompt_on_startup(false, Some(" TRUE "), true, true, true));
    assert!(!should_prompt_on_startup(false, None, false, true, true));
    assert!(!should_prompt_on_startup(false, None, true, false, true));
    assert!(!should_prompt_on_startup(false, None, true, true, false));
}

#[test]
fn startup_onboarding_single_agent_enter_defaults_to_yes_and_installs() {
    let env = TestEnv::new();
    env.seed_claude_user_config();

    let mut input = Cursor::new(b"\n".to_vec());
    let mut output = Vec::new();
    let failed = run_startup_onboarding(
        &env.detect_env(),
        7683,
        env.backup_dir(),
        &mut input,
        &mut output,
    );
    let out = String::from_utf8(output).unwrap();

    assert!(!failed, "output: {out}");
    assert!(
        out.contains("是否为 Claude Code 安装/更新 Gateterm 集成？[Y/n] "),
        "missing expected prompt in output: {out}"
    );
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);
    assert_eq!(read_json(&env.claude_settings())["model"], json!("opus"));
    assert!(!env.backups().is_empty(), "backup should be created");
}

#[test]
fn startup_onboarding_single_agent_n_and_eof_skip() {
    for raw in [b"n\n".to_vec(), b"".to_vec()] {
        let env = TestEnv::new();
        env.seed_claude_user_config();
        let before = fs::read_to_string(env.claude_settings()).unwrap();

        let mut input = Cursor::new(raw);
        let mut output = Vec::new();
        let failed = run_startup_onboarding(
            &env.detect_env(),
            7683,
            env.backup_dir(),
            &mut input,
            &mut output,
        );

        assert!(!failed);
        assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), before);
        assert_eq!(env.integration("claude-code", 7683), Integration::NotInstalled);
        assert!(env.backups().is_empty());
    }
}

#[test]
fn startup_onboarding_multi_agent_batch_yes_and_selective_s() {
    // 1) Batch yes ("y\n"): updates StalePort Claude Code + installs NotInstalled Codex
    {
        let env = TestEnv::new();
        env.seed_claude_hook(9000);
        env.seed_codex_user_config();

        let mut input = Cursor::new(b"y\n".to_vec());
        let mut output = Vec::new();
        let failed = run_startup_onboarding(
            &env.detect_env(),
            7683,
            env.backup_dir(),
            &mut input,
            &mut output,
        );
        let out = String::from_utf8(output).unwrap();

        assert!(!failed, "output: {out}");
        assert!(
            out.contains("是否立即为以上 2 个 Agent 安装/更新 Gateterm 集成？[Y/n/s(逐个选择)] "),
            "missing multi-agent prompt: {out}"
        );
        assert_eq!(env.integration("claude-code", 7683), Integration::Installed);
        assert_eq!(env.integration("codex", 7683), Integration::NeedsActivation);
        assert!(out.contains("/hooks"), "should print Codex activation hint: {out}");
    }

    // 2) Selective ("s\ny\nn\n"): updates first (StalePort Claude Code) and skips second (Codex)
    {
        let env = TestEnv::new();
        env.seed_claude_hook(9000);
        env.seed_codex_user_config();

        let mut input = Cursor::new(b"s\ny\nn\n".to_vec());
        let mut output = Vec::new();
        let failed = run_startup_onboarding(
            &env.detect_env(),
            7683,
            env.backup_dir(),
            &mut input,
            &mut output,
        );
        let out = String::from_utf8(output).unwrap();

        assert!(!failed, "output: {out}");
        assert_eq!(env.integration("claude-code", 7683), Integration::Installed);
        assert_eq!(env.integration("codex", 7683), Integration::NotInstalled);
        assert!(!env.codex_hooks().exists());
    }

    // 3) Batch no ("n\n"): skips all
    {
        let env = TestEnv::new();
        env.seed_claude_hook(9000);
        env.seed_codex_user_config();
        let claude_before = fs::read_to_string(env.claude_settings()).unwrap();

        let mut input = Cursor::new(b"n\n".to_vec());
        let mut output = Vec::new();
        let failed = run_startup_onboarding(
            &env.detect_env(),
            7683,
            env.backup_dir(),
            &mut input,
            &mut output,
        );

        assert!(!failed);
        assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), claude_before);
        assert_eq!(env.integration("claude-code", 7683), Integration::StalePort);
        assert_eq!(env.integration("codex", 7683), Integration::NotInstalled);
    }
}

#[test]
fn startup_onboarding_empty_actionable_prints_needs_activation_hints() {
    let env = TestEnv::new();
    env.seed_codex_user_config();
    // Install Codex at 7683 so it becomes NeedsActivation (not yet /hooks approved).
    let mut yes_in = Cursor::new(Vec::new());
    let mut yes_out = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["codex".into()],
            all: false,
            port: Some(7683),
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut yes_in,
        &mut yes_out,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("codex", 7683), Integration::NeedsActivation);

    let mut input = Cursor::new(Vec::new());
    let mut output = Vec::new();
    let failed = run_startup_onboarding(
        &env.detect_env(),
        7683,
        env.backup_dir(),
        &mut input,
        &mut output,
    );
    let out = String::from_utf8(output).unwrap();
    assert!(!failed);
    assert!(out.contains("Codex") && out.contains("/hooks"), "output: {out}");
}

#[test]
fn non_tty_mutating_subcommands_require_yes_flag() {
    let env = TestEnv::new();
    env.seed_claude_user_config();
    let initial = fs::read_to_string(env.claude_settings()).unwrap();

    // Install without -y in non-TTY fails without mutating
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["claude-code".into()],
            all: false,
            port: Some(7683),
            yes: false,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), initial);

    // Setup in non-TTY fails with hint pointing to install --all -y
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Setup { port: Some(7683) },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    let setup_out = String::from_utf8(out_buf).unwrap();
    assert_eq!(code, 1);
    assert!(
        setup_out.contains("agents install --all -y"),
        "expected non-TTY setup hint, got: {setup_out}"
    );
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), initial);

    // Install with -y succeeds in non-TTY
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["claude-code".into()],
            all: false,
            port: Some(7683),
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);

    // Uninstall without -y in non-TTY fails without mutating
    let installed_snapshot = fs::read_to_string(env.claude_settings()).unwrap();
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Uninstall {
            ids: vec!["claude-code".into()],
            all: false,
            yes: false,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), installed_snapshot);

    // Purge without -y in non-TTY fails without mutating
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Purge {
            ids: vec!["claude-code".into()],
            all: false,
            yes: false,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), installed_snapshot);

    // Uninstall with -y succeeds in non-TTY
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Uninstall {
            ids: vec!["claude-code".into()],
            all: false,
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::NotInstalled);

    // Seed stale hook and test Purge with -y in non-TTY
    env.seed_claude_hook(9999);
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Purge {
            ids: vec!["claude-code".into()],
            all: false,
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::NotInstalled);
}

#[test]
fn agents_install_respects_install_enabled_false_while_purge_ignores_it() {
    let env = TestEnv::new();
    env.seed_claude_hook(9000);
    let before = fs::read_to_string(env.claude_settings()).unwrap();

    // Install blocked when install_enabled == false
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: Vec::new(),
            all: true,
            port: Some(7683),
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        false,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), before);

    // Uninstall also blocked when install_enabled == false
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Uninstall {
            ids: Vec::new(),
            all: true,
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        false,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), before);

    // Setup also blocked when install_enabled == false
    let mut in_buf = Cursor::new(b"\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Setup { port: Some(7683) },
        &env.detect_env(),
        None,
        None,
        false,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), before);

    // Purge ignores install_enabled == false and clears hooks
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Purge {
            ids: Vec::new(),
            all: true,
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        false,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 9000), Integration::NotInstalled);
    assert_eq!(read_json(&env.claude_settings()), json!({ "model": "opus" }));
}

#[test]
fn agents_install_rejects_unknown_id_and_empty_selection() {
    let env = TestEnv::new();
    env.seed_claude_user_config();
    let before = fs::read_to_string(env.claude_settings()).unwrap();

    // Unknown ID
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["unknown-agent".into()],
            all: false,
            port: Some(7683),
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    let out = String::from_utf8(out_buf).unwrap();
    assert_eq!(code, 1);
    assert!(out.contains("unknown-agent"), "output: {out}");
    assert!(out.contains("claude-code"), "should list available IDs: {out}");
    assert_eq!(fs::read_to_string(env.claude_settings()).unwrap(), before);

    // Neither IDs nor --all for Install, Uninstall, Purge
    for cmd in [
        AgentsCmd::Install { ids: Vec::new(), all: false, port: Some(7683), yes: true },
        AgentsCmd::Uninstall { ids: Vec::new(), all: false, yes: true },
        AgentsCmd::Purge { ids: Vec::new(), all: false, yes: true },
    ] {
        let mut in_buf = Cursor::new(Vec::new());
        let mut out_buf = Vec::new();
        let code = run_with_io(
            cmd,
            &env.detect_env(),
            None,
            None,
            true,
            env.backup_dir(),
            false,
            &mut in_buf,
            &mut out_buf,
        );
        let out = String::from_utf8(out_buf).unwrap();
        assert_eq!(code, 1);
        assert!(out.contains("--all"), "missing usage hint: {out}");
    }
}

#[test]
fn tty_purge_defaults_to_no_while_install_defaults_to_yes() {
    let env = TestEnv::new();
    env.seed_claude_user_config();

    // TTY install with Enter installs
    let mut in_buf = Cursor::new(b"\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["claude-code".into()],
            all: false,
            port: Some(7683),
            yes: false,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);

    // TTY purge with Enter (default No) keeps hooks intact and exits 0
    let mut in_buf = Cursor::new(b"\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Purge {
            ids: vec!["claude-code".into()],
            all: false,
            yes: false,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    let out = String::from_utf8(out_buf).unwrap();
    assert_eq!(code, 0);
    assert!(out.contains("[y/N]"), "output: {out}");
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);

    // TTY purge with "y\n" removes hooks
    let mut in_buf = Cursor::new(b"y\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Purge {
            ids: vec!["claude-code".into()],
            all: false,
            yes: false,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::NotInstalled);
}

#[test]
fn agents_status_prints_statuses_and_rejects_port_zero() {
    let env = TestEnv::new();
    env.seed_claude_hook(9000);

    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Status { port: Some(7683) },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    let out = String::from_utf8(out_buf).unwrap();
    assert_eq!(code, 0);
    assert!(out.contains("claude-code") && out.contains("stale-port"), "output: {out}");

    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Status { port: Some(0) },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 1);
}

#[test]
fn agents_setup_with_tty_runs_onboarding_and_reports_up_to_date() {
    let env = TestEnv::new();
    env.seed_claude_user_config();

    // First run installs detected agent
    let mut in_buf = Cursor::new(b"\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Setup { port: Some(7683) },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);

    // Second run finds nothing actionable and prints up-to-date summary
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Setup { port: Some(7683) },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    let out = String::from_utf8(out_buf).unwrap();
    assert_eq!(code, 0);
    assert!(out.contains("就绪"), "expected up-to-date summary, got: {out}");
}

#[test]
fn startup_onboarding_orders_updates_before_new_installs() {
    let env = TestEnv::new();
    // Registry order has claude-code (index 0) before codex (index 1).
    // Make claude-code NotInstalled (detected) and codex StalePort so update-first ordering puts codex #1.
    env.seed_claude_user_config();
    let mut in_buf = Cursor::new(Vec::new());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["codex".into()],
            all: false,
            port: Some(9000),
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("codex", 7683), Integration::StalePort);

    // Choose selective "s", accept #1 (Codex update) and decline #2 (Claude Code install).
    let mut in_buf = Cursor::new(b"s\ny\nn\n".to_vec());
    let mut out_buf = Vec::new();
    let failed = run_startup_onboarding(
        &env.detect_env(),
        7683,
        env.backup_dir(),
        &mut in_buf,
        &mut out_buf,
    );
    let out = String::from_utf8(out_buf).unwrap();
    assert!(!failed, "output: {out}");
    assert!(out.contains("1. Codex [端口变更]"), "expected Codex first: {out}");
    assert!(out.contains("2. Claude Code [未安装]"), "expected Claude second: {out}");
    assert_eq!(env.integration("codex", 7683), Integration::NeedsActivation);
    assert_eq!(env.integration("claude-code", 7683), Integration::NotInstalled);
}

#[test]
fn tty_uninstall_respects_confirmation_and_declines_without_changes() {
    let env = TestEnv::new();
    env.seed_claude_user_config();
    let mut setup_in = Cursor::new(Vec::new());
    let mut setup_out = Vec::new();
    let setup_code = run_with_io(
        AgentsCmd::Install {
            ids: vec!["claude-code".into()],
            all: false,
            port: Some(7683),
            yes: true,
        },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        false,
        &mut setup_in,
        &mut setup_out,
    );
    assert_eq!(setup_code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);

    // Decline with "n\n" -> exit 0, still installed
    let mut in_buf = Cursor::new(b"n\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Uninstall { ids: Vec::new(), all: true, yes: false },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::Installed);

    // Accept with Enter -> uninstalls
    let mut in_buf = Cursor::new(b"\n".to_vec());
    let mut out_buf = Vec::new();
    let code = run_with_io(
        AgentsCmd::Uninstall { ids: Vec::new(), all: true, yes: false },
        &env.detect_env(),
        None,
        None,
        true,
        env.backup_dir(),
        true,
        &mut in_buf,
        &mut out_buf,
    );
    assert_eq!(code, 0);
    assert_eq!(env.integration("claude-code", 7683), Integration::NotInstalled);
}

#[test]
fn cli_binary_help_and_non_tty_guard_work_end_to_end() {
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_gateterm-server");
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("config.toml");
    fs::write(&cfg, "").unwrap();

    let help = Command::new(bin)
        .arg("--help")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(help.status.success());
    let help_out = String::from_utf8_lossy(&help.stdout);
    assert!(help_out.contains("agents"), "top-level help missing agents: {help_out}");
    assert!(
        help_out.contains("--no-agent-prompt"),
        "top-level help missing --no-agent-prompt: {help_out}"
    );

    let agents_help = Command::new(bin)
        .args(["agents", "--help"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(agents_help.status.success());
    let sub_out = String::from_utf8_lossy(&agents_help.stdout);
    for sub in ["status", "install", "uninstall", "purge", "setup"] {
        assert!(sub_out.contains(sub), "agents --help missing {sub}: {sub_out}");
    }

    let install_non_tty = Command::new(bin)
        .arg("--config")
        .arg(&cfg)
        .args(["agents", "install", "--all"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(install_non_tty.status.code(), Some(1));

    let setup_non_tty = Command::new(bin)
        .arg("--config")
        .arg(&cfg)
        .args(["agents", "setup"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(setup_non_tty.status.code(), Some(1));
}
