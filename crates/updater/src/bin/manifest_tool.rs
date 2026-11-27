//! CLI tool for generating Ed25519 updater keys, building and signing `update-manifest.json`,
//! and verifying signed manifests against the compiled-in or supplied public key.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Parser, Subcommand};
use gateterm_updater::{
    ArchiveFormat, MAX_MANIFEST_BYTES, Manifest, TargetArtifact, compiled_public_key_b64,
    generate_keypair, sign_manifest_bytes, truncate_notes, validate_manifest,
    verify_manifest_bytes,
};
use semver::Version;
use sha2::{Digest, Sha256};

const SIGNING_KEY_ENV: &str = "UPDATER_SIGNING_KEY";

#[derive(Debug, Clone, Copy)]
struct DesktopTargetSpec {
    target_key: &'static str,
    app_prefix: &'static str,
    arch_suffix: &'static str,
    ext: &'static str,
    format: ArchiveFormat,
    binary_path: &'static str,
}

const DESKTOP_TARGETS: &[DesktopTargetSpec] = &[
    DesktopTargetSpec {
        target_key: "desktop-web-x86_64-pc-windows-msvc",
        app_prefix: "gateterm-desktop-web",
        arch_suffix: "x86_64-pc-windows-msvc",
        ext: "zip",
        format: ArchiveFormat::Zip,
        binary_path: "gateterm-desktop-web.exe",
    },
    DesktopTargetSpec {
        target_key: "desktop-native-x86_64-pc-windows-msvc",
        app_prefix: "gateterm-desktop-native",
        arch_suffix: "x86_64-pc-windows-msvc",
        ext: "zip",
        format: ArchiveFormat::Zip,
        binary_path: "gateterm-desktop-native.exe",
    },
    DesktopTargetSpec {
        target_key: "desktop-web-x86_64-unknown-linux-gnu",
        app_prefix: "gateterm-desktop-web",
        arch_suffix: "x86_64-unknown-linux-gnu",
        ext: "tar.gz",
        format: ArchiveFormat::TarGz,
        binary_path: "gateterm-desktop-web/gateterm-desktop-web",
    },
    DesktopTargetSpec {
        target_key: "desktop-native-x86_64-unknown-linux-gnu",
        app_prefix: "gateterm-desktop-native",
        arch_suffix: "x86_64-unknown-linux-gnu",
        ext: "tar.gz",
        format: ArchiveFormat::TarGz,
        binary_path: "gateterm-desktop-native/gateterm-desktop-native",
    },
];

#[derive(Debug, Parser)]
#[command(
    name = "manifest-tool",
    about = "Generate Ed25519 keys, build & sign update-manifest.json, and verify signatures"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate a fresh Ed25519 keypair for CI secrets and compile-time embedding.
    GenerateKey,
    /// Scan dist/ for Windows/Linux desktop archives, emit update-manifest.json, and sign it.
    BuildAndSign {
        #[arg(long)]
        dist: PathBuf,
        #[arg(long)]
        version: String,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        repo: String,
        #[arg(long)]
        notes_file: PathBuf,
        #[arg(long)]
        out_manifest: PathBuf,
        #[arg(long)]
        out_sig: PathBuf,
    },
    /// Verify manifest signature and validate manifest schema and fields.
    Verify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        sig: PathBuf,
        #[arg(long)]
        public_key: Option<String>,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("manifest-tool error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::GenerateKey => {
            let (priv_b64, pub_b64) = generate_keypair();
            println!("{SIGNING_KEY_ENV}={priv_b64}");
            println!("EMBEDDED_PUBLIC_KEY_B64={pub_b64}");
            Ok(())
        }
        Commands::BuildAndSign {
            dist,
            version,
            tag,
            repo,
            notes_file,
            out_manifest,
            out_sig,
        } => build_and_sign(
            &dist,
            &version,
            &tag,
            &repo,
            &notes_file,
            &out_manifest,
            &out_sig,
        ),
        Commands::Verify {
            manifest,
            sig,
            public_key,
        } => verify_command(&manifest, &sig, public_key.as_deref()),
    }
}

