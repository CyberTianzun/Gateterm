//! 「本机装没装这个 agent」。
//!
//! 规则是**三元证据**,不靠单一目录 —— 因为「`~/.claude` 存在」什么都证明不了:
//! 任何写过那个目录的程序都会创建它,gateterm 自己装 hook 就会。参考实现正是栽在
//! 这里,最后不得不把 claude-code 整个从默认检测里排除掉。
//!
//! 按可信度从高到低:配置文件里有**非我方**内容 > CLI 在 PATH 上 > 目录里有
//! 别的东西 > 没有。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
#[cfg(any(unix, test))]
use std::sync::{Mutex, RwLock};

#[cfg(any(unix, test))]
const PATH_MARKER_START: &[u8] = b"__GATETERM_SHELL_PATH_START__";
#[cfg(any(unix, test))]
const PATH_MARKER_END: &[u8] = b"__GATETERM_SHELL_PATH_END__";
#[cfg(unix)]
const SHELL_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(unix)]
const SHELL_PROBE_FALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
#[cfg(unix)]
const MAX_SHELL_PROBE_OUTPUT: usize = 256 * 1024;
#[cfg(unix)]
const SHELL_PROBE_GUARD_ENV: &str = "GATETERM_PATH_PROBE";

/// 检测所依赖的外部环境。**PATH 必须显式传入**,不在函数里读全局环境 ——
/// 否则单测会读到开发机上真实存在的 `claude`,把「没装」的用例全部染成「装了」。
pub struct DetectEnv {
    pub home: PathBuf,
    pub path: Option<OsString>,
}

#[cfg(any(unix, test))]
struct ShellPathCache {
    cached: RwLock<Option<Option<OsString>>>,
    probe_lock: Mutex<()>,
}

#[cfg(any(unix, test))]
impl ShellPathCache {
    const fn new() -> Self {
        Self {
            cached: RwLock::new(None),
            probe_lock: Mutex::new(()),
        }
    }

    fn get_or_probe<F>(&self, refresh: bool, probe: F) -> Option<OsString>
    where
        F: FnOnce() -> Option<OsString>,
    {
        if !refresh
            && let Some(cached) = self.cached.read().unwrap().as_ref()
        {
            return cached.clone();
        }
        let _guard = self.probe_lock.lock().unwrap();
        if !refresh
            && let Some(cached) = self.cached.read().unwrap().as_ref()
        {
            return cached.clone();
        }
        let probed = probe();
        let mut slot = self.cached.write().unwrap();
        match (&*slot, probed) {
            (_, Some(path)) => {
                *slot = Some(Some(path.clone()));
                Some(path)
            }
            (Some(Some(existing)), None) => Some(existing.clone()),
            (_, None) => {
                *slot = Some(None);
                None
            }
        }
    }
}

#[cfg(unix)]
static SHELL_PATH_CACHE: ShellPathCache = ShellPathCache::new();

#[cfg(any(unix, test))]
fn shell_probe_invocations(shell: &str) -> Vec<Vec<String>> {
    let start = std::str::from_utf8(PATH_MARKER_START).expect("ASCII marker");
    let end = std::str::from_utf8(PATH_MARKER_END).expect("ASCII marker");
    let name = shell
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(shell)
        .to_ascii_lowercase();
    match name.as_str() {
        "fish" => {
            let cmd = format!("printf '{start}%s{end}' (string join : $PATH)");
            vec![
                vec!["-l".into(), "-i".into(), "-c".into(), cmd.clone()],
                vec!["-l".into(), "-c".into(), cmd],
            ]
        }
        "nu" | "nushell" => {
            let cmd = format!("print $\"{start}($env.PATH | str join ':'){end}\"");
            vec![
                vec!["-l".into(), "-i".into(), "-c".into(), cmd.clone()],
                vec!["-l".into(), "-c".into(), cmd],
            ]
        }
        "csh" | "tcsh" => {
            let cmd = format!("printf '{start}%s{end}' \"$PATH\"");
            vec![
                vec!["-i".into(), "-c".into(), cmd.clone()],
                vec!["-c".into(), cmd],
            ]
        }
        _ => {
            let cmd = format!("printf '{start}%s{end}' \"$PATH\"");
            vec![
                vec!["-l".into(), "-i".into(), "-c".into(), cmd.clone()],
                vec!["-l".into(), "-c".into(), cmd],
            ]
        }
    }
}

