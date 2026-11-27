//! Atomic persistent updater state (`skipped_version` and `last_checked_at`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use directories::ProjectDirs;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::AppKind;

struct TmpFileGuard<'a> {
    path: &'a Path,
    active: bool,
}

impl Drop for TmpFileGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

/// Persisted updater preference and check timestamp stored independently from `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    pub skipped_version: Option<String>,
    pub last_checked_at: Option<u64>,
}

impl UpdateState {
    /// Returns the default per-app state file path under `ProjectDirs("ai", "eclosion", "gateterm")`.
    pub fn default_path(app: AppKind) -> anyhow::Result<PathBuf> {
        let dirs = ProjectDirs::from("ai", "eclosion", "gateterm")
            .context("cannot determine updater config directory")?;
        Ok(dirs.config_dir().join(format!("updater-{}.json", app.id())))
    }

    /// Loads state from `path`, falling back to `UpdateState::default()` on missing or corrupt JSON.
    pub fn load(path: &Path) -> Self {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "failed to read updater state, using default");
                return Self::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "corrupt updater state JSON, falling back to default");
                Self::default()
            }
        }
    }

    /// Atomically writes state via a sibling `.tmp` file and rename, overwriting any existing file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory {}", parent.display()))?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("updater-state.json");
        let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = parent.join(format!(".{file_name}.{}.{seq}.tmp", std::process::id()));

        let payload =
            serde_json::to_vec_pretty(self).context("failed to serialize updater state")?;
        let mut tmp_guard = TmpFileGuard {
            path: &tmp_path,
            active: false,
        };
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
                .with_context(|| {
                    format!("failed to create temp state file {}", tmp_path.display())
                })?;
            tmp_guard.active = true;
            file.write_all(&payload).with_context(|| {
                format!("failed to write temp state file {}", tmp_path.display())
            })?;
            file.sync_all().with_context(|| {
                format!("failed to sync temp state file {}", tmp_path.display())
            })?;
        }

        replace_file(&tmp_path, path)?;
        tmp_guard.active = false;
        Ok(())
    }

    /// Returns `true` when automatic update prompts should suppress `version` because the user
    /// already skipped that version (or a newer one). Higher versions un-skip automatically.
    pub fn should_skip_auto(&self, version: &str) -> bool {
        let Some(skipped) = self.skipped_version.as_deref() else {
            return false;
        };
        match (Version::parse(version), Version::parse(skipped)) {
            (Ok(candidate), Ok(skipped_ver)) => !candidate.cmp_precedence(&skipped_ver).is_gt(),
            _ => skipped == version,
        }
    }

    /// Clears `skipped_version` once `current_version` catches up to or surpasses it.
    /// Returns `true` if `skipped_version` changed.
    pub fn clear_stale_skip(&mut self, current_version: &str) -> bool {
        let Some(skipped) = self.skipped_version.as_deref() else {
            return false;
        };
        let Ok(current) = Version::parse(current_version) else {
            return false;
        };
        let should_clear = match Version::parse(skipped) {
            Ok(skipped_ver) => current.cmp_precedence(&skipped_ver).is_ge(),
            Err(_) => true,
        };
        if should_clear {
            self.skipped_version = None;
        }
        should_clear
    }
}

