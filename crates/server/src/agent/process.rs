//! Codex TUI 可以退出而共享 app-server 继续活着,这时 SessionEnd 不一定立即到达。
//! hook 仍负责识别会话;这里只在成功找到托管终端内的 Codex CLI 后监视它的退出。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

use super::store::{AgentSession, AgentSessionStore};
use crate::app::AppState;
use crate::session::manager::SessionManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Identity {
    pub pid: u32,
    pub started_at: u64,
}

/// 需要监视退出的终端 CLI 进程规格(供带共享守护进程的 Agent Adapter 声明)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessWatchSpec {
    pub exe_names: &'static [&'static str],
    pub exclude_subcommands: &'static [&'static str],
}

#[derive(Debug, Clone)]
struct CliCandidate {
    session_id: u64,
    identity: Identity,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    terminal_cwd: Option<PathBuf>,
    bound_agent_session_id: Option<String>,
}

fn refresh(system: &mut System, processes: ProcessesToUpdate<'_>) {
    system.refresh_processes_specifics(
        processes,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );
}

fn refresh_with_cwd(system: &mut System, processes: ProcessesToUpdate<'_>) {
    system.refresh_processes_specifics(
        processes,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::Always),
    );
}

fn identity(system: &System, pid: Pid) -> Option<Identity> {
    let process = system.process(pid)?;
    if matches!(
        process.status(),
        ProcessStatus::Zombie | ProcessStatus::Dead
    ) {
        return None;
    }
    Some(Identity {
        pid: pid.as_u32(),
        started_at: process.start_time(),
    })
}

fn matches_cli(spec: &ProcessWatchSpec, name: &str, args: &[std::ffi::OsString]) -> bool {
    spec.exe_names.iter().any(|n| name.eq_ignore_ascii_case(n))
        && !args.is_empty()
        && !args.iter().skip(1).any(|arg| {
            arg.to_str()
                .is_some_and(|s| spec.exclude_subcommands.contains(&s))
        })
}

fn depth(system: &System, mut pid: Pid, root: Pid) -> Option<usize> {
    // 有界遍历也防住 PID 重用产生的坏父链/环。
    for depth in 0..system.processes().len() {
        if pid == root {
            return Some(depth);
        }
        pid = system.process(pid)?.parent()?;
    }
    None
}

fn find_cli_details(
    system: &System,
    root: u32,
    spec: &ProcessWatchSpec,
) -> Option<(Identity, Vec<String>, Option<PathBuf>)> {
    system
        .processes()
        .iter()
        .filter(|(_, process)| matches_cli(spec, &process.name().to_string_lossy(), process.cmd()))
        .filter_map(|(&pid, process)| {
            Some((
                depth(system, pid, Pid::from_u32(root))?,
                identity(system, pid)?,
                process,
            ))
        })
        // 优先最外层 CLI,避免把它调用的子进程当作整个会话。
        .min_by_key(|(depth, process, _)| (*depth, process.pid))
        .map(|(_, identity, process)| {
            let args = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            let cwd = process.cwd().map(Path::to_path_buf);
            (identity, args, cwd)
        })
}

