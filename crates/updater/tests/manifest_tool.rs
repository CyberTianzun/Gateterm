use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use gateterm_updater::{
    ArchiveFormat, MAX_MANIFEST_BYTES, MAX_NOTES_BYTES, Manifest, TargetArtifact,
    compiled_public_key_b64, generate_keypair, sign_manifest_bytes, validate_manifest,
};
use sha2::{Digest, Sha256};

fn manifest_tool_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_manifest-tool"))
}

fn sha256_lower_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn parse_generated_keys(stdout: &str) -> (String, String) {
    let mut priv_key = None;
    let mut pub_key = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("UPDATER_SIGNING_KEY=") {
            priv_key = Some(val.to_owned());
        } else if let Some(val) = line.strip_prefix("EMBEDDED_PUBLIC_KEY_B64=") {
            pub_key = Some(val.to_owned());
        }
    }
    (
        priv_key.expect("stdout must contain UPDATER_SIGNING_KEY="),
        pub_key.expect("stdout must contain EMBEDDED_PUBLIC_KEY_B64="),
    )
}

fn expected_archive_specs(
    version: &str,
) -> [(&'static str, String, ArchiveFormat, &'static str); 4] {
    [
        (
            "desktop-web-x86_64-pc-windows-msvc",
            format!("gateterm-desktop-web-v{version}-x86_64-pc-windows-msvc.zip"),
            ArchiveFormat::Zip,
            "gateterm-desktop-web.exe",
        ),
        (
            "desktop-native-x86_64-pc-windows-msvc",
            format!("gateterm-desktop-native-v{version}-x86_64-pc-windows-msvc.zip"),
            ArchiveFormat::Zip,
            "gateterm-desktop-native.exe",
        ),
        (
            "desktop-web-x86_64-unknown-linux-gnu",
            format!("gateterm-desktop-web-v{version}-x86_64-unknown-linux-gnu.tar.gz"),
            ArchiveFormat::TarGz,
            "gateterm-desktop-web/gateterm-desktop-web",
        ),
        (
            "desktop-native-x86_64-unknown-linux-gnu",
            format!("gateterm-desktop-native-v{version}-x86_64-unknown-linux-gnu.tar.gz"),
            ArchiveFormat::TarGz,
            "gateterm-desktop-native/gateterm-desktop-native",
        ),
    ]
}

fn write_four_archives(dist_dir: &Path, version: &str) -> Vec<(String, Vec<u8>)> {
    fs::create_dir_all(dist_dir).unwrap();
    let specs = expected_archive_specs(version);
    let mut created = Vec::with_capacity(4);
    for (idx, (_, filename, _, _)) in specs.iter().enumerate() {
        let content = format!("archive-payload-{idx}-{filename}-data").into_bytes();
        fs::write(dist_dir.join(filename), &content).unwrap();
        created.push((filename.clone(), content));
    }
    created
}

fn assert_valid_utc_rfc3339(pub_date: &str) {
    assert_eq!(pub_date.len(), 20, "unexpected pub_date format: {pub_date}");
    let bytes = pub_date.as_bytes();
    assert_eq!(bytes[4], b'-');
    assert_eq!(bytes[7], b'-');
    assert_eq!(bytes[10], b'T');
    assert_eq!(bytes[13], b':');
    assert_eq!(bytes[16], b':');
    assert_eq!(bytes[19], b'Z');
    for i in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        assert!(
            bytes[i].is_ascii_digit(),
            "non-digit in pub_date {pub_date} at {i}"
        );
    }
}

