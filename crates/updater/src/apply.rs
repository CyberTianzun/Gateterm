//! Detached in-place binary replacement, backup rollback, relaunch, and startup cleanup.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;

use crate::StagedUpdate;

/// CLI flag identifying an in-place self-update helper invocation.
pub const APPLY_UPDATE_FLAG: &str = "--apply-update";

/// Filename inside `cache_root` where rollback/apply diagnostics are recorded for the next launch.
pub const ERROR_LOG_FILENAME: &str = "last-apply-error.txt";

const PARENT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SWAP_RETRY_ATTEMPTS: usize = 50;
const DEFAULT_SWAP_RETRY_DELAY: Duration = Duration::from_millis(200);

#[cfg(target_os = "linux")]
const LINUX_PID_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(any(target_os = "linux", test))]
const GUI_ENV_VARS: [&str; 4] = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
];

#[cfg(windows)]
const DETACHED_CREATION_FLAGS: u32 = windows_sys::Win32::System::Threading::CREATE_NO_WINDOW
    | windows_sys::Win32::System::Threading::DETACHED_PROCESS
    | windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

#[cfg(windows)]
const _: () = assert!(DETACHED_CREATION_FLAGS == (0x0800_0000 | 0x0000_0008 | 0x0000_0200));

/// Parameters passed to the staged `--apply-update` helper process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyRequest {
    pub parent_pid: u32,
    pub target_exe: PathBuf,
    pub source_bin: PathBuf,
    pub backup_exe: PathBuf,
    pub error_log: PathBuf,
    pub relaunch: bool,
}

/// Errors specific to detached self-update apply dispatch.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("self-update apply is not supported on macOS")]
    UnsupportedPlatform,
}

#[derive(Debug, Clone, Copy)]
struct RetryPolicy {
    max_attempts: usize,
    delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_SWAP_RETRY_ATTEMPTS,
            delay: DEFAULT_SWAP_RETRY_DELAY,
        }
    }
}

#[derive(Debug)]
struct SwapFailure {
    err: anyhow::Error,
    can_relaunch: bool,
}

struct RemoveFileOnDrop<'a> {
    path: &'a Path,
    active: bool,
}

impl Drop for RemoveFileOnDrop<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

