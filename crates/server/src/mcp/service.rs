//! Shared plain-text session and agent-approval service layer used by both the
//! REST `/api/sessions/*` handlers and the `/mcp` JSON-RPC tool dispatcher.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use gateterm_protocol::{AgentState, SessionInfo};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::broadcast;

use crate::agent::store::{AgentSession, AnswerResult, Decision, PendingRequest};
use crate::app::AppState;
use crate::launcher::Launcher;
use crate::session::pty::{ScreenSnapshot, Session, SessionOutput};

pub const MAX_SCROLLBACK_LINES: usize = 500;
pub const MAX_WAIT_MS: u64 = 15_000;
pub const QUIET_WINDOW: Duration = Duration::from_millis(400);

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(msg)
            | Self::NotFound(msg)
            | Self::Conflict(msg)
            | Self::Internal(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ServiceError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionAgentSummary {
    pub agent_id: String,
    pub agent_session_id: String,
    pub state: AgentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionWithAgent {
    #[serde(flatten)]
    pub info: SessionInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<SessionAgentSummary>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct NewSessionInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub launcher_id: Option<String>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct SendInputArgs {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub keys: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub enter: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub wait_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SendInputResult {
    pub ok: bool,
    pub seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<ScreenSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KillResult {
    pub killed: bool,
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AnswerApprovalArgs {
    pub pending_id: String,
    pub decision: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub updated_input: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnswerApprovalResult {
    pub ok: bool,
    pub pending_id: String,
    pub decision: String,
}

fn agent_priority(state: AgentState) -> Option<u8> {
    match state {
        AgentState::Waiting => Some(5),
        AgentState::Error => Some(4),
        AgentState::Working => Some(3),
        AgentState::Thinking => Some(2),
        AgentState::Idle => Some(1),
        AgentState::Done => None,
    }
}

/// Picks the highest-priority active (non-`Done`) agent attached to `session_id`,
/// breaking ties by newest `updated_at`.
pub fn select_best_agent(agents: &[AgentSession], session_id: u64) -> Option<SessionAgentSummary> {
    agents
        .iter()
        .filter(|a| a.session == Some(session_id))
        .filter_map(|a| agent_priority(a.state).map(|rank| (rank, a.updated_at, a)))
        .max_by(|(r1, t1, a1), (r2, t2, a2)| {
            r1.cmp(r2)
                .then_with(|| t1.cmp(t2))
                .then_with(|| a2.agent_id.cmp(&a1.agent_id))
                .then_with(|| a2.agent_session_id.cmp(&a1.agent_session_id))
        })
        .map(|(_, _, a)| SessionAgentSummary {
            agent_id: a.agent_id.clone(),
            agent_session_id: a.agent_session_id.clone(),
            state: a.state,
            detail: a.detail.clone(),
            pending_id: a.pending.clone(),
        })
}

pub fn validate_command(command: Option<&[String]>) -> Result<(), ServiceError> {
    if let Some(cmd) = command
        && (cmd.is_empty() || cmd[0].trim().is_empty())
    {
        return Err(ServiceError::BadRequest(
            "command must be a non-empty array with a non-blank program".into(),
        ));
    }
    Ok(())
}

pub async fn list_sessions(state: &AppState) -> Vec<SessionWithAgent> {
    let sessions = state.manager.list();
    let agents = state.agent_sessions.list();
    sessions
        .into_iter()
        .map(|info| {
            let agent = select_best_agent(&agents, info.id);
            SessionWithAgent { info, agent }
        })
        .collect()
}

pub async fn list_launchers(state: &AppState) -> Result<Vec<Launcher>, ServiceError> {
    let providers = state.launchers.clone();
    tokio::task::spawn_blocking(move || crate::launcher::discover_all(&providers))
        .await
        .map_err(|e| ServiceError::Internal(format!("launcher discovery failed: {e}")))
}

pub async fn new_session(
    state: &AppState,
    input: NewSessionInput,
) -> Result<SessionInfo, ServiceError> {
    validate_command(input.command.as_deref())?;

    let (name, command, cwd) = if let Some(ref launcher_id) = input.launcher_id {
        let launchers = list_launchers(state).await?;
        let launcher = launchers
            .into_iter()
            .find(|l| l.id == *launcher_id)
            .ok_or_else(|| {
                ServiceError::NotFound(format!("unknown launcher_id: {launcher_id}"))
            })?;
        (
            input.name.or(Some(launcher.name)),
            input.command.or(Some(launcher.command)),
            input.cwd.or(launcher.cwd),
        )
    } else {
        (input.name, input.command, input.cwd)
    };

    validate_command(command.as_deref())?;

    let session = state
        .manager
        .create(name, command, cwd)
        .map_err(|e| ServiceError::Internal(format!("spawn failed: {e:#}")))?;
    Ok(session.info())
}

pub fn read_session(
    state: &AppState,
    id: u64,
    scrollback_lines: usize,
) -> Result<ScreenSnapshot, ServiceError> {
    let session = state
        .manager
        .get(id)
        .ok_or_else(|| ServiceError::NotFound(format!("no such session: {id}")))?;
    Ok(session.plain_text_screen(scrollback_lines.min(MAX_SCROLLBACK_LINES)))
}

pub async fn send_input(
    state: &AppState,
    id: u64,
    args: SendInputArgs,
) -> Result<SendInputResult, ServiceError> {
    let session = state
        .manager
        .get(id)
        .ok_or_else(|| ServiceError::NotFound(format!("no such session: {id}")))?;
    send_input_to_session(&session, args).await
}

pub(crate) async fn send_input_to_session(
    session: &Arc<Session>,
    args: SendInputArgs,
) -> Result<SendInputResult, ServiceError> {
    let modes = session.input_modes();
    let bytes = crate::session::keys::encode_text_and_keys(
        args.text.as_deref(),
        &args.keys,
        args.enter,
        modes,
        crate::keyboard::win32_input_mode(),
    )
    .map_err(ServiceError::BadRequest)?;

    let wait_ms = args.wait_ms.min(MAX_WAIT_MS);
    // Subscribe before checking/writing so an exit or fast command echo cannot slip past unobserved.
    let rx = (wait_ms > 0).then(|| session.subscribe());

    if session.has_exited() {
        return Err(ServiceError::Conflict(format!(
            "session {} has exited",
            session.id
        )));
    }

    if let Err(e) = session.write_input(&bytes) {
        let msg = format!("{e:#}");
        if session.has_exited() || msg.contains("session pty closed") {
            return Err(ServiceError::Conflict(format!(
                "session {} has exited",
                session.id
            )));
        }
        return Err(ServiceError::Internal(format!("write input failed: {msg}")));
    }

    if let Some(mut rx) = rx {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        let mut quiet_deadline: Option<tokio::time::Instant> = None;
        let mut saw_output = false;

        loop {
            if session.has_exited() {
                break;
            }
            let now = tokio::time::Instant::now();
            let next_wakeup = match quiet_deadline.filter(|_| saw_output) {
                Some(q) if q < deadline => q,
                _ => deadline,
            };
            if now >= next_wakeup {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep_until(next_wakeup) => {
                    break;
                }
                msg = rx.recv() => {
                    match msg {
                        Ok(SessionOutput::Exited) | Err(broadcast::error::RecvError::Closed) => break,
                        Ok(SessionOutput::Data { .. }) => {
                            saw_output = true;
                            quiet_deadline = Some((tokio::time::Instant::now() + QUIET_WINDOW).min(deadline));
                        }
                        Ok(SessionOutput::Resized { .. } | SessionOutput::Clipboard { .. }) => continue,
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if session.has_exited() {
                                break;
                            }
                            saw_output = true;
                            quiet_deadline = Some((tokio::time::Instant::now() + QUIET_WINDOW).min(deadline));
                        }
                    }
                }
            }
        }

        let screen = session.plain_text_screen(0);
        return Ok(SendInputResult {
            ok: true,
            seq: screen.seq,
            screen: Some(screen),
        });
    }

    Ok(SendInputResult {
        ok: true,
        seq: session.end_seq(),
        screen: None,
    })
}

pub fn kill_session(state: &AppState, id: u64) -> Result<KillResult, ServiceError> {
    if state.manager.kill(id) {
        Ok(KillResult { killed: true, id })
    } else {
        Err(ServiceError::NotFound(format!("no such session: {id}")))
    }
}

pub fn list_pending_approvals(state: &AppState, session_id: Option<u64>) -> Vec<PendingRequest> {
    let list = state.agent_sessions.list_pending();
    match session_id {
        Some(sid) => list.into_iter().filter(|req| req.session == Some(sid)).collect(),
        None => list,
    }
}

pub fn answer_approval(
    state: &AppState,
    args: AnswerApprovalArgs,
) -> Result<AnswerApprovalResult, ServiceError> {
    let decision = match args.decision.as_str() {
        "allow" => Decision::Allow {
            message: args.message,
            updated_input: args.updated_input,
        },
        "deny" => Decision::Deny {
            message: args.message,
        },
        other => {
            return Err(ServiceError::BadRequest(format!(
                "unknown decision: {other}"
            )));
        }
    };
    match state.agent_sessions.answer(&args.pending_id, decision) {
        AnswerResult::Ok => Ok(AnswerApprovalResult {
            ok: true,
            pending_id: args.pending_id,
            decision: args.decision,
        }),
        AnswerResult::NotFound => Err(ServiceError::NotFound(format!(
            "no such pending request: {}",
            args.pending_id
        ))),
        AnswerResult::AlreadyAnswered => Err(ServiceError::Conflict(format!(
            "pending request already answered: {}",
            args.pending_id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use gateterm_protocol::AgentState;

    use super::*;
    use crate::agent::store::{AgentSession, Update};
    use crate::launcher::{Launcher, LauncherProvider};
    use crate::session::pty::test_support;

    fn test_state() -> AppState {
        struct StaticLauncher;
        impl LauncherProvider for StaticLauncher {
            fn id(&self) -> &'static str {
                "static"
            }
            fn discover(&self) -> anyhow::Result<Vec<Launcher>> {
                Ok(vec![
                    Launcher::new("static", "echo", "Static Shell", vec!["sh".into()])
                        .with_cwd(Some("/tmp".into())),
                ])
            }
        }
        AppState {
            manager: crate::session::manager::SessionManager::new(1 << 20, Default::default()),
            token: "secret".into(),
            launchers: Arc::new(vec![Box::new(StaticLauncher)]),
            listen_port: 0,
            agents: Default::default(),
            agent_sessions: Arc::new(crate::agent::store::AgentSessionStore::new()),
            cloud: Default::default(),
            web_title: Default::default(),
            mcp: Default::default(),
        }
    }

    fn mk_agent(
        agent_id: &str,
        agent_session_id: &str,
        session: Option<u64>,
        state: AgentState,
        updated_at: u64,
        pending: Option<&str>,
    ) -> AgentSession {
        AgentSession {
            agent_id: agent_id.into(),
            agent_session_id: agent_session_id.into(),
            session,
            state,
            detail: Some(format!("{agent_id}-{state:?}")),
            cwd: None,
            title: None,
            transcript: None,
            supports_transcript: false,
            pending: pending.map(str::to_string),
            updated_at,
            acked_at: 0,
            process: None,
        }
    }

    #[test]
    fn select_best_agent_excludes_done_and_ranks_by_priority_then_updated_at() {
        let agents = vec![
            mk_agent("claude-code", "done", Some(1), AgentState::Done, 9999, None),
            mk_agent("claude-code", "idle", Some(1), AgentState::Idle, 500, None),
            mk_agent("claude-code", "thinking", Some(1), AgentState::Thinking, 600, None),
            mk_agent("claude-code", "working", Some(1), AgentState::Working, 700, None),
            mk_agent("claude-code", "error", Some(1), AgentState::Error, 800, None),
            mk_agent("codex", "other-session", Some(2), AgentState::Waiting, 9999, Some("p-other")),
            mk_agent("claude-code", "wait-old", Some(1), AgentState::Waiting, 100, Some("p-old")),
            mk_agent("codex", "wait-new", Some(1), AgentState::Waiting, 200, Some("p-new")),
        ];

        let best = select_best_agent(&agents, 1).expect("expected an active agent");
        assert_eq!(best.agent_id, "codex");
        assert_eq!(best.agent_session_id, "wait-new");
        assert_eq!(best.state, AgentState::Waiting);
        assert_eq!(best.pending_id.as_deref(), Some("p-new"));

        let without_waiting: Vec<_> = agents
            .iter()
            .filter(|a| a.state != AgentState::Waiting)
            .cloned()
            .collect();
        assert_eq!(select_best_agent(&without_waiting, 1).unwrap().state, AgentState::Error);

        let without_error: Vec<_> = without_waiting
            .into_iter()
            .filter(|a| a.state != AgentState::Error)
            .collect();
        assert_eq!(select_best_agent(&without_error, 1).unwrap().state, AgentState::Working);

        let without_working: Vec<_> = without_error
            .into_iter()
            .filter(|a| a.state != AgentState::Working)
            .collect();
        assert_eq!(select_best_agent(&without_working, 1).unwrap().state, AgentState::Thinking);

        let without_thinking: Vec<_> = without_working
            .into_iter()
            .filter(|a| a.state != AgentState::Thinking)
            .collect();
        assert_eq!(select_best_agent(&without_thinking, 1).unwrap().state, AgentState::Idle);

        let only_done = vec![mk_agent("claude-code", "done", Some(1), AgentState::Done, 100, None)];
        assert_eq!(select_best_agent(&only_done, 1), None);
    }

    #[test]
    fn validate_new_session_command_rejects_empty_array_or_blank_program() {
        assert!(validate_command(None).is_ok());
        assert!(validate_command(Some(&["bash".to_string()])).is_ok());
        assert!(matches!(validate_command(Some(&[])), Err(ServiceError::BadRequest(_))));
        assert!(matches!(
            validate_command(Some(&["   ".to_string()])),
            Err(ServiceError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn send_input_subscribes_before_write_and_settles_on_quiet_window() {
        let session = test_support::session();
        let writer_session = session.clone();
        let emitter = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            test_support::output(&writer_session, b"echo hello\r\n");
            tokio::time::sleep(Duration::from_millis(30)).await;
            test_support::output(&writer_session, b"hello\r\n$ ");
        });

        let started = std::time::Instant::now();
        let res = send_input_to_session(
            &session,
            SendInputArgs {
                text: Some("echo hello".into()),
                keys: vec![],
                enter: true,
                wait_ms: 5_000,
            },
        )
        .await
        .unwrap();
        let elapsed = started.elapsed();
        emitter.await.unwrap();

        assert!(res.ok);
        let screen = res.screen.expect("wait_ms > 0 must return screen snapshot");
        assert_eq!(res.seq, screen.seq);
        assert!(screen.text.contains("hello"), "screen text: {:?}", screen.text);
        assert!(
            elapsed >= Duration::from_millis(350) && elapsed < Duration::from_secs(2),
            "expected quiet settle around 400ms, got {elapsed:?}"
        );

        test_support::mark_exited(&session);
        let exited_started = std::time::Instant::now();
        let err = send_input_to_session(
            &session,
            SendInputArgs {
                text: Some("ls".into()),
                keys: vec![],
                enter: true,
                wait_ms: 5_000,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)), "got {err:?}");
        assert!(
            exited_started.elapsed() < Duration::from_millis(200),
            "exited session must return immediately"
        );
    }

    #[tokio::test]
    async fn send_input_with_zero_wait_omits_screen_and_exited_event_stops_wait_immediately() {
        let session = test_support::session();
        test_support::output(&session, b"prompt> ");
        let immediate = send_input_to_session(
            &session,
            SendInputArgs {
                text: Some("pwd".into()),
                keys: vec![],
                enter: false,
                wait_ms: 0,
            },
        )
        .await
        .unwrap();
        assert!(immediate.ok);
        assert_eq!(immediate.seq, session.end_seq());
        assert!(immediate.screen.is_none());

        let invalid = send_input_to_session(&session, SendInputArgs::default())
            .await
            .unwrap_err();
        assert!(matches!(invalid, ServiceError::BadRequest(_)));

        let exiter = session.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            test_support::output(&exiter, b"bye\r\n");
            test_support::mark_exited(&exiter);
        });
        let started = std::time::Instant::now();
        let res = send_input_to_session(
            &session,
            SendInputArgs {
                text: None,
                keys: vec!["Enter".into()],
                enter: false,
                wait_ms: 5_000,
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_millis(300));
        let screen = res.screen.unwrap();
        assert!(screen.exited);
        assert!(screen.text.contains("bye"));
    }

    #[tokio::test]
    async fn service_launcher_pending_and_approval_errors_are_mapped() {
        let state = test_state();

        let launchers = list_launchers(&state).await.unwrap();
        assert_eq!(launchers.len(), 1);
        assert_eq!(launchers[0].id, "static:echo");

        let missing_launcher = new_session(
            &state,
            NewSessionInput {
                launcher_id: Some("static:nope".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(missing_launcher, ServiceError::NotFound(_)));

        let bad_override = new_session(
            &state,
            NewSessionInput {
                launcher_id: Some("static:echo".into()),
                command: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(bad_override, ServiceError::BadRequest(_)));

        assert!(matches!(
            read_session(&state, 404, 0),
            Err(ServiceError::NotFound(_))
        ));
        assert!(matches!(
            kill_session(&state, 404),
            Err(ServiceError::NotFound(_))
        ));

        let _ = state.agent_sessions.apply("claude-code", "s1", Some(10), Update::default());
        let _ = state.agent_sessions.apply("claude-code", "s2", Some(20), Update::default());
        let _rx1 = state.agent_sessions.insert_pending(PendingRequest {
            id: "p1".into(),
            agent_id: "claude-code".into(),
            agent_session_id: "s1".into(),
            session: Some(10),
            tool_name: Some("Bash".into()),
            tool_input: serde_json::json!({"command": "echo 1"}),
            created_at: 1,
            expires_at: 100,
        });
        let _rx2 = state.agent_sessions.insert_pending(PendingRequest {
            id: "p2".into(),
            agent_id: "claude-code".into(),
            agent_session_id: "s2".into(),
            session: Some(20),
            tool_name: Some("Read".into()),
            tool_input: serde_json::json!({}),
            created_at: 2,
            expires_at: 100,
        });

        assert_eq!(list_pending_approvals(&state, None).len(), 2);
        let filtered = list_pending_approvals(&state, Some(20));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "p2");
        assert!(list_pending_approvals(&state, Some(999)).is_empty());

        let bad_decision = answer_approval(
            &state,
            AnswerApprovalArgs {
                pending_id: "p1".into(),
                decision: "maybe".into(),
                message: None,
                updated_input: None,
            },
        )
        .unwrap_err();
        assert!(matches!(bad_decision, ServiceError::BadRequest(_)));

        let not_found = answer_approval(
            &state,
            AnswerApprovalArgs {
                pending_id: "missing".into(),
                decision: "allow".into(),
                message: None,
                updated_input: None,
            },
        )
        .unwrap_err();
        assert!(matches!(not_found, ServiceError::NotFound(_)));

        let ok = answer_approval(
            &state,
            AnswerApprovalArgs {
                pending_id: "p1".into(),
                decision: "allow".into(),
                message: Some("ok".into()),
                updated_input: None,
            },
        )
        .unwrap();
        assert_eq!(
            ok,
            AnswerApprovalResult {
                ok: true,
                pending_id: "p1".into(),
                decision: "allow".into(),
            }
        );

        let conflict = answer_approval(
            &state,
            AnswerApprovalArgs {
                pending_id: "p1".into(),
                decision: "deny".into(),
                message: None,
                updated_input: None,
            },
        )
        .unwrap_err();
        assert!(matches!(conflict, ServiceError::Conflict(_)));
    }
}
