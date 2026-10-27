//! Codex TUI 可以退出而共享 app-server 继续活着,这时 SessionEnd 不一定立即到达。
//! hook 仍负责识别会话;这里只在成功找到托管终端内的 Codex CLI 后监视它的退出。

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

fn refresh(system: &mut System, processes: ProcessesToUpdate<'_>) {
    system.refresh_processes_specifics(
        processes,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::OnlyIfNotSet),
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

fn is_codex_cli(name: &str, args: &[std::ffi::OsString]) -> bool {
    (name.eq_ignore_ascii_case("codex") || name.eq_ignore_ascii_case("codex.exe"))
        && !args.is_empty()
        && !args.iter().skip(1).any(|arg| {
            matches!(
                arg.to_str(),
                Some("app-server" | "exec-server" | "mcp-server")
            )
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

fn find_codex(system: &System, root: u32) -> Option<Identity> {
    system
        .processes()
        .iter()
        .filter(|(_, process)| is_codex_cli(&process.name().to_string_lossy(), process.cmd()))
        .filter_map(|(&pid, _)| {
            Some((
                depth(system, pid, Pid::from_u32(root))?,
                identity(system, pid)?,
            ))
        })
        // 优先最外层 CLI,避免把它调用的 codex exec 子进程当作整个会话。
        .min_by_key(|(depth, process)| (*depth, process.pid))
        .map(|(_, process)| process)
}

pub(super) async fn watch_codex(state: &AppState, session: &AgentSession, event: &str) {
    if session.agent_id != "codex"
        || session.state == gateterm_protocol::AgentState::Done
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
        find_codex(&system, root)
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

    #[test]
    fn shared_daemon_is_not_the_terminal_cli() {
        let args = |args: &[&str]| {
            args.iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };
        assert!(is_codex_cli(
            "codex.exe",
            &args(&["codex.exe", "resume", "session-1"])
        ));
        assert!(is_codex_cli("codex", &args(&["codex", "--no-alt-screen"])));
        assert!(!is_codex_cli(
            "codex.exe",
            &args(&["codex.exe", "app-server", "--managed-daemon"])
        ));
        assert!(!is_codex_cli("node.exe", &args(&["node.exe", "codex.js"])));
        assert!(
            !is_codex_cli("codex", &[]),
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
}