/// Serializes `req` into CLI arguments starting with [`APPLY_UPDATE_FLAG`].
pub fn build_apply_args(req: &ApplyRequest) -> Vec<String> {
    build_apply_os_args(req)
        .into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

fn build_apply_os_args(req: &ApplyRequest) -> Vec<OsString> {
    let mut args = vec![
        OsString::from(APPLY_UPDATE_FLAG),
        OsString::from("--pid"),
        OsString::from(req.parent_pid.to_string()),
        OsString::from("--target"),
        req.target_exe.as_os_str().to_os_string(),
        OsString::from("--source"),
        req.source_bin.as_os_str().to_os_string(),
        OsString::from("--backup"),
        req.backup_exe.as_os_str().to_os_string(),
        OsString::from("--error-log"),
        req.error_log.as_os_str().to_os_string(),
    ];
    if req.relaunch {
        args.push(OsString::from("--relaunch"));
    }
    args
}

/// Parses `--apply-update` CLI arguments from `args` (including `argv[0]`).
/// Returns `Ok(None)` when the first argument after the executable is absent or not `--apply-update`.
pub fn parse_apply_args<I: IntoIterator<Item = String>>(
    args: I,
) -> anyhow::Result<Option<ApplyRequest>> {
    parse_apply_args_os(args.into_iter().map(OsString::from))
}

fn parse_apply_args_os<I: IntoIterator<Item = OsString>>(
    args: I,
) -> anyhow::Result<Option<ApplyRequest>> {
    let mut iter = args.into_iter();
    let Some(_exe) = iter.next() else {
        return Ok(None);
    };
    let Some(first) = iter.next() else {
        return Ok(None);
    };
    if first != OsStr::new(APPLY_UPDATE_FLAG) {
        return Ok(None);
    }

    let mut parent_pid: Option<u32> = None;
    let mut target_exe: Option<PathBuf> = None;
    let mut source_bin: Option<PathBuf> = None;
    let mut backup_exe: Option<PathBuf> = None;
    let mut error_log: Option<PathBuf> = None;
    let mut relaunch = false;

    while let Some(flag) = iter.next() {
        match flag.to_str() {
            Some("--pid") => {
                anyhow::ensure!(parent_pid.is_none(), "duplicate --pid flag");
                let raw = iter.next().context("missing value for --pid")?;
                let text = raw.to_str().context("--pid value must be valid UTF-8")?;
                let parsed = text
                    .parse::<u32>()
                    .with_context(|| format!("invalid --pid value: {text}"))?;
                parent_pid = Some(parsed);
            }
            Some("--target") => {
                set_path_flag(&mut target_exe, &mut iter, "--target")?;
            }
            Some("--source") => {
                set_path_flag(&mut source_bin, &mut iter, "--source")?;
            }
            Some("--backup") => {
                set_path_flag(&mut backup_exe, &mut iter, "--backup")?;
            }
            Some("--error-log") => {
                set_path_flag(&mut error_log, &mut iter, "--error-log")?;
            }
            Some("--relaunch") => {
                anyhow::ensure!(!relaunch, "duplicate --relaunch flag");
                relaunch = true;
            }
            _ => {
                anyhow::bail!(
                    "unknown --apply-update argument: {}",
                    flag.to_string_lossy()
                );
            }
        }
    }

    Ok(Some(ApplyRequest {
        parent_pid: parent_pid.context("missing required --pid flag")?,
        target_exe: target_exe.context("missing required --target flag")?,
        source_bin: source_bin.context("missing required --source flag")?,
        backup_exe: backup_exe.context("missing required --backup flag")?,
        error_log: error_log.context("missing required --error-log flag")?,
        relaunch,
    }))
}

fn set_path_flag<I: Iterator<Item = OsString>>(
    slot: &mut Option<PathBuf>,
    iter: &mut I,
    name: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(slot.is_none(), "duplicate {name} flag");
    let raw = iter
        .next()
        .with_context(|| format!("missing value for {name}"))?;
    anyhow::ensure!(!raw.is_empty(), "{name} value must not be empty");
    *slot = Some(PathBuf::from(raw));
    Ok(())
}

/// Top-of-`main()` hook: if `--apply-update` is the first argument, executes the swap helper
/// and returns `Some(exit_code)` so the caller can `std::process::exit(code)` immediately.
pub fn try_run_from_env() -> Option<i32> {
    try_run_from_args_os(std::env::args_os())
}

fn try_run_from_args_os<I: IntoIterator<Item = OsString>>(args: I) -> Option<i32> {
    match parse_apply_args_os(args) {
        Ok(None) => None,
        Ok(Some(req)) => Some(run_apply(&req)),
        Err(err) => {
            eprintln!("gateterm --apply-update argument error: {err:#}");
            Some(2)
        }
    }
}

/// Spawns the detached staged `--apply-update` helper to replace `target_exe` after the current
/// process exits. Returns [`ApplyError::UnsupportedPlatform`] on macOS.
pub fn spawn_apply_helper(
    staged: &StagedUpdate,
    target_exe: &Path,
    cache_root: &Path,
) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = (staged, target_exe, cache_root);
        Err(ApplyError::UnsupportedPlatform.into())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (staged, target_exe, cache_root);
        anyhow::bail!("self-update apply is not supported on this platform");
    }
    #[cfg(any(windows, target_os = "linux"))]
    {
        let req = build_helper_request(staged, target_exe, cache_root)?;
        anyhow::ensure!(
            staged.binary_path.is_file(),
            "staged update binary not found at {}",
            staged.binary_path.display()
        );
        std::fs::create_dir_all(cache_root).with_context(|| {
            format!(
                "failed to create updater cache directory {}",
                cache_root.display()
            )
        })?;
        let args = build_apply_os_args(&req);
        let child = spawn_detached_command(&staged.binary_path, &args, Some(cache_root))
            .with_context(|| {
                format!(
                    "failed to spawn apply helper {}",
                    staged.binary_path.display()
                )
            })?;
        drop(child);
        Ok(())
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn build_helper_request(
    staged: &StagedUpdate,
    target_exe: &Path,
    cache_root: &Path,
) -> anyhow::Result<ApplyRequest> {
    let resolved_target = resolve_target_path(target_exe)?;
    let backup_exe = backup_path_for(&resolved_target);
    let error_log = error_log_path(cache_root);
    Ok(ApplyRequest {
        parent_pid: std::process::id(),
        target_exe: resolved_target,
        source_bin: staged.binary_path.clone(),
        backup_exe,
        error_log,
        relaunch: true,
    })
}

#[cfg(any(windows, target_os = "linux"))]
fn resolve_target_path(target_exe: &Path) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(target_exe).with_context(|| {
        format!(
            "failed to resolve target executable path {}",
            target_exe.display()
        )
    })?;
    #[cfg(windows)]
    {
        Ok(strip_windows_verbatim_prefix(canonical))
    }
    #[cfg(not(windows))]
    {
        Ok(canonical)
    }
}

