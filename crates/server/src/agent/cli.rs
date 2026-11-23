//! `gateterm-server agents <子命令>` CLI 与交互式启动引导。
//!
//! 只做参数编排、预览提示与交互问答;具体扫描、计划与落盘全权委托给
//! [`crate::agent`] 和 [`crate::agent::ops`]。

use std::io::{self, BufRead, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use crate::agent::apply::{default_backup_dir, ApplyOpts, Outcome};
use crate::agent::detect::DetectEnv;
use crate::agent::{ops, plan_purge, AgentAdapter, AgentStatus, Integration};

#[derive(clap::Subcommand, Debug, Clone)]
pub enum AgentsCmd {
    /// 查看所有 AI Agent 的检测结果与 hook 集成状态
    Status {
        /// 覆盖服务监听端口
        #[arg(long)]
        port: Option<u16>,
    },
    /// 为指定或已检测到的 AI Agent 安装/更新 Gateterm 集成
    Install {
        /// 指定要安装的 Agent ID（可多个）
        ids: Vec<String>,
        /// 安装或更新所有已检测到（或端口过期/配置过时）的 Agent
        #[arg(long)]
        all: bool,
        /// 覆盖服务监听端口
        #[arg(long)]
        port: Option<u16>,
        /// 跳过交互确认（非交互环境/脚本必填）
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// 从指定或已集成的 AI Agent 配置中卸载 Gateterm 集成
    Uninstall {
        /// 指定要卸载的 Agent ID（可多个）
        ids: Vec<String>,
        /// 卸载所有当前已集成的 Agent
        #[arg(long)]
        all: bool,
        /// 跳过交互确认（非交互环境/脚本必填）
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// 清空指定或全部 AI Agent 配置中残留的 Gateterm hook（改前自动备份）
    Purge {
        /// 指定要清理的 Agent ID（可多个）
        ids: Vec<String>,
        /// 清理所有受支持 Agent 的配置残留
        #[arg(long)]
        all: bool,
        /// 跳过交互确认（非交互环境/脚本必填）
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// 启动交互式向导，检测并引导安装/更新 Agent 集成
    Setup {
        /// 覆盖服务监听端口
        #[arg(long)]
        port: Option<u16>,
    },
}

/// 入口：`gateterm-server agents <subcommand>`。
pub fn run(cmd: AgentsCmd, config_path: Option<PathBuf>, cli_port: Option<u16>) -> i32 {
    let config = match crate::config::Config::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置失败: {e:#}");
            return 1;
        }
    };
    let backup_dir = match default_backup_dir() {
        Some(dir) => dir,
        None if !matches!(cmd, AgentsCmd::Status { .. }) => {
            eprintln!("无法确定备份目录，放弃执行");
            return 1;
        }
        None => PathBuf::new(),
    };
    let env = DetectEnv::current();
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    if matches!(cmd, AgentsCmd::Setup { .. }) {
        let is_tty = stdin.is_terminal() && io::stderr().is_terminal();
        let stderr = io::stderr();
        let mut writer = stderr.lock();
        return run_with_io(
            cmd,
            &env,
            config.listen,
            cli_port,
            config.agents.install_enabled,
            &backup_dir,
            is_tty,
            &mut reader,
            &mut writer,
        );
    }

    let is_tty = stdin.is_terminal();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    run_with_io(
        cmd,
        &env,
        config.listen,
        cli_port,
        config.agents.install_enabled,
        &backup_dir,
        is_tty,
        &mut reader,
        &mut writer,
    )
}

/// 可测试的核心调度入口：显式注入环境、开关、TTY 状态与输入/输出流。
#[allow(clippy::too_many_arguments)]
pub fn run_with_io(
    cmd: AgentsCmd,
    env: &DetectEnv,
    config_listen: Option<SocketAddr>,
    cli_port: Option<u16>,
    install_enabled: bool,
    backup_dir: &Path,
    is_tty: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> i32 {
    match cmd {
        AgentsCmd::Status { port } => {
            let port = match resolve_port(port, cli_port, config_listen) {
                Ok(p) => p,
                Err(msg) => {
                    let _ = writeln!(writer, "错误: {msg}");
                    return 1;
                }
            };
            cmd_status(env, port, writer)
        }
        AgentsCmd::Install { ids, all, port, yes } => {
            let port = match resolve_port(port, cli_port, config_listen) {
                Ok(p) => p,
                Err(msg) => {
                    let _ = writeln!(writer, "错误: {msg}");
                    return 1;
                }
            };
            cmd_install(env, port, install_enabled, backup_dir, &ids, all, yes, is_tty, reader, writer)
        }
        AgentsCmd::Uninstall { ids, all, yes } => {
            let scan_port = fallback_scan_port(cli_port, config_listen);
            cmd_uninstall(
                env,
                scan_port,
                install_enabled,
                backup_dir,
                &ids,
                all,
                yes,
                is_tty,
                reader,
                writer,
            )
        }
        AgentsCmd::Purge { ids, all, yes } => {
            cmd_purge(env, backup_dir, &ids, all, yes, is_tty, reader, writer)
        }
        AgentsCmd::Setup { port } => {
            let port = match resolve_port(port, cli_port, config_listen) {
                Ok(p) => p,
                Err(msg) => {
                    let _ = writeln!(writer, "错误: {msg}");
                    return 1;
                }
            };
            cmd_setup(env, port, install_enabled, backup_dir, is_tty, reader, writer)
        }
    }
}

/// 解析 CLI 子命令的有效服务端口：子命令 `--port` > 顶层 `--port` > `config.listen` > `7683`。
pub fn resolve_port(
    sub_port: Option<u16>,
    top_port: Option<u16>,
    config_listen: Option<SocketAddr>,
) -> Result<u16, String> {
    let port = sub_port
        .or(top_port)
        .or_else(|| config_listen.map(|a| a.port()))
        .unwrap_or_else(|| crate::config::default_listen().port());
    if port == 0 {
        return Err(
            "端口不能为 0（随机端口无法用于 agent hook 集成），请通过 --port <1-65535> 指定有效端口"
                .into(),
        );
    }
    Ok(port)
}

/// 判断服务启动时是否应进入交互式 Agent 引导。
pub fn should_prompt_on_startup(
    no_agent_prompt_flag: bool,
    env_var: Option<&str>,
    install_enabled: bool,
    stdin_tty: bool,
    stderr_tty: bool,
) -> bool {
    let env_disables = env_var
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
        .unwrap_or(false);
    !no_agent_prompt_flag && !env_disables && install_enabled && stdin_tty && stderr_tty
}

/// 在交互终端中引导用户安装/更新未就绪的 Agent 集成；任意选中项安装失败时返回 `true`。
pub fn run_startup_onboarding(
    env: &DetectEnv,
    port: u16,
    backup_dir: &Path,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> bool {
    run_onboarding_inner(env, port, backup_dir, reader, writer, false)
}

fn run_onboarding_inner(
    env: &DetectEnv,
    port: u16,
    backup_dir: &Path,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    print_up_to_date: bool,
) -> bool {
    let statuses = crate::agent::scan(env, port);
    let mut actionable: Vec<&AgentStatus> = statuses
        .iter()
        .filter(|a| matches!(a.integration, Integration::StalePort | Integration::Outdated))
        .collect();
    actionable.extend(
        statuses
            .iter()
            .filter(|a| a.detected.installed && a.integration == Integration::NotInstalled),
    );

    if actionable.is_empty() {
        if print_up_to_date {
            let _ = writeln!(writer, "当前所有已检测到的 Agent 集成均已就绪（或未检测到可配置的 Agent）。");
        }
        print_remaining_activation_notices(&statuses, &[], writer);
        return false;
    }

    let chosen: Vec<&AgentStatus> = if actionable.len() == 1 {
        let a = actionable[0];
        let _ = writeln!(writer, "\n发现 1 个可配置/更新的 AI Agent:");
        let _ = writeln!(writer, "  {} [{}] — {}", a.name, status_tag(a.integration), a.detected.detail);
        if let Some(path) = &a.detected.config_path {
            let _ = writeln!(writer, "  配置文件: {}", path.display());
        }
        for c in &a.conflicts {
            let _ = writeln!(writer, "  冲突提示: {c}（将只安装状态上报部分）");
        }
        let _ = writeln!(
            writer,
            "  备份说明: 修改前会自动备份原配置到 {}，随时可卸载还原。",
            backup_dir.display()
        );
        let prompt = format!("是否为 {} 安装/更新 Gateterm 集成？[Y/n] ", a.name);
        if is_yes_default_yes(read_prompt_line(reader, writer, &prompt).as_deref()) {
            vec![a]
        } else {
            Vec::new()
        }
    } else {
        let _ = writeln!(writer, "\n发现 {} 个可安装/更新的 AI Agent:", actionable.len());
        for (idx, a) in actionable.iter().enumerate() {
            let _ = writeln!(
                writer,
                "  {}. {} [{}] — {}",
                idx + 1,
                a.name,
                status_tag(a.integration),
                a.detected.detail
            );
            for c in &a.conflicts {
                let _ = writeln!(writer, "     冲突提示: {c}（将只安装状态上报部分）");
            }
        }
        let _ = writeln!(
            writer,
            "  备份说明: 修改前会自动备份原配置到 {}，随时可卸载还原。",
            backup_dir.display()
        );
        let prompt = format!(
            "是否立即为以上 {} 个 Agent 安装/更新 Gateterm 集成？[Y/n/s(逐个选择)] ",
            actionable.len()
        );
        match read_prompt_line(reader, writer, &prompt).as_deref() {
            Some("" | "y" | "yes") => actionable,
            Some("s" | "select") => actionable
                .into_iter()
                .filter(|a| {
                    let per_prompt = format!(
                        "  为 {} ({}) 安装/更新？[Y/n] ",
                        a.name,
                        status_tag(a.integration)
                    );
                    is_yes_default_yes(read_prompt_line(reader, writer, &per_prompt).as_deref())
                })
                .collect(),
            _ => Vec::new(),
        }
    };

    let opts = ApplyOpts {
        enabled: true,
        backup_dir: backup_dir.to_path_buf(),
        backup_keep: 5,
    };
    let mut any_failed = false;
    let mut shown_activation: Vec<&'static str> = Vec::new();
    for a in chosen {
        let Some(adapter) = crate::agent::find(a.id) else {
            continue;
        };
        match ops::install_with_opts(adapter, &env.home, port, &opts) {
            Ok(res) => {
                print_outcomes(writer, a.name, "安装/更新", &res.outcomes);
                if !res.include_blocking {
                    let _ = writeln!(writer, "  说明: 检测到冲突，已只安装状态上报 hook");
                }
                if let Some(hint) = adapter.activation_hint() {
                    let _ = writeln!(writer, "  后续激活 ({}): {hint}", a.name);
                    shown_activation.push(a.id);
                }
            }
            Err(e) => {
                any_failed = true;
                let _ = writeln!(writer, "{}: 安装失败: {e}", a.name);
            }
        }
    }

    print_remaining_activation_notices(&statuses, &shown_activation, writer);
    any_failed
}

fn print_remaining_activation_notices(
    statuses: &[AgentStatus],
    already_shown: &[&'static str],
    writer: &mut dyn Write,
) {
    for a in statuses {
        if a.integration == Integration::NeedsActivation && !already_shown.contains(&a.id) {
            let hint = crate::agent::find(a.id)
                .and_then(|ad| ad.activation_hint())
                .unwrap_or("请运行 `gateterm-server agents status` 查看激活步骤");
            let _ = writeln!(writer, "提示: {} 已安装但待激活: {hint}", a.name);
        }
    }
}

fn fallback_scan_port(cli_port: Option<u16>, config_listen: Option<SocketAddr>) -> u16 {
    cli_port
        .or_else(|| config_listen.map(|a| a.port()))
        .filter(|&p| p != 0)
        .unwrap_or_else(|| crate::config::default_listen().port())
}

fn cmd_status(env: &DetectEnv, port: u16, writer: &mut dyn Write) -> i32 {
    let statuses = crate::agent::scan(env, port);
    let _ = writeln!(writer, "Agent 集成状态（目标服务端口: {port}）：");
    for a in statuses {
        let detected = if a.detected.installed { "detected" } else { "not-detected" };
        let _ = writeln!(
            writer,
            "- {} ({}) | integration={} | {} ({})",
            a.id,
            a.name,
            integration_label(a.integration),
            detected,
            a.detected.detail
        );
        if let Some(path) = &a.detected.config_path {
            let _ = writeln!(writer, "    config: {}", path.display());
        }
        for c in &a.conflicts {
            let _ = writeln!(writer, "    conflict: {c}");
        }
        if a.integration == Integration::NeedsActivation
            && let Some(hint) = crate::agent::find(a.id).and_then(|ad| ad.activation_hint())
        {
            let _ = writeln!(writer, "    activation_hint: {hint}");
        }
    }
    0
}

#[allow(clippy::too_many_arguments)]
fn cmd_install(
    env: &DetectEnv,
    port: u16,
    install_enabled: bool,
    backup_dir: &Path,
    ids: &[String],
    all: bool,
    yes: bool,
    is_tty: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> i32 {
    if !install_enabled {
        let _ = writeln!(
            writer,
            "错误: config.toml 中 [agents].install_enabled = false，已禁用安装/更新。如需清理残留请使用 `gateterm-server agents purge`。"
        );
        return 1;
    }
    let selection = match validate_selection(
        ids,
        all,
        "请指定至少一个 AGENT_ID 或传入 --all（交互向导请运行 `gateterm-server agents setup`）",
        writer,
    ) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if !yes && !is_tty {
        let _ = writeln!(
            writer,
            "错误: 当前为非交互终端，拒绝静默修改配置。确认执行请传入 -y / --yes。"
        );
        return 1;
    }

    let targets: Vec<&'static dyn AgentAdapter> = match selection {
        TargetSelection::Explicit(list) => list,
        TargetSelection::All => crate::agent::scan(env, port)
            .into_iter()
            .filter(|a| {
                a.detected.installed
                    || matches!(a.integration, Integration::StalePort | Integration::Outdated)
            })
            .filter_map(|a| crate::agent::find(a.id))
            .collect(),
    };
    if targets.is_empty() {
        let _ = writeln!(writer, "没有检测到需要安装或更新的 Agent。");
        return 0;
    }

    if !yes {
        let _ = writeln!(writer, "即将安装/更新以下 Agent 集成（端口 {port}）：");
        for &adapter in &targets {
            let _ = writeln!(writer, "- {} ({})", adapter.name(), adapter.id());
            match ops::preview_plan(adapter, &env.home, port) {
                Ok(summary) => {
                    print_plan_groups(writer, "写入", &summary.install);
                    if !summary.include_blocking {
                        let _ = writeln!(writer, "    注意: 检测到阻塞式 hook 冲突，将仅安装状态上报部分");
                    }
                }
                Err(e) => {
                    let _ = writeln!(writer, "    预览失败: {e}");
                }
            }
            let (_, conflicts) = adapter.integration(&crate::agent::edit::InstallCtx {
                home: env.home.clone(),
                port,
                include_blocking: true,
            });
            for c in conflicts {
                let _ = writeln!(writer, "    冲突: {c}");
            }
        }
        if !confirm_tty("确认安装以上变更？[Y/n] ", true, reader, writer) {
            let _ = writeln!(writer, "已取消。");
            return 0;
        }
    }

    let opts = ApplyOpts {
        enabled: true,
        backup_dir: backup_dir.to_path_buf(),
        backup_keep: 5,
    };
    let mut failed = false;
    for adapter in targets {
        match ops::install_with_opts(adapter, &env.home, port, &opts) {
            Ok(res) => {
                print_outcomes(writer, adapter.name(), "安装/更新", &res.outcomes);
                if !res.include_blocking {
                    let _ = writeln!(writer, "  说明: 检测到冲突，已只安装状态上报 hook");
                }
                if let Some(hint) = adapter.activation_hint() {
                    let _ = writeln!(writer, "  激活提示: {hint}");
                }
            }
            Err(e) => {
                failed = true;
                let _ = writeln!(writer, "{}: 安装失败: {e}", adapter.name());
            }
        }
    }
    i32::from(failed)
}

#[allow(clippy::too_many_arguments)]
fn cmd_uninstall(
    env: &DetectEnv,
    scan_port: u16,
    install_enabled: bool,
    backup_dir: &Path,
    ids: &[String],
    all: bool,
    yes: bool,
    is_tty: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> i32 {
    if !install_enabled {
        let _ = writeln!(
            writer,
            "错误: config.toml 中 [agents].install_enabled = false，已禁用卸载。如需清理残留请使用 `gateterm-server agents purge`。"
        );
        return 1;
    }
    let selection = match validate_selection(
        ids,
        all,
        "请指定至少一个 AGENT_ID 或传入 --all",
        writer,
    ) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if !yes && !is_tty {
        let _ = writeln!(
            writer,
            "错误: 当前为非交互终端，拒绝静默修改配置。确认执行请传入 -y / --yes。"
        );
        return 1;
    }

    let targets: Vec<&'static dyn AgentAdapter> = match selection {
        TargetSelection::Explicit(list) => list,
        TargetSelection::All => crate::agent::scan(env, scan_port)
            .into_iter()
            .filter(|a| a.integration != Integration::NotInstalled)
            .filter_map(|a| crate::agent::find(a.id))
            .collect(),
    };
    if targets.is_empty() {
        let _ = writeln!(writer, "没有已安装的 Gateterm Agent 集成需要卸载。");
        return 0;
    }

    if !yes {
        let _ = writeln!(writer, "即将卸载以下 Agent 集成：");
        for &adapter in &targets {
            let _ = writeln!(writer, "- {} ({})", adapter.name(), adapter.id());
            match ops::preview_plan(adapter, &env.home, scan_port) {
                Ok(summary) => print_plan_groups(writer, "移除", &summary.uninstall),
                Err(e) => {
                    let _ = writeln!(writer, "    预览失败: {e}");
                }
            }
        }
        if !confirm_tty("确认卸载以上变更？[Y/n] ", true, reader, writer) {
            let _ = writeln!(writer, "已取消。");
            return 0;
        }
    }

    let opts = ApplyOpts {
        enabled: true,
        backup_dir: backup_dir.to_path_buf(),
        backup_keep: 5,
    };
    let mut failed = false;
    for adapter in targets {
        match ops::uninstall_with_opts(adapter, &env.home, scan_port, &opts) {
            Ok(res) => print_outcomes(writer, adapter.name(), "卸载", &res.outcomes),
            Err(e) => {
                failed = true;
                let _ = writeln!(writer, "{}: 卸载失败: {e}", adapter.name());
            }
        }
    }
    i32::from(failed)
}

#[allow(clippy::too_many_arguments)]
fn cmd_purge(
    env: &DetectEnv,
    backup_dir: &Path,
    ids: &[String],
    all: bool,
    yes: bool,
    is_tty: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> i32 {
    let selection = match validate_selection(
        ids,
        all,
        "请指定至少一个 AGENT_ID 或传入 --all（避免误清空全部配置）",
        writer,
    ) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if !yes && !is_tty {
        let _ = writeln!(
            writer,
            "错误: 当前为非交互终端，拒绝静默清空配置。确认执行请传入 -y / --yes。"
        );
        return 1;
    }

    let targets: Vec<&'static dyn AgentAdapter> = match selection {
        TargetSelection::Explicit(list) => list,
        TargetSelection::All => crate::agent::registry().to_vec(),
    };

    if !yes {
        let _ = writeln!(writer, "即将清理以下 Agent 配置中的全部 Gateterm hook（改前会自动备份）：");
        let ctx = crate::agent::edit::InstallCtx {
            home: env.home.clone(),
            port: 0,
            include_blocking: true,
        };
        for &adapter in &targets {
            let _ = writeln!(writer, "- {} ({})", adapter.name(), adapter.id());
            match plan_purge(adapter, &ctx) {
                Ok(edits) => print_plan_groups(writer, "清理", &ops::summarize_edits(&edits)),
                Err(e) => {
                    let _ = writeln!(writer, "    预览失败: {e}");
                }
            }
        }
        if !confirm_tty(
            "确认清理以上 Agent 配置中的全部 gateterm hook？[y/N] ",
            false,
            reader,
            writer,
        ) {
            let _ = writeln!(writer, "已取消。");
            return 0;
        }
    }

    let mut failed = false;
    for adapter in targets {
        match ops::purge_with_opts(adapter, &env.home, backup_dir.to_path_buf(), 5) {
            Ok(outcomes) => print_outcomes(writer, adapter.name(), "清空", &outcomes),
            Err(e) => {
                failed = true;
                let _ = writeln!(writer, "{}: 清空失败: {e}", adapter.name());
            }
        }
    }
    i32::from(failed)
}

fn cmd_setup(
    env: &DetectEnv,
    port: u16,
    install_enabled: bool,
    backup_dir: &Path,
    is_tty: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> i32 {
    if !is_tty {
        let _ = writeln!(
            writer,
            "错误: agents setup 需要在交互终端中运行；非交互环境请使用 `gateterm-server agents install --all -y`"
        );
        return 1;
    }
    if !install_enabled {
        let _ = writeln!(
            writer,
            "错误: config.toml 中 [agents].install_enabled = false，已禁用安装引导。"
        );
        return 1;
    }
    i32::from(run_onboarding_inner(env, port, backup_dir, reader, writer, true))
}

enum TargetSelection {
    Explicit(Vec<&'static dyn AgentAdapter>),
    All,
}

fn validate_selection(
    ids: &[String],
    all: bool,
    empty_hint: &str,
    writer: &mut dyn Write,
) -> Result<TargetSelection, i32> {
    if ids.is_empty() {
        if all {
            return Ok(TargetSelection::All);
        }
        let _ = writeln!(writer, "错误: {empty_hint}");
        return Err(1);
    }

    let mut adapters = Vec::with_capacity(ids.len());
    let mut unknown = Vec::new();
    for id in ids {
        match crate::agent::find(id) {
            Some(ad) => {
                if !adapters.iter().any(|existing: &&dyn AgentAdapter| existing.id() == ad.id()) {
                    adapters.push(ad);
                }
            }
            None => unknown.push(id.as_str()),
        }
    }
    if !unknown.is_empty() {
        let available: Vec<&str> = crate::agent::registry().iter().map(|a| a.id()).collect();
        let _ = writeln!(
            writer,
            "错误: 未知的 Agent ID: {}（可用 ID: {}）",
            unknown.join(", "),
            available.join(", ")
        );
        return Err(1);
    }
    Ok(TargetSelection::Explicit(adapters))
}

fn read_prompt_line(reader: &mut dyn BufRead, writer: &mut dyn Write, prompt: &str) -> Option<String> {
    let _ = write!(writer, "{prompt}");
    let _ = writer.flush();
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_ascii_lowercase()),
    }
}

fn is_yes_default_yes(answer: Option<&str>) -> bool {
    matches!(answer, Some("" | "y" | "yes"))
}

fn confirm_tty(
    prompt: &str,
    default_yes: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> bool {
    let ans = read_prompt_line(reader, writer, prompt);
    if default_yes {
        is_yes_default_yes(ans.as_deref())
    } else {
        matches!(ans.as_deref(), Some("y" | "yes"))
    }
}

fn print_plan_groups(writer: &mut dyn Write, verb: &str, groups: &[ops::PlanGroup]) {
    if groups.is_empty() {
        let _ = writeln!(writer, "    (无文件变更)");
        return;
    }
    for g in groups {
        let _ = writeln!(writer, "    {verb}: {} [{}]", g.path.display(), g.events.join(", "));
    }
}

fn print_outcomes(writer: &mut dyn Write, name: &str, verb: &str, outcomes: &[Outcome]) {
    for o in outcomes {
        match (o.changed, &o.backup) {
            (false, _) => {
                let _ = writeln!(writer, "{name}: {} 无需变更", o.path.display());
            }
            (true, Some(b)) => {
                let _ = writeln!(
                    writer,
                    "{name}: 已{verb} {} (备份 {})",
                    o.path.display(),
                    b.display()
                );
            }
            (true, None) => {
                let _ = writeln!(writer, "{name}: 已{verb} {}", o.path.display());
            }
        }
    }
}

fn integration_label(integration: Integration) -> &'static str {
    match integration {
        Integration::Installed => "installed",
        Integration::NotInstalled => "not-installed",
        Integration::StalePort => "stale-port",
        Integration::Outdated => "outdated",
        Integration::NeedsActivation => "needs-activation",
    }
}

fn status_tag(integration: Integration) -> &'static str {
    match integration {
        Integration::StalePort => "端口变更",
        Integration::Outdated => "需更新",
        Integration::NotInstalled => "未安装",
        Integration::NeedsActivation => "待放行",
        Integration::Installed => "已安装",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_tty_distinguishes_eof_from_empty_enter() {
        let mut eof = io::Cursor::new(Vec::<u8>::new());
        let mut out = Vec::new();
        assert!(!confirm_tty("prompt? ", true, &mut eof, &mut out));

        let mut enter = io::Cursor::new(b"\n".to_vec());
        assert!(confirm_tty("prompt? ", true, &mut enter, &mut out));

        let mut enter_no_default = io::Cursor::new(b"\n".to_vec());
        assert!(!confirm_tty("prompt? ", false, &mut enter_no_default, &mut out));
    }
}
