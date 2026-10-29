//! File-backed logging and panic hook for `clients/gateterm-desktop-native`,
//! aligned with `clients/gateterm-desktop-web/src/logs.rs`.

use std::any::Any;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const MAX_LOG_BYTES: u64 = 1 << 20;

pub fn log_path() -> Result<PathBuf> {
    let dirs =
        directories::ProjectDirs::from("", "", "gateterm").context("Cannot locate config dir")?;
    Ok(dirs.config_dir().join("gateterm.log"))
}

pub fn truncate_if_larger_than(path: &Path, limit: u64) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > limit => std::fs::write(path, b""),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn init() -> Result<PathBuf> {
    let path = log_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create log directory {}", dir.display()))?;
    }
    truncate_if_larger_than(&path, MAX_LOG_BYTES)
        .with_context(|| format!("Failed to truncate log {}", path.display()))?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open log {}", path.display()))?;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .try_init();
    install_panic_hook(&path);
    Ok(path)
}

fn payload_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "(unrecognized panic payload)"
    }
}

fn format_panic(message: &str, location: Option<String>) -> String {
    let at = location.unwrap_or_else(|| "unknown location".to_string());
    let message = message.replace('\n', " / ");
    format!("PANIC at {at}: {message}")
}

pub fn install_panic_hook(path: &Path) {
    let previous = std::panic::take_hook();
    let path = path.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let line = format_panic(payload_message(info.payload()), location);
        let opened = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path);
        if let Ok(mut file) = opened {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            let _ = file.sync_data();
        }
        previous(info);
    }));
}