#[cfg(windows)]
fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    if let Some(s) = path.to_str()
        && let Some(stripped) = s.strip_prefix(r"\\?\")
        && stripped.as_bytes().get(1) == Some(&b':')
        && stripped.len() < 260
    {
        return PathBuf::from(stripped);
    }
    path
}

/// Runs the full `--apply-update` flow: waits for `req.parent_pid`, replaces `req.target_exe`
/// atomically via a same-directory `.update-new` file with backup rollback on failure, logs
/// diagnostics to `req.error_log` on failure, and relaunches `req.target_exe` when `req.relaunch`
/// is true.
pub fn run_apply(req: &ApplyRequest) -> i32 {
    let _ = std::fs::remove_file(&req.error_log);

    if let Err(err) = wait_for_parent_exit(req.parent_pid) {
        let msg = format!(
            "failed waiting for parent process {}: {err:#}",
            req.parent_pid
        );
        let _ = write_error_log(&req.error_log, &msg);
        tracing::error!("{msg}");
        return 1;
    }

    match swap_with_rollback_and_ops(req, RetryPolicy::default(), |from, to| {
        std::fs::rename(from, to)
    }) {
        Ok(()) => {
            if req.relaunch
                && let Err(err) = relaunch_target(&req.target_exe)
            {
                let msg = format!(
                    "updated {}, but failed to relaunch: {err:#}",
                    req.target_exe.display()
                );
                let _ = write_error_log(&req.error_log, &msg);
                tracing::error!("{msg}");
                return 1;
            }
            0
        }
        Err(failure) => {
            tracing::error!(error = %format!("{:#}", failure.err), "in-place update swap failed");
            if req.relaunch
                && failure.can_relaunch
                && let Err(relaunch_err) = relaunch_target(&req.target_exe)
            {
                let msg = format!(
                    "{:#}\nfailed to relaunch restored executable {}: {relaunch_err:#}",
                    failure.err,
                    req.target_exe.display()
                );
                let _ = write_error_log(&req.error_log, &msg);
                tracing::error!(error = %format!("{relaunch_err:#}"), "failed to relaunch rolled-back executable");
            }
            1
        }
    }
}

/// Best-effort startup cleanup: removes `<current_exe>.bak`, stale `staging-*` directories in
/// `cache_root`, and consumes `last-apply-error.txt` returning `Some(message)` if a previous
/// update rolled back with an error.
pub fn cleanup_after_start(current_exe: &Path, cache_root: &Path) -> Option<String> {
    let _ = std::fs::remove_file(backup_path_for(current_exe));
    if let Ok(temp_new) = temp_new_path_for(current_exe) {
        let _ = std::fs::remove_file(temp_new);
    }
    if let Ok(canonical) = std::fs::canonicalize(current_exe)
        && canonical != current_exe
    {
        let _ = std::fs::remove_file(backup_path_for(&canonical));
        if let Ok(temp_new) = temp_new_path_for(&canonical) {
            let _ = std::fs::remove_file(temp_new);
        }
    }

    if let Ok(entries) = std::fs::read_dir(cache_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("staging-")
                && entry.file_type().is_ok_and(|ft| ft.is_dir())
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    let err_path = error_log_path(cache_root);
    let message = match std::fs::read(&err_path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).trim().to_owned();
            if text.is_empty() { None } else { Some(text) }
        }
        Err(_) => None,
    };
    let _ = std::fs::remove_file(&err_path);
    message
}

fn backup_path_for(exe: &Path) -> PathBuf {
    let mut os = exe.as_os_str().to_os_string();
    os.push(".bak");
    PathBuf::from(os)
}

fn error_log_path(cache_root: &Path) -> PathBuf {
    cache_root.join(ERROR_LOG_FILENAME)
}

fn temp_new_path_for(target_exe: &Path) -> anyhow::Result<PathBuf> {
    let parent = target_exe
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .with_context(|| {
            format!(
                "target executable {} has no parent directory",
                target_exe.display()
            )
        })?;
    let name = target_exe
        .file_name()
        .filter(|n| !n.is_empty())
        .with_context(|| {
            format!(
                "target executable {} has no file name",
                target_exe.display()
            )
        })?;
    let mut temp_name = OsString::from(".");
    temp_name.push(name);
    temp_name.push(".update-new");
    Ok(parent.join(temp_name))
}

