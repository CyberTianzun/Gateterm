//! Streaming archive download, SHA-256 verification, safe single-file extraction,
//! and target executable write pre-flight checks.

use std::fmt::Write as _;
use std::future::Future;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use directories::ProjectDirs;
use flate2::read::GzDecoder;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use url::Url;

use crate::{AppKind, ArchiveFormat, TargetArtifact};

/// Maximum permitted downloaded archive and extracted binary size (256 MiB).
pub const MAX_BINARY_BYTES: u64 = crate::manifest::MAX_ARCHIVE_SIZE_BYTES;

const MAX_TAR_STREAM_OVERHEAD: u64 = 32 * 1024 * 1024;
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;

/// Progress reported while streaming an update archive to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

/// Verified, extracted update artifact staged in a temporary cache directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedUpdate {
    pub staging_dir: PathBuf,
    pub binary_path: PathBuf,
}

/// Failure modes for update download, verification, extraction, and write pre-flight.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("update download cancelled")]
    Cancelled,
    #[error("downloaded archive size mismatch: expected {expected} bytes, got {actual} bytes")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("downloaded archive SHA-256 mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("unsafe archive entry path: {0}")]
    UnsafeArchivePath(String),
    #[error("expected binary {0:?} not found in archive")]
    BinaryNotFound(String),
    #[error("target path {} is not writable: {source}", path.display())]
    NotWritable {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

struct StagingCleanupGuard {
    dir: PathBuf,
    keep: bool,
}

impl Drop for StagingCleanupGuard {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

struct UnlinkOnDrop {
    path: PathBuf,
    keep: bool,
}

impl Drop for UnlinkOnDrop {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Returns the default cache directory for staged updates:
/// `ProjectDirs("ai", "eclosion", "gateterm").cache_dir().join("updates")`.
pub fn updates_cache_dir() -> anyhow::Result<PathBuf> {
    let dirs = ProjectDirs::from("ai", "eclosion", "gateterm")
        .context("cannot determine updater cache directory")?;
    Ok(dirs.cache_dir().join("updates"))
}

/// Resolves symlinks on `target_exe` via `std::fs::canonicalize` and verifies its parent
/// directory is writable by creating and deleting a unique probe file.
pub fn preflight_writable(target_exe: &Path) -> Result<PathBuf, DownloadError> {
    static PROBE_SEQ: AtomicU64 = AtomicU64::new(0);

    let canonical =
        std::fs::canonicalize(target_exe).map_err(|source| DownloadError::NotWritable {
            path: target_exe.to_path_buf(),
            source,
        })?;
    let parent = canonical
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| DownloadError::NotWritable {
            path: canonical.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "target executable has no parent directory",
            ),
        })?;

    for _ in 0..8 {
        let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
        let probe_path = parent.join(format!(
            ".gateterm-write-probe-{}-{seq}-{:016x}.tmp",
            std::process::id(),
            random_u64()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe_path)
        {
            Ok(file) => {
                // Close handle before removing on Windows.
                drop(file);
                std::fs::remove_file(&probe_path).map_err(|source| DownloadError::NotWritable {
                    path: probe_path,
                    source,
                })?;
                return Ok(canonical);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(DownloadError::NotWritable {
                    path: parent.to_path_buf(),
                    source,
                });
            }
        }
    }

    Err(DownloadError::NotWritable {
        path: parent.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "failed to allocate unique write probe file",
        ),
    })
}

/// Streams `artifact.url` to a unique staging directory under `cache_root`, verifying exact
/// byte length, SHA-256 digest, cancellation token, and safe extraction of `artifact.binary_path`.
/// Cleans up `staging_dir` on any error or cancellation.
pub async fn download_and_stage(
    client: &reqwest::Client,
    app: AppKind,
    version: &str,
    artifact: &TargetArtifact,
    cache_root: &Path,
    mut cancel: watch::Receiver<bool>,
    on_progress: impl Fn(DownloadProgress) + Send + 'static,
) -> Result<StagedUpdate, DownloadError> {
    if *cancel.borrow() {
        return Err(DownloadError::Cancelled);
    }

    validate_archive_entry_path(&artifact.binary_path, false)?;
    if !(1..=MAX_BINARY_BYTES).contains(&artifact.size) {
        return Err(DownloadError::Other(anyhow::anyhow!(
            "invalid artifact size: {} bytes",
            artifact.size
        )));
    }
    let parsed_url = Url::parse(&artifact.url)
        .with_context(|| format!("invalid artifact URL: {}", artifact.url))?;
    if !matches!(parsed_url.scheme(), "https" | "http") {
        return Err(DownloadError::Other(anyhow::anyhow!(
            "unsupported artifact URL scheme: {}",
            parsed_url.scheme()
        )));
    }

    std::fs::create_dir_all(cache_root).with_context(|| {
        format!(
            "failed to create updates cache directory {}",
            cache_root.display()
        )
    })?;
    let staging_dir = create_unique_staging_dir(cache_root, app, version)?;
    let mut guard = StagingCleanupGuard {
        dir: staging_dir.clone(),
        keep: false,
    };

    let response = select_with_cancel(&mut cancel, client.get(parsed_url).send())
        .await?
        .context("failed to send update archive request")?
        .error_for_status()
        .context("update archive request returned error status")?;

    if let Some(content_len) = response.content_length()
        && content_len != artifact.size
    {
        return Err(DownloadError::SizeMismatch {
            expected: artifact.size,
            actual: content_len,
        });
    }

    let archive_name = match artifact.format {
        ArchiveFormat::Zip => "archive.zip",
        ArchiveFormat::TarGz => "archive.tar.gz",
    };
    let archive_path = staging_dir.join(archive_name);
    let actual_sha256 = stream_archive_to_file(
        response,
        &archive_path,
        artifact.size,
        &mut cancel,
        on_progress,
    )
    .await?;

    if actual_sha256 != artifact.sha256 {
        return Err(DownloadError::HashMismatch {
            expected: artifact.sha256.clone(),
            actual: actual_sha256,
        });
    }

    if *cancel.borrow() {
        return Err(DownloadError::Cancelled);
    }

    let staged_name = if artifact.binary_path.ends_with(".exe") {
        "staged-bin.exe"
    } else {
        "staged-bin"
    };
    let staged_binary_path = staging_dir.join(staged_name);
    let archive_for_extract = archive_path.clone();
    let dest_for_extract = staged_binary_path.clone();
    let expected_binary_path = artifact.binary_path.clone();
    let format = artifact.format;

    tokio::task::spawn_blocking(move || {
        extract_verified_binary(
            &archive_for_extract,
            format,
            &expected_binary_path,
            &dest_for_extract,
        )
    })
    .await
    .context("archive extraction task panicked")??;

    if *cancel.borrow() {
        return Err(DownloadError::Cancelled);
    }

    let _ = std::fs::remove_file(&archive_path);
    guard.keep = true;

    Ok(StagedUpdate {
        staging_dir,
        binary_path: staged_binary_path,
    })
}

/// Safely extracts the single regular file matching `expected_binary_path` from `archive_path`
/// to `dest_path`, enforcing a hard 256 MiB decompressed output cap and rejecting traversal,
/// absolute paths, symlinks, hardlinks, and non-regular entries. Sets `0o755` on Unix.
pub fn extract_verified_binary(
    archive_path: &Path,
    format: ArchiveFormat,
    expected_binary_path: &str,
    dest_path: &Path,
) -> Result<(), DownloadError> {
    extract_verified_binary_with_cap(
        archive_path,
        format,
        expected_binary_path,
        dest_path,
        MAX_BINARY_BYTES,
    )
}

fn extract_verified_binary_with_cap(
    archive_path: &Path,
    format: ArchiveFormat,
    expected_binary_path: &str,
    dest_path: &Path,
    max_bytes: u64,
) -> Result<(), DownloadError> {
    let expected = validate_archive_entry_path(expected_binary_path, false)?;
    match format {
        ArchiveFormat::Zip => extract_zip_binary(archive_path, expected, dest_path, max_bytes),
        ArchiveFormat::TarGz => extract_tar_gz_binary(archive_path, expected, dest_path, max_bytes),
    }
}

fn extract_zip_binary(
    archive_path: &Path,
    expected_binary_path: &str,
    dest_path: &Path,
    max_bytes: u64,
) -> Result<(), DownloadError> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open zip archive {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .with_context(|| format!("failed to parse zip archive {}", archive_path.display()))?;

    let mut matched_index: Option<usize> = None;
    for idx in 0..archive.len() {
        let entry = archive
            .by_index_raw(idx)
            .with_context(|| format!("failed to read zip entry #{idx}"))?;
        if entry.is_symlink() {
            return Err(DownloadError::UnsafeArchivePath(entry.name().to_owned()));
        }
        let is_dir = entry.is_dir();
        if let Some(mode) = entry.unix_mode() {
            let file_type = mode & S_IFMT;
            let expected_type = if is_dir { S_IFDIR } else { S_IFREG };
            if file_type != 0 && file_type != expected_type {
                return Err(DownloadError::UnsafeArchivePath(entry.name().to_owned()));
            }
        }
        let normalized = validate_archive_entry_path(entry.name(), is_dir)?;
        if entry.enclosed_name().is_none() {
            return Err(DownloadError::UnsafeArchivePath(entry.name().to_owned()));
        }
        if normalized == expected_binary_path {
            if is_dir || entry.encrypted() || matched_index.is_some() {
                return Err(DownloadError::UnsafeArchivePath(entry.name().to_owned()));
            }
            matched_index = Some(idx);
        }
    }

    let idx = matched_index
        .ok_or_else(|| DownloadError::BinaryNotFound(expected_binary_path.to_owned()))?;
    let entry = archive
        .by_index(idx)
        .with_context(|| format!("failed to open zip entry {expected_binary_path}"))?;
    if entry.size() > max_bytes {
        return Err(DownloadError::SizeMismatch {
            expected: max_bytes,
            actual: entry.size(),
        });
    }
    copy_capped_to_dest(entry, dest_path, max_bytes)
}

fn extract_tar_gz_binary(
    archive_path: &Path,
    expected_binary_path: &str,
    dest_path: &Path,
    max_bytes: u64,
) -> Result<(), DownloadError> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open tar.gz archive {}", archive_path.display()))?;
    let decoder = GzDecoder::new(std::io::BufReader::new(file));
    let capped_decoder = decoder.take(max_bytes.saturating_add(MAX_TAR_STREAM_OVERHEAD));
    let mut archive = tar::Archive::new(capped_decoder);

    let mut dest_guard = UnlinkOnDrop {
        path: dest_path.to_path_buf(),
        keep: false,
    };
    let mut found = false;

    let entries = archive.entries().with_context(|| {
        format!(
            "failed to read tar.gz entries from {}",
            archive_path.display()
        )
    })?;
    for entry_res in entries {
        let entry = entry_res
            .with_context(|| format!("corrupt tar.gz entry in {}", archive_path.display()))?;
        let entry_type = entry.header().entry_type();

        if entry_type == tar::EntryType::XGlobalHeader {
            continue;
        }

        let path_bytes = entry.path_bytes();
        let raw_path = std::str::from_utf8(&path_bytes).map_err(|_| {
            DownloadError::UnsafeArchivePath(String::from_utf8_lossy(&path_bytes).into_owned())
        })?;

        let is_dir = entry_type == tar::EntryType::Directory;
        let is_regular = entry_type == tar::EntryType::Regular;
        if !is_dir && !is_regular {
            return Err(DownloadError::UnsafeArchivePath(raw_path.to_owned()));
        }

        let normalized = validate_archive_entry_path(raw_path, is_dir)?;
        if normalized == expected_binary_path {
            if is_dir || found {
                return Err(DownloadError::UnsafeArchivePath(raw_path.to_owned()));
            }
            if entry.size() > max_bytes {
                return Err(DownloadError::SizeMismatch {
                    expected: max_bytes,
                    actual: entry.size(),
                });
            }
            copy_capped_to_dest(entry, dest_path, max_bytes)?;
            found = true;
        }
    }

    if !found {
        return Err(DownloadError::BinaryNotFound(
            expected_binary_path.to_owned(),
        ));
    }

    dest_guard.keep = true;
    Ok(())
}

