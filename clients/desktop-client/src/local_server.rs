//! Embedded local `gateterm-server` supervisor for `clients/desktop-client`.
//!
//! Holds `SessionManager` and `AgentSessionStore` across HTTP listener restarts so
//! active local PTY sessions survive port/token reconfigurations.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use gateterm_server::app::{AppState, serve};
use gateterm_server::config::{AgentsConfig, Config as ServerConfig};
use gateterm_server::launcher::LauncherProvider;
use gateterm_server::session::manager::SessionManager;
use tokio::net::TcpListener;

const BIND_RETRIES: usize = 8;
const BIND_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const DEFAULT_BUFFER_CAP: usize = 1024 * 1024;

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
        let cfg = ServerConfig::load(None).unwrap_or_default();
        let window_size = cfg.window_size;
        Self {
            manager: SessionManager::new(DEFAULT_BUFFER_CAP, window_size),
            launchers: Arc::new(gateterm_server::launcher::providers(&cfg.launchers)),
            agents: cfg.agents,
            agent_sessions: Arc::new(gateterm_server::agent::store::AgentSessionStore::new()),
            cloud: gateterm_server::cloud::creds::SharedCreds::new(
                gateterm_cloud_client::credentials::default_path(),
            ),
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

    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.running.as_ref().map(|r| r.listen)
    }

    pub fn base_url(&self) -> String {
        if let Some(addr) = self.listen_addr() {
            format!("http://{addr}")
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
        self.stop();

        // Try the requested address first; if already in use and port != 0, fall back to ephemeral port on loopback
        let listener = match bind_with_retry(listen).await {
            Ok(l) => l,
            Err(first_err) => {
                let fallback = SocketAddr::new(listen.ip(), 0);
                TcpListener::bind(fallback)
                    .await
                    .with_context(|| format!("Failed to bind {listen}: {first_err}"))?
            }
        };
        let actual = listener
            .local_addr()
            .context("Failed to read local_addr of embedded listener")?;
        let state = AppState {
            manager: self.manager.clone(),
            token: token.clone(),
            launchers: self.launchers.clone(),
            listen_port: actual.port(),
            agents: self.agents.clone(),
            agent_sessions: self.agent_sessions.clone(),
            cloud: self.cloud.clone(),
        };
        let join = tokio::spawn(async move {
            if let Err(e) = serve(listener, state).await {
                tracing::error!(error = %e, "embedded gateterm-server exited");
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
}

impl Drop for LocalServerSupervisor {
    fn drop(&mut self) {
        self.stop();
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