fn find_cli(system: &System, root: u32, spec: &ProcessWatchSpec) -> Option<Identity> {
    find_cli_details(system, root, spec).map(|(process, _, _)| process)
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

fn paths_related(a: &Path, b: &Path) -> bool {
    if a.starts_with(b) || b.starts_with(a) {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca.starts_with(&cb) || cb.starts_with(&ca),
        _ => false,
    }
}

/// 0 = 不匹配; 3 = CLI 进程 cwd 精确匹配; 2 = 终端初始 cwd 精确匹配; 1 = 父子目录关系。
fn cwd_match_rank(candidate: &CliCandidate, target: &Path) -> u8 {
    if candidate.cwd.as_deref().is_some_and(|p| paths_match(p, target)) {
        return 3;
    }
    if candidate
        .terminal_cwd
        .as_deref()
        .is_some_and(|p| paths_match(p, target))
    {
        return 2;
    }
    if candidate.cwd.as_deref().is_some_and(|p| paths_related(p, target))
        || candidate
            .terminal_cwd
            .as_deref()
            .is_some_and(|p| paths_related(p, target))
    {
        return 1;
    }
    0
}

fn select_candidate(
    candidates: &[CliCandidate],
    agent_session_id: &str,
    payload_cwd: Option<&Path>,
    header_session: Option<u64>,
    supersedes: bool,
) -> Option<u64> {
    if candidates.is_empty() {
        return None;
    }
    // 1. 已经绑定到当前 agent_session_id 的终端直接命中。
    if let Some(c) = candidates
        .iter()
        .find(|c| c.bound_agent_session_id.as_deref() == Some(agent_session_id))
    {
        return Some(c.session_id);
    }

    let prefer_key = |c: &CliCandidate, rank: u8| {
        (
            rank,
            Some(c.session_id) == header_session,
            c.identity.started_at,
            c.session_id,
        )
    };

    // 2. CLI 命令行参数显式包含该会话 id(例如 `resume <id>`),且该终端尚未被同家其他会话占用。
    let has_arg = |c: &CliCandidate| c.args.iter().skip(1).any(|a| a == agent_session_id);
    if let Some(c) = candidates
        .iter()
        .filter(|c| c.bound_agent_session_id.is_none() && has_arg(c))
        .max_by_key(|c| prefer_key(c, 1))
    {
        return Some(c.session_id);
    }

    // 3. 按 payload.cwd 与终端内 CLI 进程(或终端本身)的工作目录匹配。
    if let Some(target_cwd) = payload_cwd {
        // 3a. 优先选择未绑定其他会话的 CLI 终端(支持同目录多开终端时把新会话绑到新起的 CLI)。
        if let Some(c) = candidates
            .iter()
            .filter(|c| c.bound_agent_session_id.is_none())
            .filter_map(|c| {
                let rank = cwd_match_rank(c, target_cwd);
                (rank > 0).then_some((c, rank))
            })
            .max_by_key(|(c, rank)| prefer_key(c, *rank))
            .map(|(c, _)| c)
        {
            return Some(c.session_id);
        }

        // 3b. 用户在同一个 TUI 内执行 /new、/clear、/fork、/resume 切换会话时,
        // 终端原先已绑定旧会话 id,仅在顶替类事件(SessionStart / UserPromptSubmit)下允许接管。
        if supersedes {
            if let Some(c) = candidates
                .iter()
                .filter_map(|c| {
                    let rank = cwd_match_rank(c, target_cwd);
                    (rank > 0).then_some((c, rank))
                })
                .max_by_key(|(c, rank)| prefer_key(c, *rank))
                .map(|(c, _)| c)
            {
                return Some(c.session_id);
            }
        }

        if candidates
            .iter()
            .any(|c| c.cwd.is_some() || c.terminal_cwd.is_some())
        {
            return None;
        }
    }

    // 4. payload 未带 cwd(或无法读取进程 cwd)时的兜底。
    let unbound: Vec<&CliCandidate> = candidates
        .iter()
        .filter(|c| c.bound_agent_session_id.is_none())
        .collect();
    if let Some(c) = unbound
        .iter()
        .copied()
        .find(|c| Some(c.session_id) == header_session)
    {
        return Some(c.session_id);
    }
    if unbound.len() == 1 {
        return Some(unbound[0].session_id);
    }
    if supersedes {
        if let Some(c) = candidates
            .iter()
            .find(|c| Some(c.session_id) == header_session)
        {
            return Some(c.session_id);
        }
        if candidates.len() == 1 {
            return Some(candidates[0].session_id);
        }
    }
    None
}

/// 解析 hook 事件应当归属的托管终端会话。
///
/// 对非共享守护进程的 agent(`adapter.process_watch().is_none()`),直接采信 `X-Gateterm-Session` 请求头;
/// 对带常驻共享守护进程的 agent(例如 `app-server --managed-daemon`),hook 子进程继承的是**第一次拉起守护进程**
/// 的那个旧终端(甚或外部终端)的环境,`X-Gateterm-Session` 可能是早已关闭的旧会话 id。
/// 因此需要根据当前存活的托管终端及其内部的 CLI 进程来定位真实会话。
pub(super) async fn resolve_session(
    state: &AppState,
    adapter: &dyn super::AgentAdapter,
    agent_session_id: &str,
    event: &str,
    payload: &serde_json::Value,
    header_session: Option<u64>,
) -> Option<u64> {
    let Some(spec) = adapter.process_watch() else {
        return header_session;
    };
    let live_terminal = |id: u64| state.manager.get(id).is_some_and(|s| !s.has_exited());
    let existing = state
        .agent_sessions
        .snapshot(adapter.id(), agent_session_id)
        .filter(|s| s.state != gateterm_protocol::AgentState::Done)
        .and_then(|s| s.session.filter(|&id| live_terminal(id)).map(|id| (id, s.process)));
    if let Some((id, process)) = existing
        && (event != "SessionStart" || process.is_none())
    {
        return Some(id);
    }

    let supersedes = matches!(event, "SessionStart" | "UserPromptSubmit");
    if supersedes || !state.agent_sessions.is_retired(adapter.id(), agent_session_id) {
        let launched = state
            .agent_sessions
            .history
            .lock()
            .unwrap()
            .launched
            .get(&(adapter.id().to_string(), agent_session_id.to_string()))
            .copied()
            .filter(|&id| {
                live_terminal(id)
                    && !state.agent_sessions.list().iter().any(|s| {
                        s.session == Some(id)
                            && s.agent_id == adapter.id()
                            && s.agent_session_id != agent_session_id
                    })
            });
        if let Some(id) = launched {
            return Some(id);
        }
    }

    let terminals: Vec<(u64, u32, Option<PathBuf>)> = state
        .manager
        .list()
        .into_iter()
        .filter_map(|info| {
            let session = state.manager.get(info.id)?;
            if session.has_exited() {
                return None;
            }
            Some((info.id, session.process_id?, info.cwd.map(PathBuf::from)))
        })
        .collect();
    if terminals.is_empty() {
        return existing.map(|(id, _)| id).or(header_session);
    }

    let bound_by_terminal: HashMap<u64, String> = state
        .agent_sessions
        .list()
        .into_iter()
        .filter(|s| s.agent_id == adapter.id() && s.state != gateterm_protocol::AgentState::Done)
        .filter_map(|s| Some((s.session?, s.agent_session_id)))
        .collect();

    let payload_cwd = payload
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let target_session_id = agent_session_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut system = System::new();
        refresh_with_cwd(&mut system, ProcessesToUpdate::All);
        let candidates: Vec<CliCandidate> = terminals
            .into_iter()
            .filter_map(|(session_id, root, terminal_cwd)| {
                let (identity, args, cwd) = find_cli_details(&system, root, &spec)?;
                Some(CliCandidate {
                    session_id,
                    identity,
                    args,
                    cwd,
                    terminal_cwd,
                    bound_agent_session_id: bound_by_terminal.get(&session_id).cloned(),
                })
            })
            .collect();
        select_candidate(
            &candidates,
            &target_session_id,
            payload_cwd.as_deref(),
            header_session,
            supersedes,
        )
    })
    .await
    .ok()
    .flatten()
}