fn copy_capped_to_dest<R: Read>(
    reader: R,
    dest_path: &Path,
    max_bytes: u64,
) -> Result<(), DownloadError> {
    if let Some(parent) = dest_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create destination directory {}",
                parent.display()
            )
        })?;
    }

    let mut unlink_guard = UnlinkOnDrop {
        path: dest_path.to_path_buf(),
        keep: false,
    };

    // Scope file handle so it closes before UnlinkOnDrop or set_permissions runs.
    let copied = {
        let out_file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dest_path)
            .with_context(|| {
                format!(
                    "failed to create destination binary {}",
                    dest_path.display()
                )
            })?;
        let mut writer = std::io::BufWriter::new(out_file);
        let mut limited = reader.take(max_bytes.saturating_add(1));
        let copied = std::io::copy(&mut limited, &mut writer)
            .with_context(|| format!("failed to extract binary to {}", dest_path.display()))?;
        writer
            .flush()
            .with_context(|| format!("failed to flush extracted binary {}", dest_path.display()))?;
        copied
    };

    if copied > max_bytes {
        return Err(DownloadError::SizeMismatch {
            expected: max_bytes,
            actual: copied,
        });
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest_path, std::fs::Permissions::from_mode(0o755)).with_context(
            || {
                format!(
                    "failed to set executable permissions on {}",
                    dest_path.display()
                )
            },
        )?;
    }

    unlink_guard.keep = true;
    Ok(())
}

