//! Signed update manifest fetching, verification, and daily background scheduler.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use reqwest::StatusCode;
use semver::Version;
use url::Url;

use crate::{
    AppKind, AvailableUpdate, Manifest, UpdateState, compiled_public_key_b64,
    validate_and_select, verify_manifest_bytes,
};

/// Default static release manifest URL (avoids unauthenticated GitHub API rate limits).
pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/CyberTianzun/Gateterm/releases/latest/download/update-manifest.json";

/// Default detached Ed25519 signature URL corresponding to [`DEFAULT_MANIFEST_URL`].
pub const DEFAULT_SIGNATURE_URL: &str =
    "https://github.com/CyberTianzun/Gateterm/releases/latest/download/update-manifest.json.sig";

/// Interval between background update checks.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Initial delay after app startup before performing the first automatic update check.
pub const STARTUP_DELAY: Duration = Duration::from_secs(3);

/// Maximum permitted manifest response body size (256 KiB).
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// Maximum permitted signature response body size (1 KiB).
pub const MAX_SIGNATURE_BYTES: usize = 1024;

const MAX_REDIRECTS: usize = 5;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Configuration for fetching and verifying update manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckConfig {
    pub app: AppKind,
    pub current_version: String,
    pub manifest_url: String,
    pub signature_url: String,
    pub public_key_b64: String,
    pub state_path: PathBuf,
    pub target_key: Option<String>,
    pub allow_http_for_test: bool,
}

impl CheckConfig {
    /// Builds default production configuration for `app` and `current_version`.
    pub fn for_app(app: AppKind, current_version: &str) -> anyhow::Result<Self> {
        Ok(Self {
            app,
            current_version: current_version.to_owned(),
            manifest_url: DEFAULT_MANIFEST_URL.to_owned(),
            signature_url: DEFAULT_SIGNATURE_URL.to_owned(),
            public_key_b64: compiled_public_key_b64().to_owned(),
            state_path: UpdateState::default_path(app)?,
            target_key: app.target_key(),
            allow_http_for_test: false,
        })
    }
}

/// Outcome of a single update check against the signed release manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    UpdateAvailable {
        update: AvailableUpdate,
        skipped: bool,
    },
    UpToDate {
        current_version: String,
        checked_at: u64,
    },
}

/// Builds the shared HTTP client enforcing HTTPS-only redirects (max 5 hops) unless
/// `allow_http_for_test` is enabled.
pub fn build_http_client(
    current_version: &str,
    allow_http_for_test: bool,
) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("gateterm/{current_version}"))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(redirect_policy(allow_http_for_test));
    if allow_http_for_test {
        builder = builder.no_proxy();
    }
    builder.build().context("failed to build updater HTTP client")
}

fn redirect_policy(allow_http_for_test: bool) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        let scheme = attempt.url().scheme();
        if scheme == "https" || (allow_http_for_test && scheme == "http") {
            attempt.follow()
        } else {
            let msg = format!("blocked non-https redirect to {}", attempt.url());
            attempt.error(msg)
        }
    })
}

/// Fetches the manifest and `.sig`, verifies the Ed25519 signature over raw manifest bytes
/// before JSON deserialization, updates persistent skip/check state, and returns the outcome.
pub async fn check_now(config: &CheckConfig, manual: bool) -> anyhow::Result<CheckOutcome> {
    Version::parse(&config.current_version)
        .with_context(|| format!("invalid current version: {}", config.current_version))?;
    validate_endpoint_url(&config.manifest_url, config.allow_http_for_test, "manifest_url")?;
    validate_endpoint_url(
        &config.signature_url,
        config.allow_http_for_test,
        "signature_url",
    )?;

    let client = build_http_client(&config.current_version, config.allow_http_for_test)?;

    let manifest_resp = client
        .get(&config.manifest_url)
        .send()
        .await
        .context("failed to fetch update manifest")?;
    if manifest_resp.status() == StatusCode::NOT_FOUND {
        let (checked_at, _) = update_check_state(&config.state_path, &config.current_version)?;
        return Ok(CheckOutcome::UpToDate {
            current_version: config.current_version.clone(),
            checked_at,
        });
    }
    let manifest_resp = manifest_resp
        .error_for_status()
        .context("update manifest request returned error status")?;
    let manifest_bytes = read_body_capped(manifest_resp, MAX_MANIFEST_BYTES, "manifest").await?;

    let sig_resp = client
        .get(&config.signature_url)
        .send()
        .await
        .context("failed to fetch update manifest signature")?
        .error_for_status()
        .context("update manifest signature request returned error status")?;
    let sig_bytes = read_body_capped(sig_resp, MAX_SIGNATURE_BYTES, "signature").await?;
    let sig_text = std::str::from_utf8(&sig_bytes)
        .context("update manifest signature is not valid UTF-8")?;

    // Verify Ed25519 signature over the exact raw bytes BEFORE parsing untrusted JSON.
    verify_manifest_bytes(&manifest_bytes, sig_text, &config.public_key_b64)
        .context("update manifest signature verification failed")?;

    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .context("failed to parse verified update manifest JSON")?;
    let available = validate_and_select(
        manifest,
        config.app,
        &config.current_version,
        config.target_key.as_deref(),
    )?;

    let (checked_at, state) = update_check_state(&config.state_path, &config.current_version)?;
    match available {
        Some(update) => {
            let skipped = !manual && state.should_skip_auto(&update.version);
            Ok(CheckOutcome::UpdateAvailable { update, skipped })
        }
        None => Ok(CheckOutcome::UpToDate {
            current_version: config.current_version.clone(),
            checked_at,
        }),
    }
}

