//! Shared Sparkle-style desktop self-updater core for Gateterm.

pub mod apply;
pub mod check;
pub mod download;
pub mod manifest;
pub mod state;
pub mod verify;

pub use apply::{
    APPLY_UPDATE_FLAG, ApplyError, ApplyRequest, build_apply_args, cleanup_after_start,
    parse_apply_args, run_apply, spawn_apply_helper, try_run_from_env,
};
pub use check::{
    CHECK_INTERVAL, CheckConfig, CheckOutcome, DEFAULT_MANIFEST_URL, DEFAULT_SIGNATURE_URL,
    DOWNLOAD_READ_TIMEOUT, MAX_MANIFEST_BYTES, STARTUP_DELAY, build_download_http_client,
    build_http_client, check_now, start_daily_checks,
};
pub use download::{
    DownloadError, DownloadProgress, MAX_BINARY_BYTES, StagedUpdate, download_and_stage,
    extract_verified_binary, preflight_writable, updates_cache_dir,
};
pub use manifest::{
    ArchiveFormat, AvailableUpdate, MAX_NOTES_BYTES, Manifest, TargetArtifact, truncate_notes,
    validate_and_select, validate_manifest,
};
pub use state::UpdateState;
pub use verify::{
    EMBEDDED_PUBLIC_KEY_B64, VerifyError, compiled_public_key_b64, generate_keypair,
    sign_manifest_bytes, verify_manifest_bytes,
};

/// Desktop application variant sharing this updater crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppKind {
    DesktopWeb,
    DesktopNative,
}

impl AppKind {
    /// Stable lowercase identifier used in manifest target keys and state filenames.
    pub fn id(self) -> &'static str {
        match self {
            Self::DesktopWeb => "desktop-web",
            Self::DesktopNative => "desktop-native",
        }
    }

    /// Manifest target key for this application on the current platform, or `None` when
    /// in-place self-apply is unsupported (e.g. macOS).
    pub fn target_key(self) -> Option<String> {
        current_target_key(self)
    }

    /// Returns `true` when this platform supports downloading and replacing the binary in place.
    pub fn supports_self_apply() -> bool {
        supports_self_apply()
    }
}

/// Returns `true` when the current OS and architecture support in-place self-apply.
pub fn supports_self_apply() -> bool {
    cfg!(any(
        all(windows, target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    ))
}

/// Returns the manifest target key for `app` on the current OS/architecture, if self-apply is supported.
pub fn current_target_key(app: AppKind) -> Option<String> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        Some(format!("{}-x86_64-pc-windows-msvc", app.id()))
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(format!("{}-x86_64-unknown-linux-gnu", app.id()))
    }
    #[cfg(not(any(
        all(windows, target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )))]
    {
        let _ = app;
        None
    }
}
