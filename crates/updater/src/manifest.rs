//! Signed update manifest schema, validation, and platform target selection.

use std::collections::BTreeMap;

use anyhow::Context;
use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::AppKind;

/// Maximum permitted archive download / extracted binary size (256 MiB).
pub const MAX_ARCHIVE_SIZE_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum UTF-8 byte length retained from manifest release notes.
pub const MAX_NOTES_BYTES: usize = 65_536;

const RELEASE_URL_PREFIX: &str = "/CyberTianzun/Gateterm/releases/";
const DOWNLOAD_URL_PREFIX: &str = "/CyberTianzun/Gateterm/releases/download/";

/// Supported archive container formats for self-updating desktop artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
}

/// Downloadable release archive metadata for a specific desktop target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetArtifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub format: ArchiveFormat,
    pub binary_path: String,
}

/// Signed `update-manifest.json` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub version: String,
    pub tag: String,
    pub pub_date: String,
    pub release_url: String,
    pub notes: String,
    pub targets: BTreeMap<String, TargetArtifact>,
}

/// Validated update candidate ready for UI presentation and optional self-apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableUpdate {
    pub version: String,
    pub tag: String,
    pub pub_date: String,
    pub release_url: String,
    pub notes: String,
    pub artifact: Option<TargetArtifact>,
    pub can_self_apply: bool,
}

/// Validates schema version, stable semver, GitHub URLs, and every target entry in `manifest`.
pub fn validate_manifest(manifest: &Manifest) -> anyhow::Result<()> {
    anyhow::ensure!(
        manifest.schema_version == 1,
        "unsupported manifest schema_version: {}",
        manifest.schema_version
    );

    let version =
        Version::parse(&manifest.version).context("manifest version is not valid semver")?;
    anyhow::ensure!(
        version.pre.is_empty(),
        "manifest version must not be a prerelease: {}",
        manifest.version
    );

    validate_github_url(&manifest.release_url, RELEASE_URL_PREFIX)
        .context("invalid manifest release_url")?;

    for (target_key, artifact) in &manifest.targets {
        validate_github_url(&artifact.url, DOWNLOAD_URL_PREFIX)
            .with_context(|| format!("invalid artifact url for target {target_key}"))?;
        anyhow::ensure!(
            (1..=MAX_ARCHIVE_SIZE_BYTES).contains(&artifact.size),
            "invalid artifact size {} for target {target_key}",
            artifact.size
        );
        validate_sha256(&artifact.sha256)
            .with_context(|| format!("invalid sha256 for target {target_key}"))?;
        validate_binary_path(&artifact.binary_path)
            .with_context(|| format!("invalid binary_path for target {target_key}"))?;
    }

    Ok(())
}

/// Validates `manifest`, enforces strict upgrade precedence over `current_version`, caps `notes`
/// to 64 KiB on a UTF-8 boundary, and selects the artifact matching `target_key`.
pub fn validate_and_select(
    manifest: Manifest,
    app: AppKind,
    current_version: &str,
    target_key: Option<&str>,
) -> anyhow::Result<Option<AvailableUpdate>> {
    let current = Version::parse(current_version)
        .with_context(|| format!("invalid current version: {current_version}"))?;
    validate_manifest(&manifest)?;

    let candidate = Version::parse(&manifest.version)?;
    if !candidate.cmp_precedence(&current).is_gt() {
        return Ok(None);
    }

    let artifact = target_key.and_then(|key| manifest.targets.get(key).cloned());
    let can_self_apply = artifact.is_some();
    let notes = truncate_utf8_bytes(&manifest.notes, MAX_NOTES_BYTES);

    tracing::debug!(
        app = app.id(),
        current = %current,
        candidate = %candidate,
        target_key = ?target_key,
        can_self_apply,
        "validated available update"
    );

    Ok(Some(AvailableUpdate {
        version: manifest.version,
        tag: manifest.tag,
        pub_date: manifest.pub_date,
        release_url: manifest.release_url,
        notes,
        artifact,
        can_self_apply,
    }))
}

fn validate_github_url(raw: &str, required_path_prefix: &str) -> anyhow::Result<()> {
    let url = Url::parse(raw).with_context(|| format!("invalid URL: {raw}"))?;
    anyhow::ensure!(url.scheme() == "https", "URL must use https: {raw}");
    anyhow::ensure!(
        url.host_str() == Some("github.com"),
        "URL host must be github.com: {raw}"
    );
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none(),
        "URL must not contain userinfo: {raw}"
    );
    anyhow::ensure!(
        url.port().is_none(),
        "URL must not specify a custom port: {raw}"
    );
    anyhow::ensure!(
        url.path().starts_with(required_path_prefix),
        "URL path must start with {required_path_prefix}: {raw}"
    );
    let raw_after_scheme = raw.split_once("://").map(|(_, rest)| rest).unwrap_or("");
    let raw_path = raw_after_scheme
        .split_once(['/', '\\'])
        .map(|(_, path)| path)
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    anyhow::ensure!(
        !raw_path.split(['/', '\\']).any(|seg| {
            seg == ".."
                || seg == "."
                || seg.eq_ignore_ascii_case("%2e")
                || seg.eq_ignore_ascii_case("%2e%2e")
                || seg.eq_ignore_ascii_case(".%2e")
                || seg.eq_ignore_ascii_case("%2e.")
        }),
        "URL path must not contain dot/traversal segments: {raw}"
    );
    Ok(())
}

