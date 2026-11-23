//! 服务端机器的系统剪贴板 → 远端客户端(目前只有 Windows)。
//!
//! OSC 52 那条路(见 `session::osc52`)只覆盖「会往终端里发 OSC 52」的程序。很多
//! agent 在本机会话里只写系统剪贴板:Codex 用 arboard(Win32 API),agy 用
//! atotto/clipboard,在 Windows 上都不发 OSC 52。这里监听系统剪贴板的变化,判断这次
//! 写入是不是**某个会话里的进程**干的,是就交给那个会话,由它投给最近一次输入的
//! attach(`Session::deliver_clipboard`)—— 仍然是点对点,不广播。
//!
//! 归属判断(`attribute`)分两档:
//!
//! 1. 剪贴板有 owner 窗口(`GetClipboardOwner`):取它所属进程,沿父进程链往上找,
//!    碰到某个会话的根进程就归那个会话;找不到就说明是会话外的程序(你在服务端
//!    电脑上自己复制的东西),**不转发**。
//! 2. 没有 owner —— arboard / atotto 这类库用 `OpenClipboard(NULL)` 打开剪贴板,
//!    `EmptyClipboard` 之后 owner 就是 NULL,系统里没有别的信息能说明是谁写的。
//!    这时只能按时间判断:最近 [`NULL_OWNER_WINDOW`] 内有远端输入的会话里取最近的
//!    那个;但如果服务端电脑本机的键鼠输入比那次远端输入还新,就认为是本机用户
//!    复制的,不转发。owner 进程已经退出(PowerShell `Set-Clipboard` 这种一次性
//!    进程)时同样走这一档。

use std::sync::Arc;
use std::time::Duration;

use crate::session::manager::SessionManager;

/// 无 owner 的剪贴板写入,距离最近一次远端输入多久以内才认。拖选复制在鼠标抬起时
/// 立刻发生,`/copy` 在回车后立刻发生(一次性子进程冷启动如 PowerShell 也在此窗口内)。
pub const NULL_OWNER_WINDOW: Duration = Duration::from_millis(3000);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Attribution {
    Session(u64),
    Drop(&'static str),
}

#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub id: u64,
    pub root_pid: Option<u32>,
    pub since_input: Option<Duration>,
}

/// 判断进程名是否为 Windows ConPTY 控制台宿主(`conhost.exe` / `OpenConsole.exe`)。
pub(crate) fn is_conhost_exe(name: &str) -> bool {
    name.eq_ignore_ascii_case("conhost.exe") || name.eq_ignore_ascii_case("OpenConsole.exe")
}

/// 判断一次系统剪贴板写入属于哪个会话。
///
/// - `ancestors(pid)`:进程存在时返回 `[pid, 父, 祖父, …]`,不存在返回 `None`;
/// - `is_conhost(pid)`:判断进程是否为 ConPTY 控制台宿主(`conhost.exe` / `OpenConsole.exe`);
/// - `local_idle`:服务端电脑本机键鼠空闲了多久(拿不到为 `None`)。
pub(crate) fn attribute(
    owner_pid: Option<u32>,
    self_pid: u32,
    sessions: &[Candidate],
    ancestors: impl Fn(u32) -> Option<Vec<u32>>,
    is_conhost: impl Fn(u32) -> bool,
    local_idle: Option<Duration>,
) -> Attribution {
    if let Some(pid) = owner_pid {
        if pid == self_pid {
            return Attribution::Drop("written by gateterm itself");
        }
        if let Some(chain) = ancestors(pid) {
            if let Some(s) = sessions
                .iter()
                .find(|s| s.root_pid.is_some_and(|root| chain.contains(&root)))
            {
                return Attribution::Session(s.id);
            }
            if !(chain.len() >= 2 && chain[1] == self_pid && is_conhost(pid)) {
                return Attribution::Drop("owner process is outside every session");
            }
            // ConPTY (conhost.exe / OpenConsole.exe) 作为 gateterm 的直接子进程存在,
            // 不在某个 session.root_pid 子进程树下;退回到按输入时间判断。
        }
        // owner 进程已经退出(或为本服务的 ConPTY 宿主):退回到按输入时间判断
    }
    let Some((id, since)) = sessions
        .iter()
        .filter_map(|s| Some((s.id, s.since_input?)))
        .min_by_key(|(_, since)| *since)
    else {
        return Attribution::Drop("no session has seen remote input");
    };
    if since > NULL_OWNER_WINDOW {
        return Attribution::Drop("no recent remote input");
    }
    if local_idle.is_some_and(|local| local < since) {
        return Attribution::Drop("local input is more recent than remote input");
    }
    Attribution::Session(id)
}