fn replace_file(tmp_path: &Path, dest: &Path) -> anyhow::Result<()> {
    match std::fs::rename(tmp_path, dest) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Fallback for Windows targets where replacing an existing file via rename can fail.
            #[cfg(windows)]
            {
                if dest.exists() {
                    std::fs::remove_file(dest).with_context(|| {
                        format!(
                            "failed to remove existing state file {} after rename error ({err})",
                            dest.display()
                        )
                    })?;
                    std::fs::rename(tmp_path, dest).with_context(|| {
                        format!(
                            "failed to rename {} to {}",
                            tmp_path.display(),
                            dest.display()
                        )
                    })?;
                    return Ok(());
                }
            }
            Err(err).with_context(|| {
                format!(
                    "failed to rename {} to {}",
                    tmp_path.display(),
                    dest.display()
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip_corrupt_json_fallback_and_skip_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("nested").join("updater-desktop-web.json");

        // Missing file -> default without panic
        assert_eq!(UpdateState::load(&state_path), UpdateState::default());

        // Corrupt JSON -> default without panic
        std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        std::fs::write(&state_path, "{corrupt json").unwrap();
        assert_eq!(UpdateState::load(&state_path), UpdateState::default());

        // save() writes atomically and reloads identically, overwriting cleanly
        let first = UpdateState {
            skipped_version: Some("1.0.4".to_owned()),
            last_checked_at: Some(1_700_000_000),
        };
        first.save(&state_path).unwrap();
        assert_eq!(UpdateState::load(&state_path), first);

        let mut second = UpdateState {
            skipped_version: Some("1.0.4".to_owned()),
            last_checked_at: Some(1_700_000_123),
        };
        second.save(&state_path).unwrap();
        assert_eq!(UpdateState::load(&state_path), second);

        // No leftover temp files in parent directory
        let entries: Vec<_> = std::fs::read_dir(state_path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["updater-desktop-web.json"]);

        // Skip semantics
        assert!(second.should_skip_auto("1.0.4"));
        assert!(second.should_skip_auto("1.0.3"));
        assert!(!second.should_skip_auto("1.0.5"));

        // clear_stale_skip clears when current >= skipped
        assert!(!second.clear_stale_skip("1.0.3"));
        assert_eq!(second.skipped_version.as_deref(), Some("1.0.4"));

        assert!(second.clear_stale_skip("1.0.4"));
        assert_eq!(second.skipped_version, None);
        assert!(!second.clear_stale_skip("1.0.5"));

        let mut higher = UpdateState {
            skipped_version: Some("1.0.4".to_owned()),
            last_checked_at: None,
        };
        assert!(higher.clear_stale_skip("1.0.5"));
        assert_eq!(higher.skipped_version, None);

        // Default paths differ per app
        let web_path = UpdateState::default_path(AppKind::DesktopWeb).unwrap();
        let native_path = UpdateState::default_path(AppKind::DesktopNative).unwrap();
        assert_eq!(
            web_path.file_name().and_then(|n| n.to_str()),
            Some("updater-desktop-web.json")
        );
        assert_eq!(
            native_path.file_name().and_then(|n| n.to_str()),
            Some("updater-desktop-native.json")
        );
    }

    #[test]
    fn save_cleans_up_temp_file_on_failure_and_guard_unlinks_when_dropped_active() {
        let dir = tempfile::tempdir().unwrap();

        // 1. Failing replace_file (destination is an existing directory) leaves no .tmp behind.
        let dir_dest = dir.path().join("dest-is-a-dir");
        std::fs::create_dir_all(&dir_dest).unwrap();
        let state = UpdateState {
            skipped_version: Some("1.0.4".to_owned()),
            last_checked_at: Some(1_700_000_000),
        };
        assert!(state.save(&dir_dest).is_err());
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftover, vec!["dest-is-a-dir"]);

        // 2. Armed TmpFileGuard unlinks temp file on early error return (e.g. write/sync failure)
        //    after the inner file handle closes.
        let guarded_tmp = dir.path().join(".state.tmp");
        let early_err = (|| -> anyhow::Result<()> {
            let mut guard = TmpFileGuard {
                path: &guarded_tmp,
                active: false,
            };
            {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&guarded_tmp)?;
                guard.active = true;
                file.write_all(b"partial")?;
                anyhow::bail!("simulated write or sync failure");
            }
        })();
        assert!(early_err.is_err());
        assert!(!guarded_tmp.exists());

        // 3. Disarmed TmpFileGuard does not delete pre-existing file if open/create_new failed.
        let preexisting = dir.path().join(".preexisting.tmp");
        std::fs::write(&preexisting, b"keep").unwrap();
        {
            let _inactive_guard = TmpFileGuard {
                path: &preexisting,
                active: false,
            };
        }
        assert!(preexisting.exists());
    }
}