fn swap_with_rollback_and_ops<F>(
    req: &ApplyRequest,
    retry: RetryPolicy,
    mut rename_fn: F,
) -> Result<(), SwapFailure>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    if req.target_exe == req.backup_exe {
        let err = anyhow::anyhow!(
            "target and backup paths must be distinct: {}",
            req.target_exe.display()
        );
        let _ = write_error_log(&req.error_log, &format!("{err:#}"));
        return Err(SwapFailure {
            err,
            can_relaunch: req.target_exe.exists(),
        });
    }

    let temp_new = match temp_new_path_for(&req.target_exe) {
        Ok(path) => path,
        Err(err) => {
            let _ = write_error_log(&req.error_log, &format!("{err:#}"));
            return Err(SwapFailure {
                err,
                can_relaunch: req.target_exe.exists(),
            });
        }
    };

    let _ = std::fs::remove_file(&temp_new);
    let mut temp_guard = RemoveFileOnDrop {
        path: &temp_new,
        active: true,
    };

    if let Err(err) = copy_staged_binary(&req.source_bin, &temp_new) {
        let msg = format!("{err:#}");
        let _ = write_error_log(&req.error_log, &msg);
        return Err(SwapFailure {
            err,
            can_relaunch: req.target_exe.exists(),
        });
    }

    if let Err(err) = remove_file_with_retry(&req.backup_exe, retry) {
        let wrapped = anyhow::Error::new(err).context(format!(
            "failed to remove stale backup {}",
            req.backup_exe.display()
        ));
        let _ = write_error_log(&req.error_log, &format!("{wrapped:#}"));
        return Err(SwapFailure {
            err: wrapped,
            can_relaunch: req.target_exe.exists(),
        });
    }

    if let Err(err) = retry_io(retry, || rename_fn(&req.target_exe, &req.backup_exe)) {
        let wrapped = anyhow::Error::new(err).context(format!(
            "failed to back up {} to {}",
            req.target_exe.display(),
            req.backup_exe.display()
        ));
        let _ = write_error_log(&req.error_log, &format!("{wrapped:#}"));
        return Err(SwapFailure {
            err: wrapped,
            can_relaunch: req.target_exe.exists(),
        });
    }

    if let Err(swap_err) = retry_io(retry, || rename_fn(&temp_new, &req.target_exe)) {
        match retry_io(retry, || rename_fn(&req.backup_exe, &req.target_exe)) {
            Ok(()) => {
                let err = anyhow::Error::new(swap_err).context(format!(
                    "failed to move staged binary {} into {}; rolled back previous executable from {}",
                    temp_new.display(),
                    req.target_exe.display(),
                    req.backup_exe.display()
                ));
                let _ = write_error_log(&req.error_log, &format!("{err:#}"));
                return Err(SwapFailure {
                    err,
                    can_relaunch: true,
                });
            }
            Err(rb_err) => {
                let err = anyhow::anyhow!(
                    "failed to move staged binary {} into {}: {swap_err}; rollback from {} also failed: {rb_err}",
                    temp_new.display(),
                    req.target_exe.display(),
                    req.backup_exe.display()
                );
                let _ = write_error_log(&req.error_log, &format!("{err:#}"));
                return Err(SwapFailure {
                    err,
                    can_relaunch: false,
                });
            }
        }
    }

    temp_guard.active = false;
    Ok(())
}

fn copy_staged_binary(source_bin: &Path, temp_new: &Path) -> anyhow::Result<()> {
    {
        let mut src = std::fs::File::open(source_bin)
            .with_context(|| format!("failed to open staged binary {}", source_bin.display()))?;
        let mut dst = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(temp_new)
            .with_context(|| format!("failed to create staging file {}", temp_new.display()))?;
        std::io::copy(&mut src, &mut dst).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source_bin.display(),
                temp_new.display()
            )
        })?;
        dst.flush()
            .with_context(|| format!("failed to flush {}", temp_new.display()))?;
        dst.sync_all()
            .with_context(|| format!("failed to sync {}", temp_new.display()))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp_new, std::fs::Permissions::from_mode(0o755)).with_context(
            || {
                format!(
                    "failed to set executable permissions on {}",
                    temp_new.display()
                )
            },
        )?;
    }

    Ok(())
}