fn candidates(manager: &SessionManager) -> Vec<(Candidate, Arc<crate::session::pty::Session>)> {
    manager
        .sessions()
        .into_iter()
        .map(|s| {
            (
                Candidate {
                    id: s.id,
                    root_pid: s.process_id,
                    since_input: s.since_last_input(),
                },
                s,
            )
        })
        .collect()
}

/// 启动系统剪贴板监听。重复调用只生效一次;非 Windows 平台什么也不做。
pub fn start_host_watch(manager: Arc<SessionManager>) {
    #[cfg(windows)]
    win::start(manager);
    #[cfg(not(windows))]
    let _ = manager;
}

#[cfg(windows)]
mod win {
    use std::sync::mpsc;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{
        AddClipboardFormatListener, CloseClipboard, GetClipboardData, GetClipboardOwner,
        GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowThreadProcessId,
        HWND_MESSAGE, MSG, RegisterClassW, TranslateMessage, WM_CLIPBOARDUPDATE, WNDCLASSW,
    };

    use super::{Attribution, attribute, candidates};
    use crate::session::manager::SessionManager;

    /// 单条剪贴板文本上限(UTF-16 字节)。投递时还有 1 MiB UTF-8 的上限。
    const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

    struct Change {
        seq: u32,
        owner_pid: Option<u32>,
    }

    static EVENTS: OnceLock<mpsc::Sender<Change>> = OnceLock::new();

    pub(super) fn start(manager: Arc<SessionManager>) {
        let (tx, rx) = mpsc::channel();
        if EVENTS.set(tx).is_err() {
            return; // 已经在跑了
        }
        let spawned = std::thread::Builder::new()
            .name("clipboard-watch".into())
            .spawn(message_loop)
            .and_then(|_| {
                std::thread::Builder::new()
                    .name("clipboard-route".into())
                    .spawn(move || route_loop(manager, rx))
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "clipboard watcher thread failed to start");
        }
    }

    /// owner 必须在通知到达时立刻取:等工作线程慢慢处理,一次性进程早就退出了。
    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_CLIPBOARDUPDATE {
            unsafe {
                let owner = GetClipboardOwner();
                let mut pid = 0u32;
                if !owner.is_null() {
                    GetWindowThreadProcessId(owner, &mut pid);
                }
                let seq = GetClipboardSequenceNumber();
                if let Some(tx) = EVENTS.get() {
                    let _ = tx.send(Change {
                        seq,
                        owner_pid: (pid != 0).then_some(pid),
                    });
                }
            }
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    fn message_loop() {
        let class: Vec<u16> = "GatetermClipboardWatch\0".encode_utf16().collect();
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                tracing::warn!("clipboard watcher: CreateWindowExW failed");
                return;
            }
            if AddClipboardFormatListener(hwnd) == 0 {
                tracing::warn!("clipboard watcher: AddClipboardFormatListener failed");
                return;
            }
            tracing::info!("clipboard watcher started");
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ReadOutcome {
        Ok(String),
        Empty,
        NoTextFormat,
        Busy,
    }

    impl ReadOutcome {
        fn should_advance_seq(&self) -> bool {
            !matches!(self, Self::Busy)
        }
    }

    const OPEN_CLIPBOARD_RETRIES: usize = 20;
    const OPEN_CLIPBOARD_RETRY_DELAY: Duration = Duration::from_millis(15);

    fn route_loop(manager: Arc<SessionManager>, rx: mpsc::Receiver<Change>) {
        let mut last_seq = 0u32;
        let mut system = System::new();
        let self_pid = std::process::id();
        while let Ok(change) = rx.recv() {
            if change.seq == last_seq {
                continue;
            }
            let sessions = candidates(&manager);
            if sessions.is_empty() {
                last_seq = change.seq;
                continue;
            }
            let list: Vec<_> = sessions.iter().map(|(c, _)| c.clone()).collect();
            if change.owner_pid.is_some() {
                system.refresh_processes_specifics(
                    ProcessesToUpdate::All,
                    true,
                    ProcessRefreshKind::nothing().without_tasks(),
                );
            }
            let ancestors = |pid: u32| -> Option<Vec<u32>> {
                let mut cur = system.process(Pid::from_u32(pid))?;
                let mut chain = vec![pid];
                while chain.len() < 64 {
                    let Some(parent) = cur.parent() else { break };
                    if chain.contains(&parent.as_u32()) {
                        break;
                    }
                    chain.push(parent.as_u32());
                    match system.process(parent) {
                        Some(p) => cur = p,
                        None => break,
                    }
                }
                Some(chain)
            };
            let is_conhost = |pid: u32| -> bool {
                system
                    .process(Pid::from_u32(pid))
                    .is_some_and(|p| super::is_conhost_exe(&p.name().to_string_lossy()))
            };
            let verdict = attribute(
                change.owner_pid,
                self_pid,
                &list,
                ancestors,
                is_conhost,
                local_idle(),
            );
            let Attribution::Session(id) = verdict else {
                last_seq = change.seq;
                if let Attribution::Drop(why) = verdict {
                    tracing::debug!(owner = ?change.owner_pid, why, "host clipboard change ignored");
                }
                continue;
            };
            let outcome = read_text();
            if outcome.should_advance_seq() {
                last_seq = change.seq;
            }
            match outcome {
                ReadOutcome::Ok(text) => {
                    if let Some((_, session)) = sessions.iter().find(|(c, _)| c.id == id) {
                        session.deliver_clipboard(text);
                    }
                }
                other => {
                    tracing::debug!(
                        seq = change.seq,
                        owner = ?change.owner_pid,
                        outcome = ?other,
                        "host clipboard read skipped"
                    );
                }
            }
        }
    }

    /// 服务端电脑本机键鼠空闲时长。ConPTY 的输入不经过这里,只反映坐在那台电脑前的人。
    fn local_idle() -> Option<Duration> {
        unsafe {
            let mut info = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut info) == 0 {
                return None;
            }
            Some(Duration::from_millis(
                GetTickCount().wrapping_sub(info.dwTime) as u64
            ))
        }
    }