#[test]
fn manifest_tool_generates_keys_builds_signs_and_verifies_four_desktop_targets() {
    let bin = manifest_tool_bin();
    let tmp = tempfile::tempdir().unwrap();
    let dist_dir = tmp.path().join("dist");
    let notes_path = tmp.path().join("notes.md");
    let manifest_path = dist_dir.join("update-manifest.json");
    let sig_path = dist_dir.join("update-manifest.json.sig");

    // 1. Run manifest-tool generate-key -> parse private & public keys
    let key_out = Command::new(&bin).arg("generate-key").output().unwrap();
    assert!(
        key_out.status.success(),
        "generate-key failed: {}",
        String::from_utf8_lossy(&key_out.stderr)
    );
    let (priv_b64, pub_b64) = parse_generated_keys(&String::from_utf8_lossy(&key_out.stdout));
    assert!(!priv_b64.is_empty());
    assert!(!pub_b64.is_empty());

    // 2. Create temp dist/ with all 4 Windows/Linux desktop-web & desktop-native archives + notes.md
    let archives = write_four_archives(&dist_dir, "1.0.4");
    let notes_content = "## What's Changed\n- Sparkle self-update\n- 性能优化\n";
    fs::write(&notes_path, notes_content).unwrap();

    // 3. Run manifest-tool build-and-sign with UPDATER_SIGNING_KEY=<priv>
    let build_out = Command::new(&bin)
        .args([
            "build-and-sign",
            "--dist",
            dist_dir.to_str().unwrap(),
            "--version",
            "1.0.4",
            "--tag",
            "release-v1.0.4",
            "--repo",
            "CyberTianzun/Gateterm",
            "--notes-file",
            notes_path.to_str().unwrap(),
            "--out-manifest",
            manifest_path.to_str().unwrap(),
            "--out-sig",
            sig_path.to_str().unwrap(),
        ])
        .env("UPDATER_SIGNING_KEY", &priv_b64)
        .output()
        .unwrap();
    assert!(
        build_out.status.success(),
        "build-and-sign failed: {}",
        String::from_utf8_lossy(&build_out.stderr)
    );
    assert!(manifest_path.is_file());
    assert!(sig_path.is_file());

    // 4. Verify manifest contains exact 4 target keys, exact sizes, lowercase sha256, release_url, notes
    let raw_manifest = fs::read(&manifest_path).unwrap();
    let manifest: Manifest = serde_json::from_slice(&raw_manifest).unwrap();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.version, "1.0.4");
    assert_eq!(manifest.tag, "release-v1.0.4");
    assert_eq!(
        manifest.release_url,
        "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4"
    );
    assert_eq!(manifest.notes, notes_content);
    assert_valid_utc_rfc3339(&manifest.pub_date);
    validate_manifest(&manifest).unwrap();

    let specs = expected_archive_specs("1.0.4");
    let expected_keys: BTreeSet<String> =
        specs.iter().map(|(k, _, _, _)| (*k).to_owned()).collect();
    let actual_keys: BTreeSet<String> = manifest.targets.keys().cloned().collect();
    assert_eq!(actual_keys, expected_keys);

    for ((target_key, filename, format, binary_path), (_, bytes)) in
        specs.iter().zip(archives.iter())
    {
        let artifact = &manifest.targets[*target_key];
        assert_eq!(
            artifact.url,
            format!(
                "https://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/{filename}"
            )
        );
        assert_eq!(artifact.size, bytes.len() as u64);
        assert_eq!(artifact.sha256, sha256_lower_hex(bytes));
        assert_eq!(artifact.format, *format);
        assert_eq!(artifact.binary_path, *binary_path);
    }

    // 5. Run manifest-tool verify --public-key <pub> -> exits 0
    let verify_ok = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--sig",
            sig_path.to_str().unwrap(),
            "--public-key",
            &pub_b64,
        ])
        .output()
        .unwrap();
    assert!(
        verify_ok.status.success(),
        "verify should succeed: {}",
        String::from_utf8_lossy(&verify_ok.stderr)
    );

    // 6. Tamper 1 byte in manifest -> verify exits non-zero
    let mut tampered = raw_manifest.clone();
    let last_idx = tampered.len() - 1;
    tampered[last_idx] = if tampered[last_idx] == b'\n' {
        b' '
    } else {
        b'\n'
    };
    fs::write(&manifest_path, &tampered).unwrap();

    let verify_tampered = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--sig",
            sig_path.to_str().unwrap(),
            "--public-key",
            &pub_b64,
        ])
        .output()
        .unwrap();
    assert!(
        !verify_tampered.status.success(),
        "verify must fail on tampered manifest"
    );

    // Restore original manifest bytes before testing wrong public key
    fs::write(&manifest_path, &raw_manifest).unwrap();

    // 7. Run verify with wrong public key -> exits non-zero
    let (_, wrong_pub_b64) = generate_keypair();
    let verify_wrong_key = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--sig",
            sig_path.to_str().unwrap(),
            "--public-key",
            &wrong_pub_b64,
        ])
        .output()
        .unwrap();
    assert!(
        !verify_wrong_key.status.success(),
        "verify must fail with wrong public key"
    );

    // 8. Missing one of the 4 archives -> build-and-sign exits non-zero
    for (_, missing_name, _, _) in specs {
        let missing_dir = tmp.path().join(format!("dist-missing-{missing_name}"));
        write_four_archives(&missing_dir, "1.0.4");
        fs::remove_file(missing_dir.join(&missing_name)).unwrap();
        let out_m = tmp.path().join("m.json");
        let out_s = tmp.path().join("m.json.sig");

        let missing_res = Command::new(&bin)
            .args([
                "build-and-sign",
                "--dist",
                missing_dir.to_str().unwrap(),
                "--version",
                "1.0.4",
                "--tag",
                "release-v1.0.4",
                "--repo",
                "CyberTianzun/Gateterm",
                "--notes-file",
                notes_path.to_str().unwrap(),
                "--out-manifest",
                out_m.to_str().unwrap(),
                "--out-sig",
                out_s.to_str().unwrap(),
            ])
            .env("UPDATER_SIGNING_KEY", &priv_b64)
            .output()
            .unwrap();
        assert!(
            !missing_res.status.success(),
            "build-and-sign must fail when {missing_name} is missing"
        );
    }
}