fn validate_sha256(sha256: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        sha256.len() == 64
            && sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "sha256 must be 64 lowercase hex characters"
    );
    Ok(())
}

fn validate_binary_path(binary_path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!binary_path.is_empty(), "binary_path must not be empty");
    anyhow::ensure!(
        !binary_path.contains(['\\', '\0', ':']),
        "binary_path contains disallowed characters: {binary_path}"
    );
    anyhow::ensure!(
        !binary_path.starts_with('/') && !binary_path.ends_with('/'),
        "binary_path must be a relative file path without leading or trailing slash: {binary_path}"
    );
    for segment in binary_path.split('/') {
        anyhow::ensure!(
            !segment.is_empty() && segment != "." && segment != "..",
            "binary_path contains invalid segment: {binary_path}"
        );
    }
    Ok(())
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_KEY: &str = "desktop-web-x86_64-pc-windows-msvc";
    const LINUX_KEY: &str = "desktop-web-x86_64-unknown-linux-gnu";
    const VALID_SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn sample_artifact(format: ArchiveFormat, binary_path: &str) -> TargetArtifact {
        TargetArtifact {
            url: "https://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/gateterm.zip".to_owned(),
            sha256: VALID_SHA256.to_owned(),
            size: 14_820_352,
            format,
            binary_path: binary_path.to_owned(),
        }
    }

    fn sample_manifest() -> Manifest {
        let mut targets = BTreeMap::new();
        targets.insert(
            WIN_KEY.to_owned(),
            sample_artifact(ArchiveFormat::Zip, "gateterm-desktop-web.exe"),
        );
        targets.insert(
            LINUX_KEY.to_owned(),
            sample_artifact(ArchiveFormat::TarGz, "gateterm-desktop-web/gateterm-desktop-web"),
        );
        Manifest {
            schema_version: 1,
            version: "1.0.4".to_owned(),
            tag: "release-v1.0.4".to_owned(),
            pub_date: "2026-11-27T08:00:00Z".to_owned(),
            release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4"
                .to_owned(),
            notes: "## What's Changed\n- Fast self-update".to_owned(),
            targets,
        }
    }

    #[test]
    fn validates_schema_semver_urls_size_sha256_binary_path_notes_cap_and_macos_fallback() {
        // Valid newer version with Windows target
        let update = validate_and_select(
            sample_manifest(),
            AppKind::DesktopWeb,
            "1.0.3",
            Some(WIN_KEY),
        )
        .unwrap()
        .expect("should report newer update");
        assert!(update.can_self_apply);
        assert_eq!(update.version, "1.0.4");
        assert_eq!(
            update.artifact.as_ref().unwrap().binary_path,
            "gateterm-desktop-web.exe"
        );

        // Valid newer version with target_key = None (macOS fallback)
        let mac_update =
            validate_and_select(sample_manifest(), AppKind::DesktopWeb, "1.0.3", None)
                .unwrap()
                .expect("macOS should still receive update notice");
        assert!(!mac_update.can_self_apply);
        assert!(mac_update.artifact.is_none());

        // Missing target key in manifest degrades to browser download notice
        let missing_target = validate_and_select(
            sample_manifest(),
            AppKind::DesktopWeb,
            "1.0.3",
            Some("desktop-web-aarch64-unknown-linux-gnu"),
        )
        .unwrap()
        .expect("missing target key should degrade gracefully");
        assert!(!missing_target.can_self_apply);
        assert!(missing_target.artifact.is_none());

        // Same or older version -> Ok(None)
        assert!(
            validate_and_select(
                sample_manifest(),
                AppKind::DesktopWeb,
                "1.0.4",
                Some(WIN_KEY),
            )
            .unwrap()
            .is_none()
        );
        assert!(
            validate_and_select(
                sample_manifest(),
                AppKind::DesktopWeb,
                "1.0.5",
                Some(WIN_KEY),
            )
            .unwrap()
            .is_none()
        );

        // Prerelease and invalid semver / unknown schema_version
        for bad_ver in ["1.0.4-rc.1", "1.0.4-alpha", "1.0", "not-a-version"] {
            let mut m = sample_manifest();
            m.version = bad_ver.to_owned();
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for version {bad_ver}"
            );
        }
        for bad_schema in [0, 2, 99] {
            let mut m = sample_manifest();
            m.schema_version = bad_schema;
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for schema {bad_schema}"
            );
        }

        // Untrusted release_url
        for bad_release_url in [
            "http://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4",
            "https://evil.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4",
            "https://user@github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4",
            "https://:pass@github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4",
            "https://github.com:8443/CyberTianzun/Gateterm/releases/tag/release-v1.0.4",
            "https://github.com/Other/Gateterm/releases/tag/release-v1.0.4",
            "https://github.com/CyberTianzun/Gateterm/releases/../evil",
        ] {
            let mut m = sample_manifest();
            m.release_url = bad_release_url.to_owned();
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for release_url {bad_release_url}"
            );
        }

        // Untrusted artifact url (tested on unselected target too)
        for bad_artifact_url in [
            "http://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/a.zip",
            "https://evil.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/a.zip",
            "https://user@github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/a.zip",
            "https://github.com:8443/CyberTianzun/Gateterm/releases/download/release-v1.0.4/a.zip",
            "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4",
            "https://github.com/CyberTianzun/Gateterm/releases/download/../evil",
            "https://github.com/CyberTianzun/Gateterm/releases/download/v1/%2e%2e/evil",
        ] {
            let mut m = sample_manifest();
            m.targets.get_mut(LINUX_KEY).unwrap().url = bad_artifact_url.to_owned();
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for artifact url {bad_artifact_url}"
            );
        }

        // Invalid sha256
        for bad_sha in [
            "012345",
            &"A".repeat(64),
            &format!("{}g", "0".repeat(63)),
            &"0".repeat(65),
        ] {
            let mut m = sample_manifest();
            m.targets.get_mut(WIN_KEY).unwrap().sha256 = bad_sha.to_owned();
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for sha256 {bad_sha}"
            );
        }

        // Invalid size vs boundary sizes
        for bad_size in [0, 268_435_456 + 1] {
            let mut m = sample_manifest();
            m.targets.get_mut(WIN_KEY).unwrap().size = bad_size;
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for size {bad_size}"
            );
        }
        for ok_size in [1, 268_435_456] {
            let mut m = sample_manifest();
            m.targets.get_mut(WIN_KEY).unwrap().size = ok_size;
            assert!(validate_manifest(&m).is_ok());
        }

        // Traversal / unsafe binary_path
        for bad_path in [
            "",
            "../x",
            "a/../b",
            "./x",
            ".",
            "..",
            "/abs",
            r"a\b",
            "C:foo",
            "C:/foo",
            "dir/",
            "a//b",
            "bad\0name",
        ] {
            let mut m = sample_manifest();
            m.targets.get_mut(WIN_KEY).unwrap().binary_path = bad_path.to_owned();
            assert!(
                validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY)).is_err(),
                "expected error for binary_path {bad_path:?}"
            );
        }

        // Notes > 64 KiB -> truncated cleanly on UTF-8 char boundary to <= 65536 bytes
        let mut m = sample_manifest();
        let mut notes = "a".repeat(65_535);
        notes.push('中'); // 3-byte char spanning bytes 65535..65538
        notes.push_str(&"🚀".repeat(50));
        m.notes = notes;
        let capped =
            validate_and_select(m, AppKind::DesktopWeb, "1.0.3", Some(WIN_KEY))
                .unwrap()
                .unwrap();
        assert!(capped.notes.len() <= 65_536);
        assert_eq!(capped.notes.len(), 65_535);
        assert!(capped.notes.bytes().all(|b| b == b'a'));
    }

    #[test]
    fn manifest_serde_and_platform_target_detection() {
        let manifest = sample_manifest();
        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
        assert!(validate_manifest(&decoded).is_ok());

        assert_eq!(AppKind::DesktopWeb.id(), "desktop-web");
        assert_eq!(AppKind::DesktopNative.id(), "desktop-native");

        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            assert!(AppKind::supports_self_apply());
            assert_eq!(
                AppKind::DesktopWeb.target_key().as_deref(),
                Some("desktop-web-x86_64-pc-windows-msvc")
            );
            assert_eq!(
                AppKind::DesktopNative.target_key().as_deref(),
                Some("desktop-native-x86_64-pc-windows-msvc")
            );
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            assert!(AppKind::supports_self_apply());
            assert_eq!(
                AppKind::DesktopWeb.target_key().as_deref(),
                Some("desktop-web-x86_64-unknown-linux-gnu")
            );
            assert_eq!(
                AppKind::DesktopNative.target_key().as_deref(),
                Some("desktop-native-x86_64-unknown-linux-gnu")
            );
        }
        #[cfg(not(any(
            all(windows, target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64")
        )))]
        {
            assert!(!AppKind::supports_self_apply());
            assert_eq!(AppKind::DesktopWeb.target_key(), None);
        }
    }
}
