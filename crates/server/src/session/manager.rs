use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use gateterm_protocol::{ServerMsg, SessionEventKind, SessionInfo};
use tokio::sync::broadcast;

use crate::config::WindowSize;

use super::pty::{Launch, Session, SessionOutput};

/// Internal notification events preserve hook order without extending the client wire protocol.
#[derive(Clone)]
pub(crate) enum AgentNotice {
    State(ServerMsg),
    TurnCompleted { session: u64, agent_id: String, agent_session_id: String },
}

pub struct SessionManager {
    buffer_cap: usize,
    window_size: WindowSize,
    next_id: AtomicU64,
    sessions: Mutex<HashMap<u64, Arc<Session>>>,
    events: broadcast::Sender<ServerMsg>,
    agent_notices: broadcast::Sender<AgentNotice>,
}

impl SessionManager {
    pub fn new(buffer_cap: usize, window_size: WindowSize) -> Arc<Self> {
        let (events, _) = broadcast::channel(64);
        let (agent_notices, _) = broadcast::channel(64);
        Arc::new(Self {
            buffer_cap,
            window_size,
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
            events,
            agent_notices,
        })
    }

    pub fn events(&self) -> broadcast::Receiver<ServerMsg> {
        self.events.subscribe()
    }

    /// 往会话事件广播里塞一条消息。agent 集成用它把 `AgentEvent` 推给所有客户端 ——
    /// 复用同一条通路,`conn.rs` 那边一行都不用改。
    pub fn publish(&self, msg: ServerMsg) {
        if matches!(msg, ServerMsg::AgentEvent { .. }) {
            let _ = self.agent_notices.send(AgentNotice::State(msg.clone()));
        }
        let _ = self.events.send(msg);
    }

    pub(crate) fn agent_notices(&self) -> broadcast::Receiver<AgentNotice> {
        self.agent_notices.subscribe()
    }

    pub(crate) fn turn_completed(&self, session: u64, agent_id: &str, agent_session_id: &str) {
        let _ = self.agent_notices.send(AgentNotice::TurnCompleted {
            session, agent_id: agent_id.into(), agent_session_id: agent_session_id.into(),
        });
    }

    fn emit(&self, event: SessionEventKind, session: SessionInfo) {
        let _ = self.events.send(ServerMsg::SessionEvent { event, session });
    }

    pub fn create(
        self: &Arc<Self>,
        name: Option<String>,
        command: Option<Vec<String>>,
        cwd: Option<String>,
    ) -> Result<Arc<Session>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let name = name.unwrap_or_else(|| format!("gateterm-{id}"));
        let launch = Launch { command, cwd };
        let session =
            match Session::spawn(id, name, 80, 24, launch, self.buffer_cap, self.window_size) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(session = id, error = %e, "session spawn failed");
                return Err(e);
            }
        };
        self.sessions.lock().unwrap().insert(id, session.clone());
        self.emit(SessionEventKind::Created, session.info());
        tracing::info!(session = id, name = %session.info().name, "session created");

        // 监视退出:自动移除 + Closed 事件
        let mgr = self.clone();
        let watch = session.clone();
        tokio::spawn(async move {
            // 必须先订阅再查 has_exited,否则"检查时还活着、订阅前已退出"会永远等
            let mut rx = watch.subscribe();
            if !watch.has_exited() {
                loop {
                    match rx.recv().await {
                        Ok(SessionOutput::Exited) | Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if watch.has_exited() {
                                break;
                            }
                        }
                        Ok(_) => continue,
                    }
                }
            }
            if mgr.sessions.lock().unwrap().remove(&id).is_some() {
                tracing::info!(session = id, "session exited, removed");
                mgr.emit(SessionEventKind::Closed, watch.info());
            }
        });
        Ok(session)
    }

    pub fn get(&self, id: u64) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().map(|s| s.info()).collect();
        v.sort_by_key(|s| s.id);
        v
    }

    pub fn kill(&self, id: u64) -> bool {
        // 先从列表摘掉并广播 Closed,UI 不必等 ConPTY 读线程收尸。
        // 子进程/PTY 仍由 Session::kill + wait 线程清理。
        let session = self.sessions.lock().unwrap().remove(&id);
        match session {
            Some(s) => {
                tracing::info!(session = id, "killing session");
                s.kill();
                self.emit(SessionEventKind::Closed, s.info());
                true
            }
            None => {
                tracing::warn!(session = id, "kill: no such session");
                false
            }
        }
    }

    pub fn rename(&self, id: u64, name: &str) -> bool {
        match self.get(id) {
            Some(s) => {
                s.rename(name);
                self.emit(SessionEventKind::Renamed, s.info());
                true
            }
            None => false,
        }
    }
}
