use clap::Parser;
use gateterm_server::app::{serve, AppState};
use gateterm_server::config::{Config, WindowSize};
use gateterm_server::session::manager::SessionManager;

mod connection_urls;

#[derive(clap::Subcommand)]
enum Cmd {
    /// agent 的 hook 回调客户端(由装进 agent 配置里的 hook 调用,不是给人敲的)。
    ///
    /// 只有不支持 http 型 hook 的 agent(如 Codex)需要它:那边只能执行一个命令,
    /// 于是让它执行我们自己,由我们转发到本地 server。
    Hook {
        /// `http://127.0.0.1:<port>/hook/<agent>/<event>`
        url: String,
    },
    /// 清空本机所有 agent 配置里 gateterm 写过的 hook(不论端口、不论事件),改前备份。
    ///
    /// 给异常残留兜底用:server 起不来、端口早就换过好几轮、设置页打不开的时候也能清。
    PurgeHooks,
    /// Gateterm Cloud 账号(可选,用于手机推送通知)
    Cloud {
        #[command(subcommand)]
        cmd: gateterm_server::cloud::cli::CloudCmd,
    },
}

#[derive(Parser)]
#[command(name = "gateterm-server", about = "gateterm terminal session daemon")]
struct Args {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// 配置文件路径(缺省用平台配置目录)
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    /// 覆盖监听 IP(如 0.0.0.0;对外监听请自行保证网络层安全)
    #[arg(long)]
    host: Option<std::net::IpAddr>,
    /// 覆盖监听端口
    #[arg(long)]
    port: Option<u16>,
    /// 固定鉴权 token(缺省每次启动随机生成并打印)
    #[arg(long)]
    token: Option<String>,
    /// 多端同时 attach 一个会话时 pty 用谁的尺寸(缺省 smallest)
    #[arg(long, value_enum)]
    window_size: Option<WindowSize>,
}

fn main() -> anyhow::Result<()> {
    // hook 子命令要在**一切之前**处理:它由 agent 在每个事件上同步调用,必须快、
    // 必须安静。初始化日志、读配置、起 runtime 对它都是纯负担,而任何一行多余的
    // stdout 输出都会被 agent 当成决策读走。
    if let Some(Cmd::Hook { url }) = Args::parse().cmd {
        std::process::exit(gateterm_server::agent::hook_cli::run(&url));
    }
    if let Some(Cmd::PurgeHooks) = Args::parse().cmd {
        std::process::exit(purge_hooks());
    }
    // `cloud` 子命令自己管理 tokio runtime(见 `cloud::cli::run`),不能跑在
    // `#[tokio::main]` 已经起好的 runtime 里面 —— 嵌套 block_on 会 panic。所以
    // main 本身不再是 async,真正 serve 的部分挪到下面手动起的 runtime 里。
    if let Some(Cmd::Cloud { cmd }) = Args::parse().cmd {
        std::process::exit(gateterm_server::cloud::cli::run(cmd, Args::parse().config));
    }
    tokio::runtime::Runtime::new()?.block_on(run_server())
}

async fn run_server() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let config = Config::load(args.config)?;
    // 无头服务端的默认是只回环;desktop 有它自己的默认(见 config::desktop_default_listen)
    let base = config.listen.unwrap_or_else(gateterm_server::config::default_listen);
    let listen = gateterm_server::config::effective_listen(base, args.host, args.port);
    let (token, generated) = gateterm_server::config::resolve_token(args.token, config.token);
    // 必须在任何会话 spawn 之前:密钥要跨重启不变,见 `store::hook_token`
    gateterm_server::agent::store::init_hook_token(
        gateterm_server::agent::store::default_hook_token_path().as_deref(),
    );
    let manager = SessionManager::new(1 << 20, args.window_size.unwrap_or(config.window_size));
    let launchers = std::sync::Arc::new(gateterm_server::launcher::providers(&config.launchers));
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let listen = listener.local_addr()?;

    // Jupyter 式:每次启动打印可直接点开的访问 URL;IPv4 全网卡监听时列出各网卡地址。
    eprintln!("gateterm-server listening on {listen}");
    for url in connection_urls::connection_urls(listen, &token) {
        eprintln!("    {url}");
    }
    if generated {
        eprintln!("    (token 为本次启动随机生成;用 --token 或配置文件可固定)");
    }
    let listen_port = listen.port();
    // 端口一变,已装的 hook 就打不通了,而 agent 对此毫无症状 —— 只能在这里喊一声。
    let home = gateterm_server::agent::detect::DetectEnv::current();
    for w in tokio::task::spawn_blocking(move || gateterm_server::agent::startup_warnings(&home, listen_port))
        .await
        .unwrap_or_default()
    {
        eprintln!("warning: {w}");
    }
    let agents = config.agents;
    let agent_sessions = std::sync::Arc::new(gateterm_server::agent::store::AgentSessionStore::new());
    let cloud = gateterm_server::cloud::creds::SharedCreds::new(gateterm_cloud_client::credentials::default_path());
    let state = AppState { manager, token, launchers, listen_port, agents, agent_sessions, cloud };
    gateterm_server::cloud::notifier::spawn(state.manager.clone(), state.agent_sessions.clone(), state.cloud.clone());
    gateterm_server::agent::history::start_index(&state, false);
    gateterm_server::update::start_daily_checks(env!("CARGO_PKG_VERSION"), |result| {
        match result {
            Ok(Some(update)) => eprintln!(
                "gateterm 有新版本 {} (当前版本 {}),可前往 Release 页面更新:\n    {}",
                update.version,
                env!("CARGO_PKG_VERSION"),
                update.url
            ),
            Ok(None) => {}
            Err(e) => tracing::debug!(error = %e, "检查更新失败"),
        }
    });
    serve(listener, state).await
}

/// `purge-hooks` 子命令。逐家清,一家失败不影响其他家(局部失败原则),有失败则退出码 1。
fn purge_hooks() -> i32 {
    use gateterm_server::agent::{self, apply, detect::DetectEnv, edit::InstallCtx};
    let Some(backup_dir) = apply::default_backup_dir() else {
        eprintln!("无法确定备份目录,放弃清空");
        return 1;
    };
    // 显式敲命令就是显式动作,不看 `install_enabled`(和 HTTP 的 purge 同理)
    let opts = apply::ApplyOpts { enabled: true, backup_dir, backup_keep: 5 };
    // 端口只影响新写入的 URL,清空不看它,随便给一个
    let ctx = InstallCtx { home: DetectEnv::current().home, port: 0, include_blocking: true };
    let mut failed = false;
    for a in agent::registry() {
        let result = agent::plan_purge(a.as_ref(), &ctx)
            .map_err(|e| e.to_string())
            .and_then(|plan| apply::apply(&plan, &opts).map_err(|e| e.to_string()));
        match result {
            Ok(outcomes) => {
                for o in outcomes {
                    match (o.changed, o.backup) {
                        (false, _) => println!("{}: {} 里没有 gateterm 的 hook", a.name(), o.path.display()),
                        (true, Some(b)) => println!("{}: 已清空 {}(备份 {})", a.name(), o.path.display(), b.display()),
                        (true, None) => println!("{}: 已清空 {}", a.name(), o.path.display()),
                    }
                }
            }
            Err(e) => {
                failed = true;
                eprintln!("{}: 清空失败:{e}", a.name());
            }
        }
    }
    i32::from(failed)
}