fn build_and_sign(
    dist: &Path,
    version: &str,
    tag: &str,
    repo: &str,
    notes_file: &Path,
    out_manifest: &Path,
    out_sig: &Path,
) -> anyhow::Result<()> {
    let signing_key = std::env::var(SIGNING_KEY_ENV)
        .with_context(|| format!("{SIGNING_KEY_ENV} environment variable is required"))?;
    anyhow::ensure!(
        !signing_key.trim().is_empty(),
        "{SIGNING_KEY_ENV} environment variable must not be empty"
    );

    let parsed_version =
        Version::parse(version).with_context(|| format!("invalid semver --version: {version}"))?;
    anyhow::ensure!(
        parsed_version.pre.is_empty(),
        "release manifest version must not be a prerelease: {version}"
    );
    anyhow::ensure!(
        !tag.trim().is_empty() && !tag.chars().any(char::is_whitespace),
        "invalid --tag: {tag:?}"
    );
    validate_repo_slug(repo)?;
    anyhow::ensure!(
        dist.is_dir(),
        "dist directory does not exist: {}",
        dist.display()
    );

    let raw_notes = fs::read_to_string(notes_file)
        .with_context(|| format!("failed to read notes file {}", notes_file.display()))?;
    let notes = truncate_notes(&raw_notes);

    let mut targets = BTreeMap::new();
    for spec in DESKTOP_TARGETS {
        let filename = format!(
            "{}-v{version}-{}.{}",
            spec.app_prefix, spec.arch_suffix, spec.ext
        );
        let archive_path = dist.join(&filename);
        anyhow::ensure!(
            archive_path.is_file(),
            "missing expected desktop release archive: {}",
            archive_path.display()
        );
        let (size, sha256) = hash_and_size(&archive_path)?;
        targets.insert(
            spec.target_key.to_owned(),
            TargetArtifact {
                url: format!("https://github.com/{repo}/releases/download/{tag}/{filename}"),
                sha256,
                size,
                format: spec.format,
                binary_path: spec.binary_path.to_owned(),
            },
        );
    }

    let manifest = Manifest {
        schema_version: 1,
        version: version.to_owned(),
        tag: tag.to_owned(),
        pub_date: format_rfc3339_utc(SystemTime::now())?,
        release_url: format!("https://github.com/{repo}/releases/tag/{tag}"),
        notes,
        targets,
    };

    validate_manifest(&manifest).context("generated manifest failed validation")?;

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).context("failed to serialize manifest JSON")?;
    anyhow::ensure!(
        manifest_bytes.len() <= MAX_MANIFEST_BYTES,
        "serialized manifest exceeds {MAX_MANIFEST_BYTES} byte limit ({} bytes)",
        manifest_bytes.len()
    );
    let sig_b64 = sign_manifest_bytes(&manifest_bytes, &signing_key)
        .context("failed to sign update manifest")?;

    write_output_file(out_manifest, &manifest_bytes)?;
    write_output_file(out_sig, sig_b64.as_bytes())?;
    Ok(())
}

fn verify_command(
    manifest_path: &Path,
    sig_path: &Path,
    public_key: Option<&str>,
) -> anyhow::Result<()> {
    let manifest_bytes = read_file_capped(manifest_path, MAX_MANIFEST_BYTES, "manifest")?;
    let sig_text = fs::read_to_string(sig_path)
        .with_context(|| format!("failed to read signature {}", sig_path.display()))?;

    let key = match public_key {
        Some(explicit) => explicit,
        None => compiled_public_key_b64(),
    };
    verify_manifest_bytes(&manifest_bytes, &sig_text, key)
        .context("manifest signature verification failed")?;

    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .context("failed to parse verified manifest JSON")?;
    validate_manifest(&manifest).context("manifest schema validation failed")?;
    Ok(())
}

fn read_file_capped(path: &Path, max_bytes: usize, label: &str) -> anyhow::Result<Vec<u8>> {
    let meta = fs::metadata(path)
        .with_context(|| format!("failed to stat {label} {}", path.display()))?;
    anyhow::ensure!(
        meta.len() <= max_bytes as u64,
        "{label} {} exceeds {max_bytes} byte limit ({} bytes)",
        path.display(),
        meta.len()
    );
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open {label} {}", path.display()))?;
    let mut buf = Vec::with_capacity((meta.len() as usize).min(max_bytes));
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut buf)
        .with_context(|| format!("failed to read {label} {}", path.display()))?;
    anyhow::ensure!(
        buf.len() <= max_bytes,
        "{label} {} exceeds {max_bytes} byte limit",
        path.display()
    );
    Ok(buf)
}

fn validate_repo_slug(repo: &str) -> anyhow::Result<()> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    anyhow::ensure!(
        parts.next().is_none()
            && !owner.is_empty()
            && !name.is_empty()
            && !repo.chars().any(char::is_whitespace),
        "--repo must be in <owner>/<repo> format: {repo:?}"
    );
    Ok(())
}

fn hash_and_size(path: &Path) -> anyhow::Result<(u64, String)> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open archive {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut buf = [0u8; 65_536];
    loop {
        let read = reader
            .read(&mut buf)
            .with_context(|| format!("failed to read archive {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        total = total
            .checked_add(read as u64)
            .context("archive byte count overflow")?;
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Ok((total, hex))
}

fn write_output_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn format_rfc3339_utc(time: SystemTime) -> anyhow::Result<String> {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    Ok(format_unix_secs_rfc3339(secs))
}

fn format_unix_secs_rfc3339(secs: u64) -> String {
    let days_since_epoch = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil_from_days algorithm (Unix epoch 1970-01-01 = day 0).
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_unix_epoch_and_leap_dates_to_rfc3339_utc() {
        assert_eq!(format_unix_secs_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_unix_secs_rfc3339(951_827_696),
            "2000-02-29T12:34:56Z"
        );
        assert_eq!(
            format_unix_secs_rfc3339(1_709_251_199),
            "2024-02-29T23:59:59Z"
        );
        assert_eq!(
            format_unix_secs_rfc3339(1_795_766_400),
            "2026-11-27T08:00:00Z"
        );
        assert_eq!(
            format_unix_secs_rfc3339(4_107_542_400),
            "2100-03-01T00:00:00Z"
        );
    }
}