fn is_retryable_io_error(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied || matches!(err.raw_os_error(), Some(5 | 32))
}

fn retry_io<F>(retry: RetryPolicy, mut op: F) -> std::io::Result<()>
where
    F: FnMut() -> std::io::Result<()>,
{
    let attempts = retry.max_attempts.max(1);
    for attempt in 0..attempts {
        match op() {
            Ok(()) => return Ok(()),
            Err(err) if is_retryable_io_error(&err) && attempt + 1 < attempts => {
                std::thread::sleep(retry.delay);
            }
            Err(err) => return Err(err),
        }
    }
    unreachable!("attempts >= 1 guarantees loop returns")
}

fn remove_file_with_retry(path: &Path, retry: RetryPolicy) -> std::io::Result<()> {
    retry_io(retry, || match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    })
}

fn write_error_log(error_log: &Path, message: &str) -> anyhow::Result<()> {
    if let Some(parent) = error_log.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create error log directory {}", parent.display())
        })?;
    }
    std::fs::write(error_log, message)
        .with_context(|| format!("failed to write error log {}", error_log.display()))
}

fn wait_for_parent_exit(pid: u32) -> anyhow::Result<()> {
    if pid == 0 || pid == std::process::id() {
        return Ok(());
    }
    wait_for_parent_exit_platform(pid)
}

#[cfg(windows)]
fn wait_for_parent_exit_platform(pid: u32) -> anyhow::Result<()> {
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a valid non-null Win32 handle opened by `OpenProcess`.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    // SAFETY: `OpenProcess` with `PROCESS_SYNCHRONIZE` and a valid PID returns either null (already
    // exited / inaccessible) or an owned process handle closed by `HandleGuard`.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return Ok(());
    }
    let _guard = HandleGuard(handle);

    let timeout_ms = u32::try_from(PARENT_WAIT_TIMEOUT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: `handle` is open with `PROCESS_SYNCHRONIZE` for the duration of this call.
    let status = unsafe { WaitForSingleObject(handle, timeout_ms) };
    match status {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => {
            anyhow::bail!("timed out after 30s waiting for parent process {pid} to exit")
        }
        WAIT_FAILED => {
            let err = std::io::Error::last_os_error();
            Err(err).with_context(|| format!("WaitForSingleObject failed for pid {pid}"))
        }
        other => anyhow::bail!("unexpected WaitForSingleObject status {other} for pid {pid}"),
    }
}