#[cfg(any(unix, test))]
fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

#[cfg(any(unix, test))]
fn extract_marked_bytes(output: &[u8]) -> Option<&[u8]> {
    let end = rfind_subslice(output, PATH_MARKER_END)?;
    let before_end = &output[..end];
    let start = rfind_subslice(before_end, PATH_MARKER_START)?;
    let payload = &before_end[start + PATH_MARKER_START.len()..];
    (!payload.is_empty()).then_some(payload)
}

#[cfg(any(unix, test))]
fn append_tail_bounded(buf: &mut Vec<u8>, chunk: &[u8], cap: usize) {
    if cap == 0 || chunk.is_empty() {
        return;
    }
    if chunk.len() >= cap {
        buf.clear();
        buf.extend_from_slice(&chunk[chunk.len() - cap..]);
        return;
    }
    let total = buf.len() + chunk.len();
    if total > cap {
        let drop_len = total - cap;
        buf.drain(..drop_len);
    }
    buf.extend_from_slice(chunk);
}

#[cfg(unix)]
fn resolve_user_shell() -> Option<PathBuf> {
    if let Some(shell) = std::env::var_os("SHELL")
        && !shell.is_empty()
    {
        let path = PathBuf::from(shell);
        if path.is_file() {
            return Some(path);
        }
    }
    ["/bin/zsh", "/bin/bash", "/bin/sh"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

#[cfg(unix)]
fn wait_or_kill(child: &mut std::process::Child, deadline: std::time::Instant) {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

#[cfg(unix)]
fn run_shell_probe(
    shell: &Path,
    args: &[String],
    home: &Path,
    timeout: std::time::Duration,
) -> Option<OsString> {
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Instant;

    let mut cmd = Command::new(shell);
    cmd.args(args)
        .env(SHELL_PROBE_GUARD_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if home.is_dir() {
        cmd.current_dir(home);
        if std::env::var_os("HOME").is_none() {
            cmd.env("HOME", home);
        }
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let deadline = Instant::now() + timeout;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(chunk[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    let mut output = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(chunk) => append_tail_bounded(&mut output, &chunk, MAX_SHELL_PROBE_OUTPUT),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                wait_or_kill(&mut child, deadline);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let bytes = extract_marked_bytes(&output)?;
    Some(OsString::from_vec(bytes.to_vec()))
}

#[cfg(unix)]
fn probe_user_shell_path(home: &Path) -> Option<OsString> {
    if std::env::var_os(SHELL_PROBE_GUARD_ENV).is_some() {
        return None;
    }
    let shell = resolve_user_shell()?;
    let invocations = shell_probe_invocations(&shell.to_string_lossy());
    for (idx, args) in invocations.iter().enumerate() {
        let timeout = if idx == 0 {
            SHELL_PROBE_TIMEOUT
        } else {
            SHELL_PROBE_FALLBACK_TIMEOUT
        };
        if let Some(path) = run_shell_probe(&shell, args, home, timeout) {
            tracing::debug!(shell = %shell.display(), attempt = idx + 1, "resolved user shell PATH");
            return Some(path);
        }
    }
    tracing::debug!(shell = %shell.display(), "user shell PATH probe failed, using process PATH and fallbacks");
    None
}

#[cfg(unix)]
fn cached_shell_path(home: &Path, refresh: bool) -> Option<OsString> {
    SHELL_PATH_CACHE.get_or_probe(refresh, || probe_user_shell_path(home))
}

#[cfg(not(unix))]
fn cached_shell_path(_home: &Path, _refresh: bool) -> Option<OsString> {
    None
}

fn merge_path_dirs(
    process_dirs: Vec<PathBuf>,
    shell_dirs: Option<Vec<PathBuf>>,
    fallback_dirs: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(shell_dirs) = shell_dirs {
        for dir in shell_dirs {
            if dir.is_absolute() && dir.is_dir() && !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    for dir in process_dirs {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    for dir in fallback_dirs {
        if dir.is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

impl DetectEnv {
    /// 只读用户主目录,不触发 shell PATH 探测。适合只需要读写 `~/.agent/` 配置的路径。
    pub fn current_home() -> PathBuf {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn resolve(refresh_shell_path: bool) -> Self {
        let home = Self::current_home();
        let process_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let shell_dirs = cached_shell_path(&home, refresh_shell_path)
            .map(|p| std::env::split_paths(&p).collect());
        let dirs = merge_path_dirs(process_dirs, shell_dirs, fallback_bin_dirs(&home));
        let path = (!dirs.is_empty())
            .then(|| std::env::join_paths(dirs).ok())
            .flatten()
            .or_else(|| std::env::var_os("PATH"));
        Self { home, path }
    }

    /// 真实进程环境。读不出 home 时回落到当前目录,让扫描退化成「什么都没找到」
    /// 而不是整个失败。桌面 GUI 应用 / launchd / systemd 后台服务启动时,进程 PATH
    /// 往往只有系统默认目录,不含用户 shell 配置里追加的工具路径(如自定义 `NODE_HOME`、
    /// nvm、fnm、Homebrew、`~/.local/bin`),因此在 Unix 上会探测并缓存用户登录交互
    /// shell 的 PATH,同时保留进程 PATH 与常见目录兜底。
    pub fn current() -> Self {
        Self::resolve(false)
    }

    /// 与 [`Self::current`] 相同,但会强制重新探测一次用户 shell PATH(用于显式刷新历史等场景)。
    pub fn current_refreshed() -> Self {
        Self::resolve(true)
    }
}

fn fallback_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        home.join(".cargo").join("bin"),
    ];
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(windows)]
    {
        dirs.push(home.join("AppData").join("Roaming").join("npm"));
        dirs.push(
            home.join("AppData")
                .join("Local")
                .join("Microsoft")
                .join("WinGet")
                .join("Links"),
        );
    }
    dirs
}

/// 在 PATH 上找一个可执行文件。Windows 上还要试 `.cmd` / `.exe` —— agent 的
/// CLI 基本都是 npm 装的,那边落地的是 `.cmd` shim 而不是裸名字。
pub fn on_path(env: &DetectEnv, exe: &str) -> Option<PathBuf> {
    let path = env.path.as_ref()?;
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{exe}.cmd"), format!("{exe}.exe"), format!("{exe}.bat"), exe.to_string()]
    } else {
        vec![exe.to_string()]
    };
    std::env::split_paths(path).find_map(|dir| {
        names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
    })
}

/// 目录里除了这些之外还有东西吗。用来把「只有我们写过的配置」和「用户真的用过」
/// 区分开。
pub fn dir_has_entries_besides(dir: &Path, ignore: &[&str]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        // `.DS_Store` 这类噪音不算证据
        !ignore.contains(&name.as_ref()) && !name.starts_with('.')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn extract_marked_bytes_ignores_noisy_shell_banner_and_trailing_output() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"Welcome to zsh!\n\x1b[32mbanner\x1b[0m\n");
        raw.extend_from_slice(b"echoed: ");
        raw.extend_from_slice(PATH_MARKER_START);
        raw.extend_from_slice(b"%s");
        raw.extend_from_slice(PATH_MARKER_END);
        raw.extend_from_slice(b"\n");
        raw.extend_from_slice(PATH_MARKER_START);
        raw.extend_from_slice(b"/Users/hiro/frameworks/node/bin:/usr/bin:/bin");
        raw.extend_from_slice(PATH_MARKER_END);
        raw.extend_from_slice(b"\x1b]0;title\x07\n");

        assert_eq!(
            extract_marked_bytes(&raw),
            Some(&b"/Users/hiro/frameworks/node/bin:/usr/bin:/bin"[..])
        );

        let empty_payload = [PATH_MARKER_START, PATH_MARKER_END].concat();
        assert_eq!(extract_marked_bytes(&empty_payload), None);
        assert_eq!(extract_marked_bytes(b"no markers at all"), None);
    }

    #[test]
    fn append_tail_bounded_preserves_marker_past_output_cap() {
        let cap = 128;
        let mut buf = Vec::new();
        for _ in 0..10 {
            append_tail_bounded(&mut buf, &[b'x'; 40], cap);
        }
        let marked = [
            PATH_MARKER_START,
            b"/custom/node/bin:/usr/bin",
            PATH_MARKER_END,
        ]
        .concat();
        append_tail_bounded(&mut buf, &marked, cap);
        assert!(buf.len() <= cap, "buffer exceeded cap: {}", buf.len());
        assert_eq!(
            extract_marked_bytes(&buf),
            Some(&b"/custom/node/bin:/usr/bin"[..])
        );
    }

    #[test]
    fn shell_probe_invocations_match_shell_dialects() {
        let zsh = shell_probe_invocations("/bin/zsh");
        assert_eq!(zsh.len(), 2);
        assert_eq!(&zsh[0][..3], &["-l", "-i", "-c"]);
        assert_eq!(&zsh[1][..2], &["-l", "-c"]);
        assert!(zsh[0][3].contains("$PATH"));

        let bash = shell_probe_invocations("/usr/bin/bash");
        assert_eq!(bash.len(), 2);
        assert_eq!(&bash[0][..3], &["-l", "-i", "-c"]);
        assert_eq!(&bash[1][..2], &["-l", "-c"]);

        let fish = shell_probe_invocations("/opt/homebrew/bin/fish");
        assert_eq!(fish.len(), 2);
        assert_eq!(&fish[0][..3], &["-l", "-i", "-c"]);
        assert!(fish[0][3].contains("string join : $PATH"));

        let tcsh = shell_probe_invocations("/bin/tcsh");
        assert_eq!(tcsh.len(), 2);
        assert_eq!(&tcsh[0][..2], &["-i", "-c"]);
        assert_eq!(&tcsh[1][..1], &["-c"]);

        let nu = shell_probe_invocations("/opt/homebrew/bin/nu");
        assert_eq!(nu.len(), 2);
        assert_eq!(&nu[0][..3], &["-l", "-i", "-c"]);
        assert!(nu[0][3].contains("$env.PATH"));
    }

    #[test]
    fn merge_path_dirs_prefers_shell_path_drops_relative_entries_and_finds_custom_cli() {
        let tmp = tempfile::tempdir().unwrap();
        let sys_bin = tmp.path().join("sys_bin");
        let node_bin = tmp.path().join("frameworks").join("node-v22").join("bin");
        let local_bin = tmp.path().join("local_bin");
        let missing_dir = tmp.path().join("does_not_exist");
        fs::create_dir_all(&sys_bin).unwrap();
        fs::create_dir_all(&node_bin).unwrap();
        fs::create_dir_all(&local_bin).unwrap();

        let claude_name = if cfg!(windows) { "claude.cmd" } else { "claude" };
        let expected_claude = node_bin.join(claude_name);
        fs::write(&expected_claude, "shim").unwrap();

        let merged = merge_path_dirs(
            vec![sys_bin.clone()],
            Some(vec![
                node_bin.clone(),
                PathBuf::from("."),
                PathBuf::from(""),
                missing_dir.clone(),
                sys_bin.clone(),
                node_bin.clone(),
            ]),
            vec![local_bin.clone(), tmp.path().join("missing_fallback")],
        );

        assert_eq!(merged, vec![node_bin.clone(), sys_bin.clone(), local_bin.clone()]);

        let env = DetectEnv {
            home: tmp.path().to_path_buf(),
            path: Some(std::env::join_paths(&merged).unwrap()),
        };
        assert_eq!(on_path(&env, "claude"), Some(expected_claude));

        // When shell probing fails, process PATH + existing fallbacks are still preserved.
        let fallback_only = merge_path_dirs(
            vec![sys_bin.clone()],
            None,
            vec![local_bin.clone(), missing_dir],
        );
        assert_eq!(fallback_only, vec![sys_bin, local_bin]);
    }

    #[test]
    fn shell_path_cache_probes_once_refreshes_on_demand_and_keeps_prior_on_failed_refresh() {
        let cache = ShellPathCache::new();
        let calls = AtomicUsize::new(0);

        let first = cache.get_or_probe(false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(OsString::from("/custom/node/bin:/usr/bin"))
        });
        assert_eq!(first, Some(OsString::from("/custom/node/bin:/usr/bin")));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let second = cache.get_or_probe(false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(OsString::from("/should/not/run"))
        });
        assert_eq!(second, Some(OsString::from("/custom/node/bin:/usr/bin")));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Failed refresh preserves the previously resolved valid PATH.
        let failed_refresh = cache.get_or_probe(true, || {
            calls.fetch_add(1, Ordering::SeqCst);
            None
        });
        assert_eq!(failed_refresh, Some(OsString::from("/custom/node/bin:/usr/bin")));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // Successful refresh replaces the cached value.
        let updated = cache.get_or_probe(true, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(OsString::from("/new/bin:/usr/bin"))
        });
        assert_eq!(updated, Some(OsString::from("/new/bin:/usr/bin")));
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // A failed cold probe is cached as None until explicitly refreshed.
        let cold_fail = ShellPathCache::new();
        let fail_calls = AtomicUsize::new(0);
        assert_eq!(
            cold_fail.get_or_probe(false, || {
                fail_calls.fetch_add(1, Ordering::SeqCst);
                None
            }),
            None
        );
        assert_eq!(
            cold_fail.get_or_probe(false, || {
                fail_calls.fetch_add(1, Ordering::SeqCst);
                Some(OsString::from("/never"))
            }),
            None
        );
        assert_eq!(fail_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            cold_fail.get_or_probe(true, || {
                fail_calls.fetch_add(1, Ordering::SeqCst);
                Some(OsString::from("/recovered/bin"))
            }),
            Some(OsString::from("/recovered/bin"))
        );
        assert_eq!(fail_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn shell_path_cache_serializes_concurrent_cold_probes() {
        use std::sync::{Arc, Barrier};
        use std::time::Duration;

        let cache = Arc::new(ShellPathCache::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(4));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let cache = Arc::clone(&cache);
            let calls = Arc::clone(&calls);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                cache.get_or_probe(false, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(30));
                    Some(OsString::from("/shared/node/bin:/usr/bin"))
                })
            }));
        }

        for handle in handles {
            assert_eq!(
                handle.join().unwrap(),
                Some(OsString::from("/shared/node/bin:/usr/bin"))
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[test]
    fn unix_shell_probe_extracts_path_through_banner_and_enforces_timeout() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let custom_bin = tmp.path().join("frameworks/node-v22/bin");
        fs::create_dir_all(&custom_bin).unwrap();
        fs::write(custom_bin.join("claude"), "#!/bin/sh\n").unwrap();

        // Simulate a user shell whose rc prints a banner and exports a custom NODE_HOME/bin.
        let fake_zsh = tmp.path().join("zsh");
        fs::write(
            &fake_zsh,
            format!(
                "#!/bin/sh\nprintf 'noisy banner\\n'\nexport PATH='{}:/usr/bin:/bin'\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '-c' ]; then shift; eval \"$1\"; exit $?; fi\n  shift\ndone\n",
                custom_bin.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_zsh, fs::Permissions::from_mode(0o755)).unwrap();

        let invocations = shell_probe_invocations(&fake_zsh.to_string_lossy());
        let probed = run_shell_probe(&fake_zsh, &invocations[0], tmp.path(), Duration::from_secs(2))
            .expect("should extract PATH from shell output");
        let shell_dirs: Vec<PathBuf> = std::env::split_paths(&probed).collect();
        let merged = merge_path_dirs(vec![PathBuf::from("/usr/bin")], Some(shell_dirs), Vec::new());
        let env = DetectEnv {
            home: tmp.path().to_path_buf(),
            path: Some(std::env::join_paths(&merged).unwrap()),
        };
        assert_eq!(on_path(&env, "claude"), Some(custom_bin.join("claude")));

        // A hanging shell must be killed promptly once timeout elapses.
        let hanging_shell = tmp.path().join("hang-sh");
        fs::write(&hanging_shell, "#!/bin/sh\nexec sleep 30\n").unwrap();
        fs::set_permissions(&hanging_shell, fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        let timed_out = run_shell_probe(
            &hanging_shell,
            &invocations[0],
            tmp.path(),
            Duration::from_millis(150),
        );
        assert_eq!(timed_out, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