#[test]
fn build_and_sign_and_verify_reject_missing_key_prerelease_and_invalid_schema() {
    let bin = manifest_tool_bin();
    let tmp = tempfile::tempdir().unwrap();
    let dist_dir = tmp.path().join("dist");
    let notes_path = tmp.path().join("notes.md");
    fs::write(&notes_path, "Release notes").unwrap();
    write_four_archives(&dist_dir, "1.0.4");

    let (priv_b64, pub_b64) = generate_keypair();
    let out_m = tmp.path().join("out.json");
    let out_s = tmp.path().join("out.json.sig");

    // Missing UPDATER_SIGNING_KEY env var -> fail
    let missing_env = Command::new(&bin)
        .args([
            "build-and-sign",
            "--dist",
            dist_dir.to_str().unwrap(),
            "--version",
            "1.0.4",
            "--tag",
            "release-v1.0.4",
            "--repo",
            "CyberTianzun/Gateterm",
            "--notes-file",
            notes_path.to_str().unwrap(),
            "--out-manifest",
            out_m.to_str().unwrap(),
            "--out-sig",
            out_s.to_str().unwrap(),
        ])
        .env_remove("UPDATER_SIGNING_KEY")
        .output()
        .unwrap();
    assert!(!missing_env.status.success());

    // Invalid UPDATER_SIGNING_KEY -> fail
    let bad_env = Command::new(&bin)
        .args([
            "build-and-sign",
            "--dist",
            dist_dir.to_str().unwrap(),
            "--version",
            "1.0.4",
            "--tag",
            "release-v1.0.4",
            "--repo",
            "CyberTianzun/Gateterm",
            "--notes-file",
            notes_path.to_str().unwrap(),
            "--out-manifest",
            out_m.to_str().unwrap(),
            "--out-sig",
            out_s.to_str().unwrap(),
        ])
        .env("UPDATER_SIGNING_KEY", "not-valid-base64")
        .output()
        .unwrap();
    assert!(!bad_env.status.success());

    // Prerelease version in build-and-sign -> fail
    let prerelease_dist = tmp.path().join("dist-rc");
    write_four_archives(&prerelease_dist, "1.0.4-rc.1");
    let prerelease_build = Command::new(&bin)
        .args([
            "build-and-sign",
            "--dist",
            prerelease_dist.to_str().unwrap(),
            "--version",
            "1.0.4-rc.1",
            "--tag",
            "release-v1.0.4-rc.1",
            "--repo",
            "CyberTianzun/Gateterm",
            "--notes-file",
            notes_path.to_str().unwrap(),
            "--out-manifest",
            out_m.to_str().unwrap(),
            "--out-sig",
            out_s.to_str().unwrap(),
        ])
        .env("UPDATER_SIGNING_KEY", &priv_b64)
        .output()
        .unwrap();
    assert!(!prerelease_build.status.success());

    // Signed manifest with prerelease version -> verify rejects even with valid signature
    let mut targets = std::collections::BTreeMap::new();
    targets.insert(
        "desktop-web-x86_64-pc-windows-msvc".to_owned(),
        TargetArtifact {
            url: "https://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/a.zip"
                .to_owned(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            size: 1024,
            format: ArchiveFormat::Zip,
            binary_path: "gateterm-desktop-web.exe".to_owned(),
        },
    );
    let prerelease_manifest = Manifest {
        schema_version: 1,
        version: "1.0.4-rc.1".to_owned(),
        tag: "release-v1.0.4-rc.1".to_owned(),
        pub_date: "2026-11-27T08:00:00Z".to_owned(),
        release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4-rc.1"
            .to_owned(),
        notes: "rc".to_owned(),
        targets: targets.clone(),
    };
    let pre_bytes = serde_json::to_vec_pretty(&prerelease_manifest).unwrap();
    let pre_sig = sign_manifest_bytes(&pre_bytes, &priv_b64).unwrap();
    fs::write(&out_m, &pre_bytes).unwrap();
    fs::write(&out_s, &pre_sig).unwrap();
    let verify_pre = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            out_m.to_str().unwrap(),
            "--sig",
            out_s.to_str().unwrap(),
            "--public-key",
            &pub_b64,
        ])
        .output()
        .unwrap();
    assert!(!verify_pre.status.success());

    // Signed manifest with unsupported schema_version -> verify rejects
    let bad_schema_manifest = Manifest {
        schema_version: 2,
        version: "1.0.4".to_owned(),
        tag: "release-v1.0.4".to_owned(),
        pub_date: "2026-11-27T08:00:00Z".to_owned(),
        release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4"
            .to_owned(),
        notes: "bad schema".to_owned(),
        targets,
    };
    let schema_bytes = serde_json::to_vec_pretty(&bad_schema_manifest).unwrap();
    let schema_sig = sign_manifest_bytes(&schema_bytes, &priv_b64).unwrap();
    fs::write(&out_m, &schema_bytes).unwrap();
    fs::write(&out_s, &schema_sig).unwrap();
    let verify_schema = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            out_m.to_str().unwrap(),
            "--sig",
            out_s.to_str().unwrap(),
            "--public-key",
            &pub_b64,
        ])
        .output()
        .unwrap();
    assert!(!verify_schema.status.success());

    // Valid signed manifest verified with empty public key -> fail
    let valid_manifest = Manifest {
        schema_version: 1,
        ..bad_schema_manifest
    };
    let valid_bytes = serde_json::to_vec_pretty(&valid_manifest).unwrap();
    let valid_sig = sign_manifest_bytes(&valid_bytes, &priv_b64).unwrap();
    fs::write(&out_m, &valid_bytes).unwrap();
    fs::write(&out_s, &valid_sig).unwrap();
    let verify_empty_key = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            out_m.to_str().unwrap(),
            "--sig",
            out_s.to_str().unwrap(),
            "--public-key",
            "",
        ])
        .output()
        .unwrap();
    assert!(!verify_empty_key.status.success());

    // Default --public-key uses compiled_public_key_b64() and fails when unconfigured
    if compiled_public_key_b64().trim().is_empty() {
        let verify_default_unconfigured = Command::new(&bin)
            .args([
                "verify",
                "--manifest",
                out_m.to_str().unwrap(),
                "--sig",
                out_s.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!verify_default_unconfigured.status.success());
    }
}