#[cfg(target_os = "linux")]
fn wait_for_parent_exit_platform(pid: u32) -> anyhow::Result<()> {
    let stat_path = format!("/proc/{pid}/stat");
    let deadline = std::time::Instant::now() + PARENT_WAIT_TIMEOUT;
    loop {
        match std::fs::read_to_string(&stat_path) {
            Ok(stat) => {
                if is_proc_stat_exited(&stat) {
                    return Ok(());
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound || err.raw_os_error() == Some(3) =>
            {
                return Ok(());
            }
            Err(err) => {
                return Err(err).with_context(|| format!("failed to inspect {stat_path}"));
            }
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("timed out after 30s waiting for parent process {pid} to exit");
        }
        std::thread::sleep(LINUX_PID_POLL_INTERVAL);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn wait_for_parent_exit_platform(_pid: u32) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Err(ApplyError::UnsupportedPlatform.into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        anyhow::bail!("self-update apply is not supported on this platform")
    }
}

#[cfg(any(target_os = "linux", test))]
fn is_proc_stat_exited(stat: &str) -> bool {
    let Some(rparen) = stat.rfind(')') else {
        return false;
    };
    let state = stat[rparen + 1..].trim_start().chars().next();
    matches!(state, Some('Z' | 'X'))
}

fn relaunch_target(target_exe: &Path) -> anyhow::Result<()> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        let parent = target_exe.parent().filter(|p| !p.as_os_str().is_empty());
        let child = spawn_detached_command(target_exe, &[], parent)?;
        drop(child);
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let _ = target_exe;
        Err(ApplyError::UnsupportedPlatform.into())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = target_exe;
        anyhow::bail!("self-update apply is not supported on this platform")
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn spawn_detached_command(
    program: &Path,
    args: &[OsString],
    current_dir: Option<&Path>,
) -> anyhow::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    configure_detached_process(&mut cmd);
    cmd.spawn()
        .with_context(|| format!("failed to spawn {}", program.display()))
}

#[cfg(windows)]
fn configure_detached_process(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(DETACHED_CREATION_FLAGS);
}

#[cfg(target_os = "linux")]
fn configure_detached_process(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    preserve_gui_env(cmd, |key| std::env::var_os(key));
    cmd.process_group(0);
}

#[cfg(any(target_os = "linux", test))]
fn preserve_gui_env<F>(cmd: &mut std::process::Command, mut lookup: F)
where
    F: FnMut(&str) -> Option<OsString>,
{
    for key in GUI_ENV_VARS {
        if let Some(val) = lookup(key) {
            cmd.env(key, val);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn apply_args_roundtrip_and_ignores_normal_or_hook_args() {
        assert!(
            parse_apply_args(["app".into(), "hook".into(), "http://127.0.0.1".into()])
                .unwrap()
                .is_none()
        );
        assert!(parse_apply_args(["app".into()]).unwrap().is_none());
        assert!(parse_apply_args(Vec::<String>::new()).unwrap().is_none());

        let req = ApplyRequest {
            parent_pid: 42,
            target_exe: "/a/bin".into(),
            source_bin: "/tmp/new".into(),
            backup_exe: "/a/bin.bak".into(),
            error_log: "/tmp/err.log".into(),
            relaunch: true,
        };
        let cli = build_apply_args(&req);
        let mut full = vec!["staged".into()];
        full.extend(cli);
        assert_eq!(parse_apply_args(full).unwrap().unwrap(), req);

        let no_relaunch = ApplyRequest {
            relaunch: false,
            ..req.clone()
        };
        let mut full_no_relaunch = vec!["staged".into()];
        full_no_relaunch.extend(build_apply_args(&no_relaunch));
        assert_eq!(
            parse_apply_args(full_no_relaunch).unwrap().unwrap(),
            no_relaunch
        );

        // Malformed --apply-update arguments fail explicitly.
        assert!(parse_apply_args(["staged".into(), APPLY_UPDATE_FLAG.into()]).is_err());
        assert!(
            parse_apply_args([
                "staged".into(),
                APPLY_UPDATE_FLAG.into(),
                "--pid".into(),
                "not-a-pid".into(),
            ])
            .is_err()
        );
        assert!(
            parse_apply_args([
                "staged".into(),
                APPLY_UPDATE_FLAG.into(),
                "--unknown".into(),
            ])
            .is_err()
        );

        // try_run_from_args_os ignores non-update launches without panic, including invalid UTF-8.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let invalid = OsString::from_vec(vec![0xff, 0xfe]);
            assert_eq!(try_run_from_args_os([OsString::from("app"), invalid]), None);
        }
        assert_eq!(
            try_run_from_args_os([OsString::from("app"), OsString::from("hook")]),
            None
        );
        assert_eq!(
            try_run_from_args_os([OsString::from("app"), OsString::from(APPLY_UPDATE_FLAG)]),
            Some(2)
        );
    }

    #[test]
    fn swap_binary_replaces_target_and_leaves_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target_exe = dir.path().join("gateterm-desktop-web.exe");
        let source_bin = dir.path().join("staged-bin.exe");
        let backup_exe = dir.path().join("gateterm-desktop-web.exe.bak");
        let error_log = dir.path().join("last-apply-error.txt");

        std::fs::write(&target_exe, b"old-v1").unwrap();
        std::fs::write(&source_bin, b"new-v2").unwrap();
        // Pre-existing stale backup must be replaced cleanly.
        std::fs::write(&backup_exe, b"ancient-v0").unwrap();
        // Pre-existing stale error log is cleared when a new apply run starts.
        std::fs::write(&error_log, b"stale error").unwrap();

        let req = ApplyRequest {
            parent_pid: 0,
            target_exe: target_exe.clone(),
            source_bin,
            backup_exe: backup_exe.clone(),
            error_log: error_log.clone(),
            relaunch: false,
        };

        assert_eq!(run_apply(&req), 0);
        assert_eq!(std::fs::read(&target_exe).unwrap(), b"new-v2");
        assert_eq!(std::fs::read(&backup_exe).unwrap(), b"old-v1");
        assert!(!error_log.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target_exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }

        let leftover_temp = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().ends_with(".update-new"));
        assert!(!leftover_temp, "leftover .update-new file must not remain");
    }

    #[test]
    fn swap_binary_retries_transient_lock_and_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let target_exe = dir.path().join("gateterm.exe");
        let source_bin = dir.path().join("staged-bin.exe");
        let backup_exe = dir.path().join("gateterm.exe.bak");
        let error_log = dir.path().join("last-apply-error.txt");

        std::fs::write(&target_exe, b"old-v1").unwrap();
        std::fs::write(&source_bin, b"new-v2").unwrap();

        let req = ApplyRequest {
            parent_pid: 0,
            target_exe: target_exe.clone(),
            source_bin,
            backup_exe: backup_exe.clone(),
            error_log: error_log.clone(),
            relaunch: false,
        };

        let attempts = AtomicUsize::new(0);
        let res = swap_with_rollback_and_ops(
            &req,
            RetryPolicy {
                max_attempts: 4,
                delay: Duration::from_millis(1),
            },
            |from, to| {
                if to == target_exe && from != backup_exe {
                    let n = attempts.fetch_add(1, Ordering::Relaxed);
                    if n < 2 {
                        return Err(std::io::Error::from_raw_os_error(32));
                    }
                }
                std::fs::rename(from, to)
            },
        );

        assert!(res.is_ok());
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        assert_eq!(std::fs::read(&target_exe).unwrap(), b"new-v2");
        assert_eq!(std::fs::read(&backup_exe).unwrap(), b"old-v1");
        assert!(!error_log.exists());
    }

    #[test]
    fn swap_binary_rolls_back_and_writes_error_log_when_final_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target_exe = dir.path().join("gateterm");
        let source_bin = dir.path().join("staged-bin");
        let backup_exe = dir.path().join("gateterm.bak");
        let error_log = dir.path().join("nested").join("last-apply-error.txt");

        std::fs::write(&target_exe, b"old-v1").unwrap();
        std::fs::write(&source_bin, b"new-v2").unwrap();

        let req = ApplyRequest {
            parent_pid: 0,
            target_exe: target_exe.clone(),
            source_bin,
            backup_exe: backup_exe.clone(),
            error_log: error_log.clone(),
            relaunch: false,
        };

        let outcome = swap_with_rollback_and_ops(
            &req,
            RetryPolicy {
                max_attempts: 2,
                delay: Duration::from_millis(1),
            },
            |from, to| {
                if to == target_exe && from != backup_exe {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "simulated lock on target",
                    ))
                } else {
                    std::fs::rename(from, to)
                }
            },
        );
        let failure = outcome.unwrap_err();
        assert!(failure.can_relaunch);
        assert_eq!(std::fs::read(&target_exe).unwrap(), b"old-v1");
        assert!(!backup_exe.exists());
        let err_text = std::fs::read_to_string(&error_log).unwrap();
        assert!(err_text.contains("simulated lock on target"));
        assert!(err_text.contains("rolled back"));

        let leftover_temp = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().ends_with(".update-new"));
        assert!(
            !leftover_temp,
            "leftover .update-new file must be cleaned up on rollback"
        );

        // Failure on first rename (target -> backup) leaves original target intact and can_relaunch = true.
        let first_rename_err = swap_with_rollback_and_ops(
            &req,
            RetryPolicy {
                max_attempts: 1,
                delay: Duration::from_millis(1),
            },
            |_from, _to| Err(std::io::Error::from_raw_os_error(5)),
        )
        .unwrap_err();
        assert!(first_rename_err.can_relaunch);
        assert_eq!(std::fs::read(&target_exe).unwrap(), b"old-v1");

        // Failure on both final rename and rollback sets can_relaunch = false and logs both errors.
        let double_err = swap_with_rollback_and_ops(
            &req,
            RetryPolicy {
                max_attempts: 1,
                delay: Duration::from_millis(1),
            },
            |from, to| {
                if to == target_exe {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "target dir locked",
                    ))
                } else {
                    std::fs::rename(from, to)
                }
            },
        )
        .unwrap_err();
        assert!(!double_err.can_relaunch);
        let double_err_text = std::fs::read_to_string(&error_log).unwrap();
        assert!(double_err_text.contains("rollback from"));
        assert!(double_err_text.contains("also failed"));
    }

    #[test]
    fn cleanup_after_start_removes_bak_and_staging_dirs_and_consumes_error_log() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("app");
        let cache_root = dir.path().join("cache");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::create_dir_all(cache_root.join("staging-desktop-web-1.0.4-abc")).unwrap();
        std::fs::write(
            cache_root
                .join("staging-desktop-web-1.0.4-abc")
                .join("staged-bin"),
            b"bin",
        )
        .unwrap();
        std::fs::create_dir_all(cache_root.join("keep-unrelated")).unwrap();

        let current_exe = app_dir.join("gateterm-desktop-web.exe");
        let bak_exe = app_dir.join("gateterm-desktop-web.exe.bak");
        std::fs::write(&current_exe, b"v2").unwrap();
        std::fs::write(&bak_exe, b"v1").unwrap();
        let err_log = cache_root.join(ERROR_LOG_FILENAME);
        std::fs::write(&err_log, "boom\n").unwrap();

        assert_eq!(
            cleanup_after_start(&current_exe, &cache_root).as_deref(),
            Some("boom")
        );
        assert!(!bak_exe.exists());
        assert!(!cache_root.join("staging-desktop-web-1.0.4-abc").exists());
        assert!(cache_root.join("keep-unrelated").exists());
        assert!(!err_log.exists());
        assert_eq!(cleanup_after_start(&current_exe, &cache_root), None);
    }

    #[test]
    fn proc_stat_parser_and_gui_env_preservation() {
        assert!(!is_proc_stat_exited("1234 (gateterm) S 1 2 3"));
        assert!(!is_proc_stat_exited("1234 (gate (term) v2) R 1 2 3"));
        assert!(is_proc_stat_exited("1234 (gate (term) v2) Z 1 2 3"));
        assert!(is_proc_stat_exited("1234 (gateterm) X 1 2 3"));
        assert!(!is_proc_stat_exited("malformed"));

        let mut env_map = HashMap::from([
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        let mut cmd = std::process::Command::new("dummy");
        preserve_gui_env(&mut cmd, |k| env_map.remove(k).map(OsString::from));
        let envs: HashMap<_, _> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        assert_eq!(envs.get("DISPLAY").map(String::as_str), Some(":0"));
        assert_eq!(
            envs.get("WAYLAND_DISPLAY").map(String::as_str),
            Some("wayland-0")
        );
        assert_eq!(
            envs.get("DBUS_SESSION_BUS_ADDRESS").map(String::as_str),
            Some("unix:path=/run/user/1000/bus")
        );
        assert_eq!(
            envs.get("XDG_RUNTIME_DIR").map(String::as_str),
            Some("/run/user/1000")
        );
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn helper_request_uses_canonical_target_and_detached_spawn_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let target_exe = dir.path().join("gateterm-desktop-web.exe");
        let cache_root = dir.path().join("cache");
        let staged_dir = cache_root.join("staging-desktop-web-1.0.4-abc");
        std::fs::create_dir_all(&staged_dir).unwrap();
        std::fs::write(&target_exe, b"old").unwrap();

        let staged = StagedUpdate {
            staging_dir: staged_dir.clone(),
            binary_path: staged_dir.join("staged-bin.exe"),
        };
        let req = build_helper_request(&staged, &target_exe, &cache_root).unwrap();
        assert_eq!(req.parent_pid, std::process::id());
        assert_eq!(req.source_bin, staged.binary_path);
        assert_eq!(req.backup_exe, backup_path_for(&req.target_exe));
        assert_eq!(req.error_log, cache_root.join(ERROR_LOG_FILENAME));
        assert!(req.relaunch);

        // Missing staged binary or missing target fails cleanly before spawning.
        assert!(spawn_apply_helper(&staged, &target_exe, &cache_root).is_err());
        assert!(
            spawn_apply_helper(&staged, &dir.path().join("missing-target"), &cache_root).is_err()
        );

        // Verify wait_for_parent_exit returns promptly both while Child handle keeps an exited
        // process object alive (WaitForSingleObject) and after handle closure (null handle /
        // missing /proc entry), and that detached process spawning works cleanly.
        let current_test_bin = std::env::current_exe().unwrap();
        let mut child =
            spawn_detached_command(&current_test_bin, &[OsString::from("--list")], None).unwrap();
        let exited_pid = child.id();
        let status = child.wait().unwrap();
        assert!(status.success());
        wait_for_parent_exit(exited_pid).unwrap();
        drop(child);
        wait_for_parent_exit(exited_pid).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_spawn_apply_helper_returns_unsupported_platform() {
        let dir = tempfile::tempdir().unwrap();
        let staged = StagedUpdate {
            staging_dir: dir.path().to_path_buf(),
            binary_path: dir.path().join("staged-bin"),
        };
        let err = spawn_apply_helper(&staged, &dir.path().join("app"), dir.path()).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ApplyError>(),
            Some(ApplyError::UnsupportedPlatform)
        ));
    }
}