fn validate_archive_entry_path(
    raw: &str,
    allow_trailing_slash: bool,
) -> Result<&str, DownloadError> {
    if raw.is_empty() || raw.contains(['\\', '\0', ':']) || raw.starts_with('/') {
        return Err(DownloadError::UnsafeArchivePath(raw.to_owned()));
    }
    let normalized = if allow_trailing_slash {
        raw.strip_suffix('/').unwrap_or(raw)
    } else {
        if raw.ends_with('/') {
            return Err(DownloadError::UnsafeArchivePath(raw.to_owned()));
        }
        raw
    };
    if normalized.is_empty() {
        return Err(DownloadError::UnsafeArchivePath(raw.to_owned()));
    }
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(DownloadError::UnsafeArchivePath(raw.to_owned()));
        }
    }
    Ok(normalized)
}

async fn stream_archive_to_file(
    mut response: reqwest::Response,
    archive_path: &Path,
    expected_size: u64,
    cancel: &mut watch::Receiver<bool>,
    on_progress: impl Fn(DownloadProgress) + Send,
) -> Result<String, DownloadError> {
    let mut file = tokio::fs::File::create(archive_path)
        .await
        .with_context(|| {
            format!(
                "failed to create staging archive {}",
                archive_path.display()
            )
        })?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    on_progress(DownloadProgress {
        downloaded: 0,
        total: expected_size,
    });

    loop {
        if *cancel.borrow() {
            return Err(DownloadError::Cancelled);
        }
        let maybe_chunk = select_with_cancel(cancel, response.chunk())
            .await?
            .context("failed while streaming update archive")?;
        let Some(chunk) = maybe_chunk else {
            break;
        };
        if chunk.is_empty() {
            continue;
        }
        let next_downloaded = downloaded.saturating_add(chunk.len() as u64);
        if next_downloaded > expected_size {
            return Err(DownloadError::SizeMismatch {
                expected: expected_size,
                actual: next_downloaded,
            });
        }
        file.write_all(&chunk).await.with_context(|| {
            format!("failed to write staging archive {}", archive_path.display())
        })?;
        hasher.update(&chunk);
        downloaded = next_downloaded;
        on_progress(DownloadProgress {
            downloaded,
            total: expected_size,
        });
    }

    file.flush()
        .await
        .with_context(|| format!("failed to flush staging archive {}", archive_path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("failed to sync staging archive {}", archive_path.display()))?;
    drop(file);

    if downloaded != expected_size {
        return Err(DownloadError::SizeMismatch {
            expected: expected_size,
            actual: downloaded,
        });
    }

    Ok(format_sha256_lower(&hasher.finalize()))
}

async fn select_with_cancel<F, T>(
    cancel: &mut watch::Receiver<bool>,
    fut: F,
) -> Result<T, DownloadError>
where
    F: Future<Output = T>,
{
    tokio::select! {
        biased;
        _ = wait_cancelled(cancel) => Err(DownloadError::Cancelled),
        output = fut => Ok(output),
    }
}

async fn wait_cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            // Sender dropped while value is false; cancellation cannot fire.
            std::future::pending::<()>().await;
        }
    }
}