#[test]
fn build_and_sign_truncates_oversized_notes_on_utf8_boundary_and_enforces_256kib_cap() {
    let bin = manifest_tool_bin();
    let tmp = tempfile::tempdir().unwrap();
    let dist_dir = tmp.path().join("dist");
    write_four_archives(&dist_dir, "1.0.4");
    let (priv_b64, pub_b64) = generate_keypair();

    // 1. > 64 KiB notes spanning a multi-byte UTF-8 boundary is truncated cleanly to <= 65536 bytes.
    let notes_path = tmp.path().join("huge-notes.md");
    let mut long_notes = "a".repeat(MAX_NOTES_BYTES - 1);
    long_notes.push('中'); // 3-byte char straddling byte boundary 65535..65538
    long_notes.push_str(&"🚀".repeat(100));
    fs::write(&notes_path, &long_notes).unwrap();

    let out_m = tmp.path().join("truncated-manifest.json");
    let out_s = tmp.path().join("truncated-manifest.json.sig");
    let build_ok = Command::new(&bin)
        .args([
            "build-and-sign",
            "--dist",
            dist_dir.to_str().unwrap(),
            "--version",
            "1.0.4",
            "--tag",
            "release-v1.0.4",
            "--repo",
            "CyberTianzun/Gateterm",
            "--notes-file",
            notes_path.to_str().unwrap(),
            "--out-manifest",
            out_m.to_str().unwrap(),
            "--out-sig",
            out_s.to_str().unwrap(),
        ])
        .env("UPDATER_SIGNING_KEY", &priv_b64)
        .output()
        .unwrap();
    assert!(
        build_ok.status.success(),
        "build-and-sign should succeed after truncating notes: {}",
        String::from_utf8_lossy(&build_ok.stderr)
    );

    let raw_manifest = fs::read(&out_m).unwrap();
    assert!(raw_manifest.len() <= MAX_MANIFEST_BYTES);
    let manifest: Manifest = serde_json::from_slice(&raw_manifest).unwrap();
    assert!(manifest.notes.len() <= MAX_NOTES_BYTES);
    assert_eq!(manifest.notes.len(), MAX_NOTES_BYTES - 1);
    assert!(manifest.notes.bytes().all(|b| b == b'a'));

    let verify_ok = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            out_m.to_str().unwrap(),
            "--sig",
            out_s.to_str().unwrap(),
            "--public-key",
            &pub_b64,
        ])
        .output()
        .unwrap();
    assert!(
        verify_ok.status.success(),
        "verify must accept signed manifest with truncated notes: {}",
        String::from_utf8_lossy(&verify_ok.stderr)
    );

    // 2. Notes that stay <= 64 KiB in raw UTF-8 but JSON-escape past 256 KiB fail build-and-sign.
    let escaped_notes_path = tmp.path().join("escaped-notes.md");
    fs::write(&escaped_notes_path, "\x01".repeat(MAX_NOTES_BYTES)).unwrap();
    let escaped_m = tmp.path().join("escaped-manifest.json");
    let escaped_s = tmp.path().join("escaped-manifest.json.sig");
    let build_oversized = Command::new(&bin)
        .args([
            "build-and-sign",
            "--dist",
            dist_dir.to_str().unwrap(),
            "--version",
            "1.0.4",
            "--tag",
            "release-v1.0.4",
            "--repo",
            "CyberTianzun/Gateterm",
            "--notes-file",
            escaped_notes_path.to_str().unwrap(),
            "--out-manifest",
            escaped_m.to_str().unwrap(),
            "--out-sig",
            escaped_s.to_str().unwrap(),
        ])
        .env("UPDATER_SIGNING_KEY", &priv_b64)
        .output()
        .unwrap();
    assert!(!build_oversized.status.success());
    assert!(
        String::from_utf8_lossy(&build_oversized.stderr)
            .contains(&MAX_MANIFEST_BYTES.to_string()),
        "expected manifest size cap error, got: {}",
        String::from_utf8_lossy(&build_oversized.stderr)
    );

    // 3. Signed manifest larger than 256 KiB is rejected by `manifest-tool verify`.
    let oversized_manifest = Manifest {
        schema_version: 1,
        version: "1.0.4".to_owned(),
        tag: "release-v1.0.4".to_owned(),
        pub_date: "2026-11-27T08:00:00Z".to_owned(),
        release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4"
            .to_owned(),
        notes: "x".repeat(MAX_MANIFEST_BYTES + 128),
        targets: manifest.targets,
    };
    let oversized_bytes = serde_json::to_vec_pretty(&oversized_manifest).unwrap();
    assert!(oversized_bytes.len() > MAX_MANIFEST_BYTES);
    let oversized_sig = sign_manifest_bytes(&oversized_bytes, &priv_b64).unwrap();
    let oversized_m = tmp.path().join("oversized-manifest.json");
    let oversized_s = tmp.path().join("oversized-manifest.json.sig");
    fs::write(&oversized_m, &oversized_bytes).unwrap();
    fs::write(&oversized_s, &oversized_sig).unwrap();

    let verify_oversized = Command::new(&bin)
        .args([
            "verify",
            "--manifest",
            oversized_m.to_str().unwrap(),
            "--sig",
            oversized_s.to_str().unwrap(),
            "--public-key",
            &pub_b64,
        ])
        .output()
        .unwrap();
    assert!(!verify_oversized.status.success());
    assert!(
        String::from_utf8_lossy(&verify_oversized.stderr)
            .contains(&MAX_MANIFEST_BYTES.to_string()),
        "expected manifest size cap error in verify, got: {}",
        String::from_utf8_lossy(&verify_oversized.stderr)
    );
}
