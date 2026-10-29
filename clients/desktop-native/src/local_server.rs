//! Embedded local `gateterm-server` supervisor for `clients/desktop-native`.
//!
//! Holds `SessionManager` and `AgentSessionStore` across HTTP listener restarts so
//! active local PTY sessions survive port/token reconfigurations, and spawns the Cloud
//! push notifier and Agent transcript indexer just like `clients/desktop-web`.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use gateterm_server::app::{AppState, serve};
use gateterm_server::config::{AgentsConfig, Config as ServerConfig};
use gateterm_server::launcher::LauncherProvider;
use gateterm_server::session::manager::SessionManager;
use tokio::net::TcpListener;

const BIND_RETRIES: usize = 10;
const BIND_RETRY_INTERVAL: Duration = Duration::from_millis(20);
const DEFAULT_BUFFER_CAP: usize = 1024 * 1024;

pub fn display_host(ip: IpAddr) -> String {
    if ip.is_unspecified() {
        "127.0.0.1".to_string()
    } else if ip.is_ipv6() {
        format!("[{ip}]")
    } else {
        ip.to_string()
    }
}

struct RunningListener {
    listen: SocketAddr,
    token: String,
    join: tokio::task::JoinHandle<()>,
}

pub struct LocalServerSupervisor {
    manager: Arc<SessionManager>,
    launchers: Arc<Vec<Box<dyn LauncherProvider>>>,
    agents: AgentsConfig,
    agent_sessions: Arc<gateterm_server::agent::store::AgentSessionStore>,
    cloud: gateterm_server::cloud::creds::SharedCreds,
    running: Option<RunningListener>,
}

impl LocalServerSupervisor {
    pub fn from_system_config() -> Self {
        gateterm_server::agent::store::init_hook_token(
            gateterm_server::agent::store::default_hook_token_path().as_deref(),
        );
        let cfg = ServerConfig::load(None).unwrap_or_default();
        let window_size = cfg.window_size;
        let manager = SessionManager::new(DEFAULT_BUFFER_CAP, window_size);
        let agent_sessions = Arc::new(gateterm_server::agent::store::AgentSessionStore::new());
        let cloud = gateterm_server::cloud::creds::SharedCreds::new(
            gateterm_cloud_client::credentials::default_path(),
        );
        gateterm_server::cloud::notifier::spawn(
            manager.clone(),
            agent_sessions.clone(),
            cloud.clone(),
        );
        Self {
            manager,
            launchers: Arc::new(gateterm_server::launcher::providers(&cfg.launchers)),
            agents: cfg.agents,
            agent_sessions,
            cloud,
            running: None,
        }
    }

    pub async fn start(listen_str: &str, token: String) -> Result<Self> {
        let addr: SocketAddr = listen_str
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:17683".parse().unwrap());
        let mut sup = Self::from_system_config();
        sup.start_or_rebind(addr, token).await?;
        Ok(sup)
    }

    pub fn manager(&self) -> &Arc<SessionManager> {
        &self.manager
    }

    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.running.as_ref().map(|r| r.listen)
    }

    pub fn base_url(&self) -> String {
        if let Some(addr) = self.listen_addr() {
            format!("http://{}:{}", display_host(addr.ip()), addr.port())
        } else {
            "http://127.0.0.1:17683".to_string()
        }
    }

    pub fn access_url(&self) -> String {
        let base = self.base_url();
        let tok = self.token();
        format!("{base}/#token={tok}")
    }

    pub fn token(&self) -> &str {
        self.running
            .as_ref()
            .map(|r| r.token.as_str())
            .unwrap_or("")
    }

    pub fn session_count(&self) -> usize {
        self.manager.list().len()
    }

    pub async fn start_or_rebind(
        &mut self,
        listen: SocketAddr,
        token: String,
    ) -> Result<SocketAddr> {
        ensure!(
            !token.trim().is_empty(),
            "Refusing to start embedded server with empty token"
        );
        let same_addr = self.running.as_ref().is_some_and(|r| r.listen == listen);
        let (listener, actual) = if same_addr {
            self.stop();
            let l = bind_with_retry(listen).await?;
            let actual = l
                .local_addr()
                .context("Failed to read local_addr of embedded listener")?;
            (l, actual)
        } else {
            let l = match TcpListener::bind(listen).await {
                Ok(l) => l,
                Err(first_err) => {
                    let fallback = SocketAddr::new(listen.ip(), 0);
                    TcpListener::bind(fallback)
                        .await
                        .with_context(|| format!("Failed to bind {listen}: {first_err}"))?
                }
            };
            let actual = l
                .local_addr()
                .context("Failed to read local_addr of embedded listener")?;
            self.stop();
            (l, actual)
        };
        let state = AppState {
            manager: self.manager.clone(),
            token: token.clone(),
            launchers: self.launchers.clone(),
            listen_port: actual.port(),
            agents: self.agents,
            agent_sessions: self.agent_sessions.clone(),
            cloud: self.cloud.clone(),
        };
        gateterm_server::agent::history::start_index(&state, false);
        let join = tokio::spawn(async move {
            if let Err(e) = serve(listener, state).await {
                tracing::error!(error = %e, "embedded gateterm-server exited");
            }
        });
        let port = actual.port();
        tokio::task::spawn_blocking(move || {
            let env = gateterm_server::agent::detect::DetectEnv::current();
            for w in gateterm_server::agent::startup_warnings(&env, port) {
                tracing::warn!("{w}");
            }
        });
        self.running = Some(RunningListener {
            listen: actual,
            token,
            join,
        });
        Ok(actual)
    }

    pub fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.join.abort();
        }
    }

    pub fn shutdown_all(&mut self) {
        self.stop();
        let ids: Vec<u64> = self.manager.list().iter().map(|s| s.id).collect();
        for id in ids {
            self.manager.kill(id);
        }
    }
}

impl Drop for LocalServerSupervisor {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

async fn bind_with_retry(addr: SocketAddr) -> Result<TcpListener> {
    let mut last_err = None;
    for i in 0..BIND_RETRIES {
        match TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) => {
                last_err = Some(e);
                if i + 1 < BIND_RETRIES {
                    tokio::time::sleep(BIND_RETRY_INTERVAL).await;
                }
            }
        }
    }
    Err(last_err.unwrap()).with_context(|| format!("Could not bind {addr}"))
}