fn create_unique_staging_dir(
    cache_root: &Path,
    app: AppKind,
    version: &str,
) -> Result<PathBuf, DownloadError> {
    let safe_version = sanitize_segment(version);
    for _ in 0..8 {
        let dir = cache_root.join(format!(
            "staging-{}-{safe_version}-{:016x}",
            app.id(),
            random_u64()
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(DownloadError::Other(anyhow::Error::new(err).context(
                    format!("failed to create staging directory {}", dir.display()),
                )));
            }
        }
    }
    Err(DownloadError::Other(anyhow::anyhow!(
        "failed to allocate unique staging directory in {}",
        cache_root.display()
    )))
}

fn sanitize_segment(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "unknown".to_owned()
    } else {
        sanitized
    }
}

fn random_u64() -> u64 {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    u64::from_le_bytes(bytes)
}

fn format_sha256_lower(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_http_client;
    use axum::{
        Router,
        extract::Path as AxumPath,
        http::{StatusCode, header},
        response::IntoResponse,
        routing::get,
    };
    use flate2::{Compression, write::GzEncoder};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use zip::write::SimpleFileOptions;

    fn sha256_hex(bytes: &[u8]) -> String {
        format_sha256_lower(&Sha256::digest(bytes))
    }

    fn build_zip_bytes(entries: &[(&str, &[u8], Option<u32>, bool)]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            for &(name, data, unix_mode, is_symlink) in entries {
                if is_symlink {
                    let target = std::str::from_utf8(data).unwrap_or("target");
                    writer
                        .add_symlink(name, target, SimpleFileOptions::default())
                        .unwrap();
                } else if name.ends_with('/') {
                    writer
                        .add_directory(name, SimpleFileOptions::default())
                        .unwrap();
                } else {
                    let mut opts = SimpleFileOptions::default();
                    if let Some(mode) = unix_mode {
                        opts = opts.unix_permissions(mode);
                    }
                    writer.start_file(name, opts).unwrap();
                    writer.write_all(data).unwrap();
                }
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn build_tar_gz_bytes(entries: &[(&str, &[u8], tar::EntryType, Option<&str>)]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for &(path, data, entry_type, link_target) in entries {
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(entry_type);
            header.set_mode(if entry_type == tar::EntryType::Directory {
                0o755
            } else {
                0o644
            });
            header.set_size(if entry_type == tar::EntryType::Regular {
                data.len() as u64
            } else {
                0
            });
            if let Some(target) = link_target {
                let target_bytes = target.as_bytes();
                let old = header.as_old_mut();
                old.linkname[..target_bytes.len()].copy_from_slice(target_bytes);
            }
            // Write raw name directly so traversal test cases like "../evil" bypass Builder::set_path checks.
            let path_bytes = path.as_bytes();
            assert!(
                path_bytes.len() <= 100,
                "test helper supports paths <= 100 bytes"
            );
            header.as_old_mut().name[..path_bytes.len()].copy_from_slice(path_bytes);
            header.set_cksum();

            if entry_type == tar::EntryType::Regular {
                builder.append(&header, data).unwrap();
            } else {
                builder.append(&header, std::io::empty()).unwrap();
            }
        }
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    async fn read_http_request_path(socket: &mut tokio::net::TcpStream) -> String {
        let mut buf = Vec::with_capacity(1024);
        let mut tmp = [0u8; 512];
        loop {
            let n = socket.read(&mut tmp).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let req = String::from_utf8_lossy(&buf);
        req.lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/")
            .to_owned()
    }

    #[tokio::test]
    async fn downloads_verifies_sha256_reports_progress_and_extracts_zip_and_tar_gz() {
        let zip_payload = b"fake-windows-pe-binary-v1.0.4-payload".repeat(256);
        let zip_bytes = build_zip_bytes(&[
            ("README.txt", b"hello", None, false),
            ("gateterm-desktop-web.exe", &zip_payload, None, false),
        ]);
        let zip_sha = sha256_hex(&zip_bytes);

        let tar_payload = b"\x7fELF-fake-linux-binary-v1.0.4".repeat(256);
        let tar_bytes = build_tar_gz_bytes(&[
            (
                "gateterm-desktop-web/",
                &[],
                tar::EntryType::Directory,
                None,
            ),
            (
                "gateterm-desktop-web/gateterm-desktop-web",
                &tar_payload,
                tar::EntryType::Regular,
                None,
            ),
            (
                "gateterm-desktop-web/install.sh",
                b"#!/bin/sh\n",
                tar::EntryType::Regular,
                None,
            ),
        ]);
        let tar_sha = sha256_hex(&tar_bytes);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let zip_srv = zip_bytes.clone();
        let tar_srv = tar_bytes.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let z = zip_srv.clone();
                let t = tar_srv.clone();
                tokio::spawn(async move {
                    let path = read_http_request_path(&mut socket).await;
                    let body = if path == "/win.zip" { &z } else { &t };
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(hdr.as_bytes()).await.unwrap();
                    for chunk in body.chunks(512) {
                        socket.write_all(chunk).await.unwrap();
                        socket.flush().await.unwrap();
                        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });

        let cache_dir = tempfile::tempdir().unwrap();
        let client = build_http_client("1.0.3", true).unwrap();

        // 1. Zip test
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let progress_events = Arc::new(Mutex::new(Vec::<DownloadProgress>::new()));
        let progress_clone = progress_events.clone();
        let zip_artifact = TargetArtifact {
            url: format!("{base_url}/win.zip"),
            sha256: zip_sha,
            size: zip_bytes.len() as u64,
            format: ArchiveFormat::Zip,
            binary_path: "gateterm-desktop-web.exe".to_owned(),
        };
        let staged_zip = download_and_stage(
            &client,
            AppKind::DesktopWeb,
            "1.0.4",
            &zip_artifact,
            cache_dir.path(),
            rx,
            move |p| {
                progress_clone.lock().unwrap().push(p);
            },
        )
        .await
        .unwrap();

        assert!(staged_zip.staging_dir.starts_with(cache_dir.path()));
        assert_eq!(std::fs::read(&staged_zip.binary_path).unwrap(), zip_payload);
        {
            let events = progress_events.lock().unwrap();
            assert!(!events.is_empty());
            assert_eq!(
                events.last().copied(),
                Some(DownloadProgress {
                    downloaded: zip_bytes.len() as u64,
                    total: zip_bytes.len() as u64,
                })
            );
            for w in events.windows(2) {
                assert!(w[0].downloaded <= w[1].downloaded);
                assert_eq!(w[0].total, zip_bytes.len() as u64);
            }
        }

        // 2. TarGz test (also verify Send + !Sync closure like std::sync::mpsc::Sender works in tokio::spawn)
        let (_tx2, rx2) = tokio::sync::watch::channel(false);
        let (tar_tx, tar_rx) = std::sync::mpsc::channel::<DownloadProgress>();
        let tar_artifact = TargetArtifact {
            url: format!("{base_url}/linux.tar.gz"),
            sha256: tar_sha,
            size: tar_bytes.len() as u64,
            format: ArchiveFormat::TarGz,
            binary_path: "gateterm-desktop-web/gateterm-desktop-web".to_owned(),
        };
        let tar_client = client.clone();
        let tar_cache = cache_dir.path().to_path_buf();
        let staged_tar = tokio::spawn(async move {
            download_and_stage(
                &tar_client,
                AppKind::DesktopWeb,
                "1.0.4",
                &tar_artifact,
                &tar_cache,
                rx2,
                move |p| {
                    let _ = tar_tx.send(p);
                },
            )
            .await
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(std::fs::read(&staged_tar.binary_path).unwrap(), tar_payload);
        let tar_events: Vec<_> = tar_rx.try_iter().collect();
        assert_eq!(
            tar_events.last().copied(),
            Some(DownloadProgress {
                downloaded: tar_bytes.len() as u64,
                total: tar_bytes.len() as u64,
            })
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&staged_tar.binary_path)
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "expected executable bits on Unix");
        }

        server.abort();
    }

    #[tokio::test]
    async fn rejects_sha256_mismatch_oversized_body_and_cancels_cleanly_removing_staging_dir() {
        let archive_bytes = build_zip_bytes(&[("app.exe", b"binary-payload-123456", None, false)]);
        let real_sha = sha256_hex(&archive_bytes);
        let archive_len = archive_bytes.len() as u64;

        let (first_chunk_sent_tx, first_chunk_sent_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let first_tx_arc = Arc::new(Mutex::new(Some(first_chunk_sent_tx)));
        let release_rx_arc = Arc::new(tokio::sync::Mutex::new(Some(release_rx)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let ok_srv = archive_bytes.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let ok_body = ok_srv.clone();
                let first_tx = first_tx_arc.clone();
                let rel_rx = release_rx_arc.clone();
                tokio::spawn(async move {
                    let path = read_http_request_path(&mut socket).await;
                    match path.as_str() {
                        "/ok.zip" => {
                            let hdr = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                ok_body.len()
                            );
                            let _ = socket.write_all(hdr.as_bytes()).await;
                            let _ = socket.write_all(&ok_body).await;
                        }
                        "/oversized-cl.zip" => {
                            let big = vec![b'X'; ok_body.len() + 64];
                            let hdr = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                big.len()
                            );
                            let _ = socket.write_all(hdr.as_bytes()).await;
                            let _ = socket.write_all(&big).await;
                        }
                        "/oversized-chunked.zip" => {
                            let hdr = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                            let _ = socket.write_all(hdr.as_bytes()).await;
                            let chunk = vec![b'Y'; ok_body.len() + 32];
                            let chunk_hdr = format!("{:x}\r\n", chunk.len());
                            let _ = socket.write_all(chunk_hdr.as_bytes()).await;
                            let _ = socket.write_all(&chunk).await;
                            let _ = socket.write_all(b"\r\n0\r\n\r\n").await;
                        }
                        "/truncated-cl.zip" => {
                            let short = &ok_body[..ok_body.len() / 2];
                            let hdr = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                short.len()
                            );
                            let _ = socket.write_all(hdr.as_bytes()).await;
                            let _ = socket.write_all(short).await;
                        }
                        "/truncated-chunked.zip" => {
                            let short = &ok_body[..ok_body.len() / 2];
                            let hdr = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                            let _ = socket.write_all(hdr.as_bytes()).await;
                            let chunk_hdr = format!("{:x}\r\n", short.len());
                            let _ = socket.write_all(chunk_hdr.as_bytes()).await;
                            let _ = socket.write_all(short).await;
                            let _ = socket.write_all(b"\r\n0\r\n\r\n").await;
                        }
                        "/slow.zip" => {
                            let hdr = "HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n";
                            let _ = socket.write_all(hdr.as_bytes()).await;
                            let _ = socket.write_all(&[b'Z'; 64]).await;
                            let _ = socket.flush().await;
                            if let Some(tx) = first_tx.lock().unwrap().take() {
                                let _ = tx.send(());
                            }
                            if let Some(rx) = rel_rx.lock().await.take() {
                                let _ = rx.await;
                            }
                            let _ = socket.write_all(&[b'Z'; 960]).await;
                        }
                        _ => {
                            let _ = socket
                                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                                .await;
                        }
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });

        let client = build_http_client("1.0.3", true).unwrap();

        // 1. Wrong sha256 -> HashMismatch and staging_dir removed
        {
            let cache_dir = tempfile::tempdir().unwrap();
            let (_tx, rx) = tokio::sync::watch::channel(false);
            let wrong_sha = if "0".repeat(64) == real_sha {
                "1".repeat(64)
            } else {
                "0".repeat(64)
            };
            let artifact = TargetArtifact {
                url: format!("{base_url}/ok.zip"),
                sha256: wrong_sha,
                size: archive_len,
                format: ArchiveFormat::Zip,
                binary_path: "app.exe".to_owned(),
            };
            let err = download_and_stage(
                &client,
                AppKind::DesktopWeb,
                "1.0.4",
                &artifact,
                cache_dir.path(),
                rx,
                |_| {},
            )
            .await
            .unwrap_err();
            assert!(
                matches!(err, DownloadError::HashMismatch { .. }),
                "expected HashMismatch, got {err:?}"
            );
            assert_eq!(std::fs::read_dir(cache_dir.path()).unwrap().count(), 0);
        }

        // 2. Oversized & truncated responses (with and without Content-Length) -> SizeMismatch
        for path in [
            "/oversized-cl.zip",
            "/oversized-chunked.zip",
            "/truncated-cl.zip",
            "/truncated-chunked.zip",
        ] {
            let cache_dir = tempfile::tempdir().unwrap();
            let (_tx, rx) = tokio::sync::watch::channel(false);
            let artifact = TargetArtifact {
                url: format!("{base_url}{path}"),
                sha256: real_sha.clone(),
                size: archive_len,
                format: ArchiveFormat::Zip,
                binary_path: "app.exe".to_owned(),
            };
            let err = download_and_stage(
                &client,
                AppKind::DesktopWeb,
                "1.0.4",
                &artifact,
                cache_dir.path(),
                rx,
                |_| {},
            )
            .await
            .unwrap_err();
            assert!(
                matches!(err, DownloadError::SizeMismatch { .. }),
                "expected SizeMismatch for {path}, got {err:?}"
            );
            assert_eq!(std::fs::read_dir(cache_dir.path()).unwrap().count(), 0);
        }

        // 3. Cancel watch channel triggered mid-stream -> Cancelled and staging_dir removed
        {
            let cache_dir = tempfile::tempdir().unwrap();
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
            let artifact = TargetArtifact {
                url: format!("{base_url}/slow.zip"),
                sha256: real_sha.clone(),
                size: 1024,
                format: ArchiveFormat::Zip,
                binary_path: "app.exe".to_owned(),
            };
            let client_clone = client.clone();
            let cache_path = cache_dir.path().to_path_buf();
            let task = tokio::spawn(async move {
                download_and_stage(
                    &client_clone,
                    AppKind::DesktopWeb,
                    "1.0.4",
                    &artifact,
                    &cache_path,
                    cancel_rx,
                    |_| {},
                )
                .await
            });
            first_chunk_sent_rx.await.unwrap();
            cancel_tx.send(true).unwrap();
            let err = task.await.unwrap().unwrap_err();
            let _ = release_tx.send(());
            assert!(
                matches!(err, DownloadError::Cancelled),
                "expected Cancelled, got {err:?}"
            );
            assert_eq!(std::fs::read_dir(cache_dir.path()).unwrap().count(), 0);
        }

        // 4. Pre-cancelled watch channel -> Cancelled immediately without creating staging dir
        {
            let cache_dir = tempfile::tempdir().unwrap();
            let (_tx, rx) = tokio::sync::watch::channel(true);
            let artifact = TargetArtifact {
                url: format!("{base_url}/ok.zip"),
                sha256: real_sha,
                size: archive_len,
                format: ArchiveFormat::Zip,
                binary_path: "app.exe".to_owned(),
            };
            let err = download_and_stage(
                &client,
                AppKind::DesktopWeb,
                "1.0.4",
                &artifact,
                cache_dir.path(),
                rx,
                |_| {},
            )
            .await
            .unwrap_err();
            assert!(matches!(err, DownloadError::Cancelled));
            assert_eq!(std::fs::read_dir(cache_dir.path()).unwrap().count(), 0);
        }

        server.abort();
    }

    #[tokio::test]
    async fn rejects_https_to_http_redirect_and_redirect_loops_during_download() {
        let final_hit = Arc::new(AtomicBool::new(false));
        let final_hit_clone = final_hit.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let base_http = base.clone();
        let base_hop = base.clone();

        let app = Router::new()
            .route(
                "/to-http",
                get(move || {
                    let target = format!("{base_http}/final.zip");
                    async move { (StatusCode::FOUND, [(header::LOCATION, target)]).into_response() }
                }),
            )
            .route(
                "/final.zip",
                get(move || {
                    let flag = final_hit_clone.clone();
                    async move {
                        flag.store(true, Ordering::SeqCst);
                        (StatusCode::OK, b"zip".to_vec())
                    }
                }),
            )
            .route(
                "/hop/{step}",
                get(move |AxumPath(step): AxumPath<usize>| {
                    let base = base_hop.clone();
                    async move {
                        let next = format!("{base}/hop/{}", step + 1);
                        (StatusCode::FOUND, [(header::LOCATION, next)]).into_response()
                    }
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cache_dir = tempfile::tempdir().unwrap();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let strict_client = build_http_client("1.0.3", false).unwrap();
        let artifact = TargetArtifact {
            url: format!("{base}/to-http"),
            sha256: "0".repeat(64),
            size: 3,
            format: ArchiveFormat::Zip,
            binary_path: "app.exe".to_owned(),
        };
        assert!(
            download_and_stage(
                &strict_client,
                AppKind::DesktopWeb,
                "1.0.4",
                &artifact,
                cache_dir.path(),
                rx,
                |_| {},
            )
            .await
            .is_err()
        );
        assert!(!final_hit.load(Ordering::SeqCst));
        assert_eq!(std::fs::read_dir(cache_dir.path()).unwrap().count(), 0);

        let (_tx2, rx2) = tokio::sync::watch::channel(false);
        let test_client = build_http_client("1.0.3", true).unwrap();
        let loop_artifact = TargetArtifact {
            url: format!("{base}/hop/0"),
            sha256: "0".repeat(64),
            size: 3,
            format: ArchiveFormat::Zip,
            binary_path: "app.exe".to_owned(),
        };
        assert!(
            download_and_stage(
                &test_client,
                AppKind::DesktopWeb,
                "1.0.4",
                &loop_artifact,
                cache_dir.path(),
                rx2,
                |_| {},
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read_dir(cache_dir.path()).unwrap().count(), 0);

        server.abort();
    }

    #[test]
    fn rejects_zipslip_tar_traversal_symlink_entries_and_missing_binary() {
        let root = tempfile::tempdir().unwrap();
        let dest_dir = root.path().join("dest");
        std::fs::create_dir_all(&dest_dir).unwrap();
        let escaped = root.path().join("evil");

        // Zip traversal variants
        for bad_name in [
            "../evil",
            "a/../../evil",
            "a/../b",
            "/abs/evil",
            r"a\evil",
            "C:evil",
        ] {
            let zip_path = root.path().join("bad-path.zip");
            std::fs::write(
                &zip_path,
                build_zip_bytes(&[
                    ("app.exe", b"ok", None, false),
                    (bad_name, b"pwn", None, false),
                ]),
            )
            .unwrap();
            let out_path = dest_dir.join("out.exe");
            let err = extract_verified_binary(&zip_path, ArchiveFormat::Zip, "app.exe", &out_path)
                .unwrap_err();
            assert!(
                matches!(err, DownloadError::UnsafeArchivePath(_)),
                "expected UnsafeArchivePath for {bad_name:?}, got {err:?}"
            );
            assert!(!escaped.exists());
            assert!(!out_path.exists());
        }

        // Zip symlink at binary_path and non-regular FIFO entry
        let zip_sym = root.path().join("zipsym.zip");
        std::fs::write(
            &zip_sym,
            build_zip_bytes(&[("app.exe", b"/etc/passwd", None, true)]),
        )
        .unwrap();
        let err = extract_verified_binary(
            &zip_sym,
            ArchiveFormat::Zip,
            "app.exe",
            &dest_dir.join("out.exe"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafeArchivePath(_)));

        let zip_fifo = root.path().join("zipfifo.zip");
        let mut fifo_bytes = build_zip_bytes(&[("app.exe", b"", None, false)]);
        // Patch the central directory header to System::Unix (3) + S_IFIFO (0o010_644 << 16).
        let cd_pos = fifo_bytes
            .windows(4)
            .position(|w| w == b"PK\x01\x02")
            .unwrap();
        fifo_bytes[cd_pos + 5] = 3;
        fifo_bytes[cd_pos + 38..cd_pos + 42].copy_from_slice(&(0o010_644u32 << 16).to_le_bytes());
        std::fs::write(&zip_fifo, fifo_bytes).unwrap();
        let err = extract_verified_binary(
            &zip_fifo,
            ArchiveFormat::Zip,
            "app.exe",
            &dest_dir.join("out.exe"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafeArchivePath(_)));

        // Zip directory at binary_path
        let zip_dir = root.path().join("zipdir.zip");
        std::fs::write(&zip_dir, build_zip_bytes(&[("app.exe/", &[], None, false)])).unwrap();
        let err = extract_verified_binary(
            &zip_dir,
            ArchiveFormat::Zip,
            "app.exe",
            &dest_dir.join("out.exe"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafeArchivePath(_)));

        // Zip missing binary_path
        let zip_missing = root.path().join("zipmissing.zip");
        std::fs::write(
            &zip_missing,
            build_zip_bytes(&[("other.exe", b"hello", None, false)]),
        )
        .unwrap();
        let err = extract_verified_binary(
            &zip_missing,
            ArchiveFormat::Zip,
            "app.exe",
            &dest_dir.join("out.exe"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::BinaryNotFound(_)));

        // Tar traversal "../evil" (even after valid target entry, dest must be cleaned up)
        for bad_tar_path in ["../evil", "dir/../evil", "/abs/evil", r"dir\evil", "C:evil"] {
            let tar_slip = root.path().join("tarslip.tar.gz");
            std::fs::write(
                &tar_slip,
                build_tar_gz_bytes(&[
                    ("dir/app", b"ok", tar::EntryType::Regular, None),
                    (bad_tar_path, b"pwn", tar::EntryType::Regular, None),
                ]),
            )
            .unwrap();
            let out_path = dest_dir.join("out");
            let err =
                extract_verified_binary(&tar_slip, ArchiveFormat::TarGz, "dir/app", &out_path)
                    .unwrap_err();
            assert!(
                matches!(err, DownloadError::UnsafeArchivePath(_)),
                "expected UnsafeArchivePath for tar {bad_tar_path:?}, got {err:?}"
            );
            assert!(!escaped.exists());
            assert!(!out_path.exists());
        }

        // Tar symlink, hardlink, FIFO, and directory at binary_path
        let tar_sym = root.path().join("tarsym.tar.gz");
        std::fs::write(
            &tar_sym,
            build_tar_gz_bytes(&[("dir/app", &[], tar::EntryType::Symlink, Some("/etc/passwd"))]),
        )
        .unwrap();
        let err = extract_verified_binary(
            &tar_sym,
            ArchiveFormat::TarGz,
            "dir/app",
            &dest_dir.join("out"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafeArchivePath(_)));

        let tar_hardlink = root.path().join("tarhardlink.tar.gz");
        std::fs::write(
            &tar_hardlink,
            build_tar_gz_bytes(&[("dir/app", &[], tar::EntryType::Link, Some("/etc/passwd"))]),
        )
        .unwrap();
        let err = extract_verified_binary(
            &tar_hardlink,
            ArchiveFormat::TarGz,
            "dir/app",
            &dest_dir.join("out"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafeArchivePath(_)));

        let tar_fifo = root.path().join("tarfifo.tar.gz");
        std::fs::write(
            &tar_fifo,
            build_tar_gz_bytes(&[("dir/app", &[], tar::EntryType::Fifo, None)]),
        )
        .unwrap();
        let err = extract_verified_binary(
            &tar_fifo,
            ArchiveFormat::TarGz,
            "dir/app",
            &dest_dir.join("out"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafeArchivePath(_)));

        // Tar missing binary_path
        let tar_missing = root.path().join("tarmissing.tar.gz");
        std::fs::write(
            &tar_missing,
            build_tar_gz_bytes(&[("dir/other", b"ok", tar::EntryType::Regular, None)]),
        )
        .unwrap();
        let err = extract_verified_binary(
            &tar_missing,
            ArchiveFormat::TarGz,
            "dir/app",
            &dest_dir.join("out"),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::BinaryNotFound(_)));
    }

    #[test]
    fn rejects_zip_and_gzip_decompression_bombs_over_256_mib() {
        let root = tempfile::tempdir().unwrap();
        let bomb_size = MAX_BINARY_BYTES + 1;
        let zero_chunk = vec![0u8; 1024 * 1024];

        // 1. Zip bomb (256 MiB + 1 byte)
        let zip_bomb = root.path().join("bomb.zip");
        {
            let file = std::fs::File::create(&zip_bomb).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file(
                    "app.exe",
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated)
                        .compression_level(Some(1)),
                )
                .unwrap();
            let mut remaining = bomb_size;
            while remaining > 0 {
                let n = (remaining as usize).min(zero_chunk.len());
                writer.write_all(&zero_chunk[..n]).unwrap();
                remaining -= n as u64;
            }
            writer.finish().unwrap();
        }
        let zip_dest = root.path().join("out.exe");
        let err = extract_verified_binary(&zip_bomb, ArchiveFormat::Zip, "app.exe", &zip_dest)
            .unwrap_err();
        assert!(
            matches!(err, DownloadError::SizeMismatch { .. }),
            "got {err:?}"
        );
        assert!(!zip_dest.exists());

        // 2. Tar.gz bomb (256 MiB + 1 byte)
        let tar_bomb = root.path().join("bomb.tar.gz");
        {
            let file = std::fs::File::create(&tar_bomb).unwrap();
            let encoder = GzEncoder::new(file, Compression::fast());
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(bomb_size);
            let reader = std::io::repeat(0u8).take(bomb_size);
            builder.append_data(&mut header, "app", reader).unwrap();
            let encoder = builder.into_inner().unwrap();
            encoder.finish().unwrap();
        }
        let tar_dest = root.path().join("out");
        let err =
            extract_verified_binary(&tar_bomb, ArchiveFormat::TarGz, "app", &tar_dest).unwrap_err();
        assert!(
            matches!(err, DownloadError::SizeMismatch { .. }),
            "got {err:?}"
        );
        assert!(!tar_dest.exists());

        // 3. Also verify Read::take catches an entry exceeding cap when header size is bypassed/small
        let small_zip = root.path().join("cap-check.zip");
        std::fs::write(
            &small_zip,
            build_zip_bytes(&[("app.exe", &[b'A'; 4096], None, false)]),
        )
        .unwrap();
        let cap_dest = root.path().join("cap-out.exe");
        let err = extract_verified_binary_with_cap(
            &small_zip,
            ArchiveFormat::Zip,
            "app.exe",
            &cap_dest,
            1024,
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::SizeMismatch { .. }));
        assert!(!cap_dest.exists());
    }

    #[test]
    fn preflight_writable_accepts_writable_dir_and_rejects_unwritable_path() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("gateterm.exe");
        std::fs::write(&exe_path, b"exe").unwrap();

        let canonical = preflight_writable(&exe_path).unwrap();
        assert_eq!(canonical, std::fs::canonicalize(&exe_path).unwrap());
        // Probe file must have been cleaned up
        let remaining: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(remaining.len(), 1);

        #[cfg(unix)]
        {
            let link_dir = tempfile::tempdir().unwrap();
            let link_path = link_dir.path().join("gateterm-link");
            std::os::unix::fs::symlink(&exe_path, &link_path).unwrap();
            let resolved = preflight_writable(&link_path).unwrap();
            assert_eq!(resolved, canonical);
        }

        let missing = dir.path().join("nonexistent-subdir").join("gateterm.exe");
        let err = preflight_writable(&missing).unwrap_err();
        assert!(matches!(err, DownloadError::NotWritable { .. }));

        let cache = updates_cache_dir().unwrap();
        assert_eq!(cache.file_name().and_then(|s| s.to_str()), Some("updates"));
    }
}
