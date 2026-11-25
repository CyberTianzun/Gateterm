use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use gateterm_protocol::{ServerMsg, SessionEventKind, SessionInfo};
use tokio::sync::broadcast;

use crate::config::{ClipboardConfig, WindowSize};

use super::pty::{Launch, Session, SessionOutput};

/// Internal notification events preserve hook order without extending the client wire protocol.
#[derive(Clone)]
pub(crate) enum AgentNotice {
    State(ServerMsg),
    TurnCompleted { session: u64, agent_id: String, agent_session_id: String },
}

pub struct SessionManager {
    instance_id: String,
    buffer_cap: usize,
    window_size: WindowSize,
    clipboard: ClipboardConfig,
    next_id: AtomicU64,
    sessions: Mutex<HashMap<u64, Arc<Session>>>,
    events: broadcast::Sender<ServerMsg>,
    agent_notices: broadcast::Sender<AgentNotice>,
}

impl SessionManager {
    pub fn new(buffer_cap: usize, window_size: WindowSize) -> Arc<Self> {
        Self::with_clipboard(buffer_cap, window_size, ClipboardConfig::default())
    }

    pub fn with_clipboard(
        buffer_cap: usize,
        window_size: WindowSize,
        clipboard: ClipboardConfig,
    ) -> Arc<Self> {
        let (events, _) = broadcast::channel(64);
        let (agent_notices, _) = broadcast::channel(64);
        Arc::new(Self {
            instance_id: uuid::Uuid::new_v4().simple().to_string(),
            buffer_cap,
            window_size,
            clipboard,
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
            events,
            agent_notices,
        })
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
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
        let session = match Session::spawn_with_clipboard(
            id,
            name,
            80,
            24,
            launch,
            self.buffer_cap,
            self.window_size,
            self.clipboard,
            Some(self.instance_id.as_str()),
        ) {
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

    /// 当前所有会话(无序)。
    pub fn sessions(&self) -> Vec<Arc<Session>> {
        self.sessions.lock().unwrap().values().cloned().collect()
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().map(|s| s.info()).collect();
        v.sort_by_key(|s| s.id);
        v
    }

    /// All active sessions sorted ascending by `id`.
    pub fn list_sessions(&self) -> Vec<Arc<Session>> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
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

    /// Auto-rename a session if it exists, is not manually locked, and the name changes.
    pub fn auto_rename(&self, id: u64, name: &str) -> bool {
        let Some(s) = self.get(id) else {
            return false;
        };
        if s.auto_rename(name) {
            self.emit(SessionEventKind::Renamed, s.info());
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_test_session(&self, session: Arc<Session>) {
        self.sessions.lock().unwrap().insert(session.id, session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::pty::test_support;

    #[test]
    fn auto_rename_emits_renamed_event_only_when_name_changes_and_not_locked() {
        let mgr = SessionManager::new(1 << 20, WindowSize::default());
        let mut events = mgr.events();
        let session = test_support::session();
        mgr.sessions
            .lock()
            .unwrap()
            .insert(session.id, session.clone());

        assert!(mgr.auto_rename(session.id, "Fix auth"));
        match events.try_recv().unwrap() {
            ServerMsg::SessionEvent {
                event: SessionEventKind::Renamed,
                session: info,
            } => assert_eq!(info.name, "Fix auth"),
            other => panic!("expected Renamed event, got {other:?}"),
        }

        // Idempotent, empty, and unknown session calls must not emit events.
        assert!(!mgr.auto_rename(session.id, "Fix auth"));
        assert!(!mgr.auto_rename(session.id, ""));
        assert!(!mgr.auto_rename(999, "Missing"));
        assert!(events.try_recv().is_err());

        // Manual rename locks out subsequent auto_rename calls.
        assert!(mgr.rename(session.id, "Pinned Tab"));
        match events.try_recv().unwrap() {
            ServerMsg::SessionEvent {
                event: SessionEventKind::Renamed,
                session: info,
            } => assert_eq!(info.name, "Pinned Tab"),
            other => panic!("expected Renamed event, got {other:?}"),
        }
        assert!(!mgr.auto_rename(session.id, "Overwrite Attempt"));
        assert_eq!(session.info().name, "Pinned Tab");
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn create_injects_gateterm_server_id_and_list_sessions_is_sorted() {
        use std::time::Duration;

        let mgr = SessionManager::new(1 << 20, WindowSize::default());
        #[cfg(windows)]
        let cmd = Some(vec![
            "cmd.exe".into(),
            "/C".into(),
            "echo %GATETERM_SERVER_ID%:%GATETERM_SESSION_ID%".into(),
        ]);
        #[cfg(unix)]
        let cmd = Some(vec![
            "/bin/sh".into(),
            "-c".into(),
            r#"printf "%s:%s\n" "$GATETERM_SERVER_ID" "$GATETERM_SESSION_ID""#.into(),
        ]);

        let session = mgr.create(None, cmd, None).unwrap();
        let expected = format!("{}:{}", mgr.instance_id(), session.id);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let last_text = session.plain_text_screen(0).text;
            if last_text.contains(&expected) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "expected {expected:?} in screen text, got: {last_text:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        mgr.kill(session.id);

        let mgr2 = SessionManager::new(1 << 20, WindowSize::default());
        let s1 = mgr2.create(None, None, None).unwrap();
        let s2 = mgr2.create(None, None, None).unwrap();
        let s3 = mgr2.create(None, None, None).unwrap();
        let ids: Vec<u64> = mgr2.list_sessions().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![s1.id, s2.id, s3.id]);
        for id in ids {
            mgr2.kill(id);
        }
    }
}