/// Starts a background task that runs an initial check after [`STARTUP_DELAY`] (`is_startup = true`)
/// and repeats every [`CHECK_INTERVAL`] (`is_startup = false`) with `MissedTickBehavior::Skip`.
pub fn start_daily_checks<F>(config: CheckConfig, on_result: F) -> tokio::task::JoinHandle<()>
where
    F: Fn(bool, anyhow::Result<CheckOutcome>) + Send + Sync + 'static,
{
    tokio::spawn(run_daily_loop(
        STARTUP_DELAY,
        CHECK_INTERVAL,
        move || {
            let cfg = config.clone();
            async move { check_now(&cfg, false).await }
        },
        on_result,
    ))
}

async fn run_daily_loop<C, Fut, F>(
    startup_delay: Duration,
    check_interval: Duration,
    mut check: C,
    on_result: F,
) where
    C: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<CheckOutcome>>,
    F: Fn(bool, anyhow::Result<CheckOutcome>),
{
    tokio::time::sleep(startup_delay).await;
    let mut interval = tokio::time::interval(check_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut is_startup = true;
    loop {
        interval.tick().await;
        let result = check().await;
        on_result(std::mem::take(&mut is_startup), result);
    }
}

fn validate_endpoint_url(
    raw: &str,
    allow_http_for_test: bool,
    label: &str,
) -> anyhow::Result<()> {
    let parsed = Url::parse(raw).with_context(|| format!("invalid {label}: {raw}"))?;
    let scheme = parsed.scheme();
    let allowed = scheme == "https" || (allow_http_for_test && scheme == "http");
    anyhow::ensure!(allowed, "{label} must use https: {raw}");
    Ok(())
}

async fn read_body_capped(
    mut response: reqwest::Response,
    max_bytes: usize,
    label: &str,
) -> anyhow::Result<Vec<u8>> {
    if let Some(content_length) = response.content_length() {
        anyhow::ensure!(
            content_length <= max_bytes as u64,
            "{label} response exceeds {max_bytes} byte limit ({content_length} bytes)"
        );
    }
    let initial_cap = response
        .content_length()
        .and_then(|len| usize::try_from(len).ok())
        .map(|len| len.min(max_bytes))
        .unwrap_or(0);
    let mut buf = Vec::with_capacity(initial_cap);
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("failed to read {label} response body"))?
    {
        let next_len = buf.len().saturating_add(chunk.len());
        anyhow::ensure!(
            next_len <= max_bytes,
            "{label} response body exceeded {max_bytes} byte limit"
        );
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn update_check_state(
    state_path: &Path,
    current_version: &str,
) -> anyhow::Result<(u64, UpdateState)> {
    let checked_at = unix_timestamp_secs();
    let mut state = UpdateState::load(state_path);
    state.clear_stale_skip(current_version);
    state.last_checked_at = Some(checked_at);
    state
        .save(state_path)
        .with_context(|| format!("failed to persist updater state at {}", state_path.display()))?;
    Ok((checked_at, state))
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArchiveFormat, Manifest, TargetArtifact, UpdateState, generate_keypair, sign_manifest_bytes,
    };
    use axum::{
        Router,
        extract::{Path as AxumPath, State},
        http::{HeaderMap, StatusCode, header},
        response::{IntoResponse, Response},
        routing::get,
    };
    use std::collections::BTreeMap;
    use std::sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    const VALID_SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[derive(Clone)]
    struct MockFeed {
        manifest_status: StatusCode,
        manifest_body: Vec<u8>,
        sig_status: StatusCode,
        sig_body: Vec<u8>,
        seen_user_agents: Arc<Mutex<Vec<String>>>,
    }

    fn signed_manifest_payload(version: &str, private_key_b64: &str) -> (Vec<u8>, Vec<u8>) {
        let mut targets = BTreeMap::new();
        targets.insert(
            "desktop-web-x86_64-pc-windows-msvc".to_owned(),
            TargetArtifact {
                url: format!(
                    "https://github.com/CyberTianzun/Gateterm/releases/download/release-v{version}/gateterm-desktop-web-v{version}-x86_64-pc-windows-msvc.zip"
                ),
                sha256: VALID_SHA256.to_owned(),
                size: 14_820_352,
                format: ArchiveFormat::Zip,
                binary_path: "gateterm-desktop-web.exe".to_owned(),
            },
        );
        let manifest = Manifest {
            schema_version: 1,
            version: version.to_owned(),
            tag: format!("release-v{version}"),
            pub_date: "2026-11-27T08:00:00Z".to_owned(),
            release_url: format!(
                "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v{version}"
            ),
            notes: format!("Release notes for {version}"),
            targets,
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let sig = sign_manifest_bytes(&manifest_bytes, private_key_b64).unwrap();
        (manifest_bytes, sig.into_bytes())
    }

    async fn start_feed_server(feed: Arc<RwLock<MockFeed>>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/update-manifest.json",
                get(|State(state): State<Arc<RwLock<MockFeed>>>, headers: HeaderMap| async move {
                    let ua = headers
                        .get(header::USER_AGENT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_owned();
                    let guard = state.read().unwrap();
                    guard.seen_user_agents.lock().unwrap().push(ua);
                    (guard.manifest_status, guard.manifest_body.clone())
                }),
            )
            .route(
                "/update-manifest.json.sig",
                get(|State(state): State<Arc<RwLock<MockFeed>>>| async move {
                    let guard = state.read().unwrap();
                    (guard.sig_status, guard.sig_body.clone())
                }),
            )
            .with_state(feed);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn fetches_verifies_and_respects_skip_on_auto_vs_manual() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("updater-desktop-web.json");
        let (priv_b64, pub_b64) = generate_keypair();
        let (manifest_104, sig_104) = signed_manifest_payload("1.0.4", &priv_b64);

        let seen_uas = Arc::new(Mutex::new(Vec::new()));
        let feed = Arc::new(RwLock::new(MockFeed {
            manifest_status: StatusCode::OK,
            manifest_body: manifest_104,
            sig_status: StatusCode::OK,
            sig_body: sig_104.clone(),
            seen_user_agents: seen_uas.clone(),
        }));
        let (base_url, server) = start_feed_server(feed.clone()).await;

        let cfg = CheckConfig {
            app: AppKind::DesktopWeb,
            current_version: "1.0.3".to_owned(),
            manifest_url: format!("{base_url}/update-manifest.json"),
            signature_url: format!("{base_url}/update-manifest.json.sig"),
            public_key_b64: pub_b64.clone(),
            state_path: state_path.clone(),
            target_key: Some("desktop-web-x86_64-pc-windows-msvc".to_owned()),
            allow_http_for_test: true,
        };

        // 1. Valid signed manifest -> UpdateAvailable { skipped: false }
        let outcome = check_now(&cfg, false).await.unwrap();
        match outcome {
            CheckOutcome::UpdateAvailable { update, skipped } => {
                assert!(!skipped);
                assert_eq!(update.version, "1.0.4");
                assert!(update.can_self_apply);
            }
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
        assert!(UpdateState::load(&state_path).last_checked_at.is_some());
        assert_eq!(
            seen_uas.lock().unwrap().last().map(String::as_str),
            Some("gateterm/1.0.3")
        );

        // 2. Persist skipped_version = "1.0.4": auto check -> skipped: true; manual -> skipped: false
        let mut state = UpdateState::load(&state_path);
        state.skipped_version = Some("1.0.4".to_owned());
        state.save(&state_path).unwrap();

        match check_now(&cfg, false).await.unwrap() {
            CheckOutcome::UpdateAvailable { update, skipped } => {
                assert_eq!(update.version, "1.0.4");
                assert!(skipped);
            }
            other => panic!("expected skipped UpdateAvailable, got {other:?}"),
        }
        match check_now(&cfg, true).await.unwrap() {
            CheckOutcome::UpdateAvailable { update, skipped } => {
                assert_eq!(update.version, "1.0.4");
                assert!(!skipped);
            }
            other => panic!("expected unskipped manual UpdateAvailable, got {other:?}"),
        }

        // 3. Manifest bumped to "1.0.5": auto check -> UpdateAvailable { skipped: false }
        let (manifest_105, sig_105) = signed_manifest_payload("1.0.5", &priv_b64);
        {
            let mut guard = feed.write().unwrap();
            guard.manifest_body = manifest_105;
            guard.sig_body = sig_105.clone();
        }
        match check_now(&cfg, false).await.unwrap() {
            CheckOutcome::UpdateAvailable { update, skipped } => {
                assert_eq!(update.version, "1.0.5");
                assert!(!skipped);
            }
            other => panic!("expected UpdateAvailable for 1.0.5, got {other:?}"),
        }

        // Stale skip cleared when current_version >= skipped_version; same/newer current -> UpToDate
        let mut upgraded_cfg = cfg.clone();
        upgraded_cfg.current_version = "1.0.5".to_owned();
        assert!(matches!(
            check_now(&upgraded_cfg, false).await.unwrap(),
            CheckOutcome::UpToDate { .. }
        ));
        assert_eq!(UpdateState::load(&state_path).skipped_version, None);

        // 4. 404 on manifest -> Ok(CheckOutcome::UpToDate { .. })
        {
            let mut guard = feed.write().unwrap();
            guard.manifest_status = StatusCode::NOT_FOUND;
            guard.manifest_body.clear();
        }
        match check_now(&cfg, false).await.unwrap() {
            CheckOutcome::UpToDate {
                current_version,
                checked_at,
            } => {
                assert_eq!(current_version, "1.0.3");
                assert!(checked_at > 0);
            }
            other => panic!("expected UpToDate on 404, got {other:?}"),
        }

        // 5. Tampered / invalid JSON verified BEFORE JSON deserialization -> signature error
        {
            let mut guard = feed.write().unwrap();
            guard.manifest_status = StatusCode::OK;
            guard.manifest_body = b"not-even-json".to_vec();
            guard.sig_body = sig_105;
        }
        let err = check_now(&cfg, false).await.unwrap_err();
        assert!(
            err.to_string().contains("signature verification failed"),
            "expected signature verification failure before JSON parse, got {err:#}"
        );

        // Missing .sig on 200 manifest -> Err (never fall back to unsigned checks)
        {
            let (m105, _) = signed_manifest_payload("1.0.5", &priv_b64);
            let mut guard = feed.write().unwrap();
            guard.manifest_status = StatusCode::OK;
            guard.manifest_body = m105;
            guard.sig_status = StatusCode::NOT_FOUND;
            guard.sig_body.clear();
        }
        assert!(check_now(&cfg, false).await.is_err());

        // Oversized manifest (> 256 KiB) and oversized .sig (> 1 KiB) -> Err
        {
            let mut guard = feed.write().unwrap();
            guard.manifest_status = StatusCode::OK;
            guard.manifest_body = vec![b' '; MAX_MANIFEST_BYTES + 1];
            guard.sig_status = StatusCode::OK;
            guard.sig_body = sig_104;
        }
        assert!(check_now(&cfg, false).await.is_err());
        {
            let (m105, _) = signed_manifest_payload("1.0.5", &priv_b64);
            let mut guard = feed.write().unwrap();
            guard.manifest_body = m105;
            guard.sig_body = vec![b'A'; MAX_SIGNATURE_BYTES + 1];
        }
        assert!(check_now(&cfg, false).await.is_err());

        // Non-HTTPS URL rejected when allow_http_for_test == false
        let mut prod_cfg = cfg.clone();
        prod_cfg.allow_http_for_test = false;
        assert!(check_now(&prod_cfg, false).await.is_err());

        // Default config sanity
        let default_cfg = CheckConfig::for_app(AppKind::DesktopWeb, "1.0.3").unwrap();
        assert_eq!(default_cfg.manifest_url, DEFAULT_MANIFEST_URL);
        assert_eq!(default_cfg.signature_url, DEFAULT_SIGNATURE_URL);
        assert!(!default_cfg.allow_http_for_test);

        server.abort();
    }

    #[tokio::test]
    async fn rejects_https_to_http_redirect_and_redirect_loops() {
        let final_hit = Arc::new(AtomicBool::new(false));
        let final_hit_clone = final_hit.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let base_for_http = base.clone();
        let base_for_hop = base.clone();
        let app = Router::new()
            .route(
                "/to-http",
                get(move || {
                    let target = format!("{base_for_http}/final");
                    async move { redirect_302(&target) }
                }),
            )
            .route(
                "/final",
                get(move || {
                    let flag = final_hit_clone.clone();
                    async move {
                        flag.store(true, Ordering::SeqCst);
                        "reached"
                    }
                }),
            )
            .route(
                "/hop/{max}/{step}",
                get(move |AxumPath((max, step)): AxumPath<(usize, usize)>| {
                    let base = base_for_hop.clone();
                    async move {
                        if step < max {
                            let next = format!("{base}/hop/{max}/{}", step + 1);
                            redirect_302(&next)
                        } else {
                            (StatusCode::OK, "ok").into_response()
                        }
                    }
                }),
            );

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // allow_http_for_test = false must reject redirect to http://...
        let strict_client = build_http_client("1.0.3", false).unwrap();
        let err = strict_client
            .get(format!("{base}/to-http"))
            .send()
            .await
            .unwrap_err();
        assert!(err.is_redirect(), "expected redirect policy error, got {err:?}");
        assert!(
            !final_hit.load(Ordering::SeqCst),
            "blocked http redirect target must never be requested"
        );

        // 5 redirects allowed; >5 redirects (6 hops) rejected
        let test_client = build_http_client("1.0.3", true).unwrap();
        let ok_resp = test_client
            .get(format!("{base}/hop/5/0"))
            .send()
            .await
            .unwrap();
        assert_eq!(ok_resp.status(), StatusCode::OK);

        let loop_err = test_client
            .get(format!("{base}/hop/6/0"))
            .send()
            .await
            .unwrap_err();
        assert!(
            loop_err.is_redirect(),
            "expected redirect loop error, got {loop_err:?}"
        );

        server.abort();
    }

    fn redirect_302(location: &str) -> Response {
        (StatusCode::FOUND, [(header::LOCATION, location.to_owned())]).into_response()
    }

    #[tokio::test(start_paused = true)]
    async fn daily_scheduler_runs_after_startup_delay_and_skips_missed_ticks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(Mutex::new(Vec::<bool>::new()));
        let calls_clone = calls.clone();
        let events_clone = events.clone();

        let handle = tokio::spawn(run_daily_loop(
            STARTUP_DELAY,
            CHECK_INTERVAL,
            move || {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok(CheckOutcome::UpToDate {
                    current_version: "1.0.3".to_owned(),
                    checked_at: 1,
                }))
            },
            move |is_startup, res| {
                assert!(res.is_ok());
                events_clone.lock().unwrap().push(is_startup);
            },
        ));

        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        tokio::time::advance(STARTUP_DELAY - Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(*events.lock().unwrap(), vec![true]);

        tokio::time::advance(CHECK_INTERVAL * 4).await;
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(*events.lock().unwrap(), vec![true, false]);

        handle.abort();
    }

    #[tokio::test]
    async fn daily_loop_wires_check_now_against_local_server() {
        let dir = tempfile::tempdir().unwrap();
        let (priv_b64, pub_b64) = generate_keypair();
        let (manifest_104, sig_104) = signed_manifest_payload("1.0.4", &priv_b64);
        let feed = Arc::new(RwLock::new(MockFeed {
            manifest_status: StatusCode::OK,
            manifest_body: manifest_104,
            sig_status: StatusCode::OK,
            sig_body: sig_104,
            seen_user_agents: Arc::new(Mutex::new(Vec::new())),
        }));
        let (base_url, server) = start_feed_server(feed).await;

        let cfg = CheckConfig {
            app: AppKind::DesktopWeb,
            current_version: "1.0.3".to_owned(),
            manifest_url: format!("{base_url}/update-manifest.json"),
            signature_url: format!("{base_url}/update-manifest.json.sig"),
            public_key_b64: pub_b64,
            state_path: dir.path().join("updater-desktop-web.json"),
            target_key: None,
            allow_http_for_test: true,
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let loop_cfg = cfg.clone();
        let loop_handle = tokio::spawn(run_daily_loop(
            Duration::from_millis(5),
            Duration::from_secs(60),
            move || {
                let cfg = loop_cfg.clone();
                async move { check_now(&cfg, false).await }
            },
            move |is_startup, res| {
                let _ = tx.send((is_startup, res));
            },
        ));

        let (is_startup, res) = rx.recv().await.unwrap();
        assert!(is_startup);
        assert!(matches!(
            res.unwrap(),
            CheckOutcome::UpdateAvailable { skipped: false, .. }
        ));
        loop_handle.abort();

        // Also ensure start_daily_checks compiles and spawns cleanly
        let prod_handle = start_daily_checks(cfg, |_, _| {});
        prod_handle.abort();
        server.abort();
    }
}
