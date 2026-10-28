//! Input capabilities of the system ConPTY used by portable-pty.

/// Older Windows ConPTY silently discards win32-input-mode packets. Keep the
/// regular VT path unless the host is known to have the newer implementation.
pub fn win32_input_mode() -> bool {
    #[cfg(windows)]
    {
        static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *SUPPORTED.get_or_init(|| supports_build(sysinfo::System::kernel_version().as_deref()))
    }
    #[cfg(not(windows))]
    { false }
}

#[cfg(any(windows, test))]
fn supports_build(build: Option<&str>) -> bool {
    // Windows 11 generation and later. Unknown builds fail closed to VT;
    // installing Windows Terminal does not replace portable-pty's system ConPTY.
    build.and_then(|s| s.parse::<u32>().ok()).is_some_and(|b| b >= 22000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_and_unknown_conpty_builds_use_vt() {
        for build in [None, Some(""), Some("unknown"), Some("17763"), Some("19045"), Some("20348")] {
            assert!(!supports_build(build), "{build:?}");
        }
    }

    #[test]
    fn newer_conpty_builds_keep_precise_key_input() {
        for build in ["22000", "22621", "26100"] {
            assert!(supports_build(Some(build)), "{build}");
        }
    }
}