    fn read_text() -> ReadOutcome {
        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT as u32) == 0 {
                return ReadOutcome::NoTextFormat;
            }
            // 刚写完的程序可能还没放手,重试几次(最多 20 × 15ms = 300ms)
            let mut opened = false;
            for attempt in 0..OPEN_CLIPBOARD_RETRIES {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    opened = true;
                    break;
                }
                if attempt + 1 < OPEN_CLIPBOARD_RETRIES {
                    std::thread::sleep(OPEN_CLIPBOARD_RETRY_DELAY);
                }
            }
            if !opened {
                return ReadOutcome::Busy;
            }
            let outcome = (|| {
                let handle = GetClipboardData(CF_UNICODETEXT as u32);
                if handle.is_null() {
                    return ReadOutcome::NoTextFormat;
                }
                let size = GlobalSize(handle);
                if size == 0 || size > MAX_TEXT_BYTES {
                    return ReadOutcome::Empty;
                }
                let ptr = GlobalLock(handle) as *const u16;
                if ptr.is_null() {
                    return ReadOutcome::Empty;
                }
                let units = std::slice::from_raw_parts(ptr, size / 2);
                let len = units.iter().position(|&u| u == 0).unwrap_or(units.len());
                let s = String::from_utf16_lossy(&units[..len]);
                GlobalUnlock(handle);
                if s.is_empty() {
                    ReadOutcome::Empty
                } else {
                    ReadOutcome::Ok(s)
                }
            })();
            CloseClipboard();
            outcome
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn read_outcome_only_skips_last_seq_update_when_busy() {
            assert!(!ReadOutcome::Busy.should_advance_seq());
            assert!(ReadOutcome::Empty.should_advance_seq());
            assert!(ReadOutcome::NoTextFormat.should_advance_seq());
            assert!(ReadOutcome::Ok("text".into()).should_advance_seq());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: u64, root: u32, since_ms: Option<u64>) -> Candidate {
        Candidate {
            id,
            root_pid: Some(root),
            since_input: since_ms.map(Duration::from_millis),
        }
    }

    /// 进程树:100(会话1 根)→ 101 → 102;200(会话2 根)→ 201;900 是会话外的程序
    fn tree(pid: u32) -> Option<Vec<u32>> {
        match pid {
            102 => Some(vec![102, 101, 100, 1]),
            201 => Some(vec![201, 200, 1]),
            900 => Some(vec![900, 1]),
            _ => None,
        }
    }

    fn no_conhost(_: u32) -> bool {
        false
    }

    #[test]
    fn owner_inside_a_session_tree_goes_to_that_session_regardless_of_timing() {
        let sessions = [cand(1, 100, None), cand(2, 200, Some(10))];
        assert_eq!(attribute(Some(102), 7, &sessions, tree, no_conhost, None), Attribution::Session(1));
        assert_eq!(attribute(Some(201), 7, &sessions, tree, no_conhost, Some(Duration::ZERO)), Attribution::Session(2));
    }

    #[test]
    fn owner_outside_every_session_is_never_forwarded() {
        let sessions = [cand(1, 100, Some(5))];
        assert!(matches!(attribute(Some(900), 7, &sessions, tree, no_conhost, None), Attribution::Drop(_)));
        assert!(matches!(attribute(Some(7), 7, &sessions, tree, no_conhost, None), Attribution::Drop(_)));
    }

    #[test]
    fn null_owner_goes_to_the_most_recent_remote_input_within_the_window() {
        let sessions = [cand(1, 100, Some(1500)), cand(2, 200, Some(300))];
        assert_eq!(attribute(None, 7, &sessions, tree, no_conhost, None), Attribution::Session(2));
        // 退出的 owner 进程同样按时间判断
        assert_eq!(attribute(Some(555), 7, &sessions, tree, no_conhost, Some(Duration::from_secs(60))), Attribution::Session(2));
    }

    #[test]
    fn null_owner_is_dropped_when_input_is_stale_or_local_user_is_more_recent() {
        let stale = [cand(1, 100, Some(5000))];
        assert!(matches!(attribute(None, 7, &stale, tree, no_conhost, None), Attribution::Drop(_)));
        let fresh = [cand(1, 100, Some(800))];
        assert!(matches!(
            attribute(None, 7, &fresh, tree, no_conhost, Some(Duration::from_millis(100))),
            Attribution::Drop(_)
        ));
        assert!(matches!(attribute(None, 7, &[cand(1, 100, None)], tree, no_conhost, None), Attribution::Drop(_)));
    }

    #[test]
    fn conhost_child_of_self_falls_back_to_input_window_while_external_process_stays_dropped() {
        // 进程树: 50 是 self(7) 直接拉起的 conhost.exe / OpenConsole.exe;
        // 51 是 self(7) 的非 conhost 子进程; 60 是外部终端的 conhost.exe; 900 是外部普通进程。
        fn conhost_tree(pid: u32) -> Option<Vec<u32>> {
            match pid {
                50 => Some(vec![50, 7, 1]),
                51 => Some(vec![51, 7, 1]),
                60 => Some(vec![60, 999, 1]),
                900 => Some(vec![900, 1]),
                _ => None,
            }
        }
        fn is_conhost(pid: u32) -> bool {
            matches!(pid, 50 | 60)
        }

        let sessions = [cand(1, 100, Some(1500)), cand(2, 200, Some(300))];
        // conhost 直接子进程 + local_idle > since_input -> 走 NULL_OWNER_WINDOW 路由到最近会话
        assert_eq!(
            attribute(Some(50), 7, &sessions, conhost_tree, is_conhost, Some(Duration::from_secs(60))),
            Attribution::Session(2)
        );
        // 本机键鼠比远端输入更新 -> 丢弃
        assert!(matches!(
            attribute(Some(50), 7, &sessions, conhost_tree, is_conhost, Some(Duration::from_millis(100))),
            Attribution::Drop(_)
        ));
        // 远端输入已超过 NULL_OWNER_WINDOW -> 丢弃
        let stale = [cand(1, 100, Some(5000))];
        assert!(matches!(
            attribute(Some(50), 7, &stale, conhost_tree, is_conhost, None),
            Attribution::Drop(_)
        ));
        // 外部进程 / 外部 conhost / 非 conhost 子进程一律严格丢弃
        assert_eq!(
            attribute(Some(900), 7, &sessions, conhost_tree, is_conhost, None),
            Attribution::Drop("owner process is outside every session")
        );
        assert_eq!(
            attribute(Some(60), 7, &sessions, conhost_tree, is_conhost, None),
            Attribution::Drop("owner process is outside every session")
        );
        assert_eq!(
            attribute(Some(51), 7, &sessions, conhost_tree, is_conhost, None),
            Attribution::Drop("owner process is outside every session")
        );
    }

    #[test]
    fn is_conhost_exe_matches_console_hosts_case_insensitively() {
        assert!(is_conhost_exe("conhost.exe"));
        assert!(is_conhost_exe("CONHOST.EXE"));
        assert!(is_conhost_exe("OpenConsole.exe"));
        assert!(is_conhost_exe("openconsole.exe"));
        assert!(!is_conhost_exe("notepad.exe"));
        assert!(!is_conhost_exe("RustDesk.exe"));
        assert!(!is_conhost_exe(""));
    }
}