pub(super) async fn watch_agent(
    state: &AppState,
    adapter: &dyn super::AgentAdapter,
    session: &AgentSession,
    event: &str,
) {
    let Some(spec) = adapter.process_watch() else {
        return;
    };
    if session.state == gateterm_protocol::AgentState::Done
        || (session.process.is_some() && event != "SessionStart")
    {
        return;
    }
    let Some(root) = session
        .session
        .and_then(|id| state.manager.get(id))
        .and_then(|s| s.process_id)
    else {
        return;
    };
    let Ok(Some(process)) = tokio::task::spawn_blocking(move || {
        let mut system = System::new();
        refresh(&mut system, ProcessesToUpdate::All);
        find_cli(&system, root, &spec)
    })
    .await
    else {
        return;
    };

    if state.agent_sessions.bind_process(session, process) {
        spawn_watch(
            &state.agent_sessions,
            &state.manager,
            session.clone(),
            process,
        );
    }
}

fn spawn_watch(
    store: &Arc<AgentSessionStore>,
    manager: &Arc<SessionManager>,
    session: AgentSession,
    process: Identity,
) {
    let store = Arc::downgrade(store);
    let manager = Arc::downgrade(manager);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let (Some(store), Some(manager)) = (store.upgrade(), manager.upgrade()) else {
                break;
            };
            let current = store.snapshot(&session.agent_id, &session.agent_session_id);
            if !current.is_some_and(|s| s.session == session.session && s.process == Some(process))
            {
                break;
            }
            let Ok(alive) = tokio::task::spawn_blocking(move || {
                let mut system = System::new();
                let pid = Pid::from_u32(process.pid);
                refresh(&mut system, ProcessesToUpdate::Some(&[pid]));
                identity(&system, pid) == Some(process)
            })
            .await
            else {
                continue;
            };
            if !alive {
                if let Some(gone) = store.finish_process(&session, process) {
                    tracing::info!(session = ?gone.session, pid = process.pid, "Codex CLI exited");
                    manager.publish(gone.to_msg());
                }
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::store::Update;
    use super::*;
    use gateterm_protocol::{AgentState, ServerMsg};

    const CODEX_SPEC: ProcessWatchSpec = ProcessWatchSpec {
        exe_names: &["codex", "codex.exe"],
        exclude_subcommands: &["app-server", "exec-server", "mcp-server"],
    };

    #[test]
    fn shared_daemon_is_not_the_terminal_cli() {
        let args = |args: &[&str]| {
            args.iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };
        assert!(matches_cli(
            &CODEX_SPEC,
            "codex.exe",
            &args(&["codex.exe", "resume", "session-1"])
        ));
        assert!(matches_cli(&CODEX_SPEC, "codex", &args(&["codex", "--no-alt-screen"])));
        assert!(!matches_cli(
            &CODEX_SPEC,
            "codex.exe",
            &args(&["codex.exe", "app-server", "--managed-daemon"])
        ));
        assert!(!matches_cli(&CODEX_SPEC, "node.exe", &args(&["node.exe", "codex.js"])));
        assert!(
            !matches_cli(&CODEX_SPEC, "codex", &[]),
            "缺少命令行时不能把 daemon 猜成 CLI"
        );
    }

    #[test]
    fn process_snapshot_has_the_current_process_and_its_parent() {
        let mut system = System::new();
        refresh(&mut system, ProcessesToUpdate::All);
        let pid = Pid::from_u32(std::process::id());
        assert_eq!(identity(&system, pid).unwrap().pid, std::process::id());
        let parent = system.process(pid).unwrap().parent().unwrap();
        assert_eq!(depth(&system, pid, parent), Some(1));
        assert_eq!(depth(&system, parent, pid), None);
    }

    #[tokio::test]
    async fn cli_exit_broadcasts_done_while_terminal_stays_alive() {
        let manager = SessionManager::new(1 << 16, Default::default());
        let terminal = manager.create(None, None, None).unwrap();
        let store = Arc::new(AgentSessionStore::new());
        let session = store.apply("codex", "cx", Some(terminal.id), Update::default());
        let mut events = manager.events();
        #[cfg(windows)]
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 60"])
            .spawn()
            .unwrap();
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = Pid::from_u32(child.id());
        let mut system = System::new();
        refresh(&mut system, ProcessesToUpdate::Some(&[pid]));
        let process = identity(&system, pid).unwrap();
        assert!(store.bind_process(&session, process));
        spawn_watch(&store, &manager, session, process);

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(
            store.snapshot("codex", "cx").is_some(),
            "运行中的 CLI 必须保留"
        );
        child.kill().unwrap();
        child.wait().unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await;
        let terminal_alive = manager.get(terminal.id).is_some_and(|s| !s.has_exited());
        manager.kill(terminal.id);
        assert!(matches!(
            event.unwrap().unwrap(),
            ServerMsg::AgentEvent {
                state: AgentState::Done,
                ..
            }
        ));
        assert!(store.snapshot("codex", "cx").is_none());
        assert!(terminal_alive, "agent 退出不能关闭终端");
    }

    #[test]
    fn old_watcher_cannot_remove_a_resumed_session() {
        let store = AgentSessionStore::new();
        let session = store.apply("codex", "cx", Some(1), Update::default());
        let old = Identity {
            pid: 10,
            started_at: 100,
        };
        let new = Identity {
            pid: 10,
            started_at: 200,
        };
        assert!(store.bind_process(&session, old));
        assert!(store.bind_process(&session, new));
        assert!(store.finish_process(&session, old).is_none());
        assert_eq!(store.snapshot("codex", "cx").unwrap().process, Some(new));
    }

    #[test]
    fn late_daemon_events_cannot_reopen_an_exited_cli_but_resume_can() {
        let store = AgentSessionStore::new();
        let session = store.apply("codex", "cx", Some(1), Update::default());
        let process = Identity {
            pid: 10,
            started_at: 100,
        };
        assert!(store.bind_process(&session, process));
        assert!(store.finish_process(&session, process).is_some());
        let late = store.apply(
            "codex",
            "cx",
            Some(1),
            Update {
                state: Some(AgentState::Working),
                ..Default::default()
            },
        );
        assert_eq!(late.state, AgentState::Done);
        assert!(store.list().is_empty());
        let (resumed, _) = store.apply_superseding("codex", "cx", Some(1), Update::default());
        assert_eq!(resumed.state, AgentState::Idle);
        assert!(resumed.process.is_none());
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn daemon_candidate_selection_overrides_stale_header_and_handles_thread_switches() {
        let candidates = vec![
            CliCandidate {
                session_id: 2,
                identity: Identity { pid: 200, started_at: 10 },
                args: vec!["codex".into(), "resume".into(), "thread-old".into()],
                cwd: Some(PathBuf::from("/work/futures")),
                terminal_cwd: Some(PathBuf::from("/Users/hiro")),
                bound_agent_session_id: None,
            },
            CliCandidate {
                session_id: 5,
                identity: Identity { pid: 500, started_at: 20 },
                args: vec!["codex".into()],
                cwd: Some(PathBuf::from("/work/other")),
                terminal_cwd: Some(PathBuf::from("/work/other")),
                bound_agent_session_id: Some("thread-other".into()),
            },
        ];

        // 守护进程带着早已关闭的 X-Gateterm-Session: 29,应当按 CLI 的 cwd 绑定到会话 2。
        assert_eq!(
            select_candidate(
                &candidates,
                "thread-new",
                Some(Path::new("/work/futures")),
                Some(29),
                false,
            ),
            Some(2)
        );

        // 会话 2 绑定到 thread-new 后,thread-old 发来的晚到非顶替事件(Stop/SessionEnd)
        // 即使匹配了启动命令行里的 resume 参数,也不能把会话 2 抢回去。
        let mut bound = candidates.clone();
        bound[0].bound_agent_session_id = Some("thread-new".into());
        assert_eq!(
            select_candidate(
                &bound,
                "thread-old",
                Some(Path::new("/work/futures")),
                Some(29),
                false,
            ),
            None
        );

        // 用户在会话 2 的 TUI 里再次 /new 切到 thread-3(SessionStart, supersedes=true)时允许接管。
        assert_eq!(
            select_candidate(
                &bound,
                "thread-3",
                Some(Path::new("/work/futures")),
                Some(29),
                true,
            ),
            Some(2)
        );

        // 外部终端(如 iTerm2)在无关目录启动的 Codex 触发守护进程 hook 时,不误绑任何托管终端。
        assert_eq!(
            select_candidate(
                &bound,
                "external-thread",
                Some(Path::new("/work/external")),
                Some(29),
                true,
            ),
            None
        );
    }
}
