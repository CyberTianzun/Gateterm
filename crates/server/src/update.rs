//! Shared, best-effort GitHub Release checks for the server and desktop app.

use std::sync::{Arc, OnceLock, RwLock, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/CyberTianzun/Gateterm/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseUpdate {
    pub version: String,
    pub url: String,
}

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub update: Option<ReleaseUpdate>,
    pub checked_at: Option<u64>,
}

struct UpdateMonitor {
    started: AtomicBool,
    status: RwLock<UpdateStatus>,
}

impl UpdateMonitor {
    fn new(current_version: &str) -> Self {
        Self { started: AtomicBool::new(false), status: RwLock::new(UpdateStatus {
            current_version: current_version.into(), update: None, checked_at: None,
        }) }
    }

    fn snapshot(&self) -> UpdateStatus { self.status.read().unwrap().clone() }

    fn record_success(&self, update: Option<ReleaseUpdate>) {
        let mut status = self.status.write().unwrap();
        status.update = update;
        status.checked_at = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
    }
}

fn monitor() -> &'static Arc<UpdateMonitor> {
    static MONITOR: OnceLock<Arc<UpdateMonitor>> = OnceLock::new();
    MONITOR.get_or_init(|| Arc::new(UpdateMonitor::new(env!("CARGO_PKG_VERSION"))))
}

/// Memory-only snapshot: opening or refreshing a web page never calls GitHub.
pub fn cached_status() -> UpdateStatus { monitor().snapshot() }

/// One checker per process, independent of HTTP restarts and connected web pages.
/// The existing startup notification is called only after the first attempt;
/// subsequent checks update the sidebar cache without opening native dialogs.
pub fn start_daily_checks(
    current_version: &'static str,
    on_first: impl FnOnce(Result<Option<ReleaseUpdate>>) + Send + 'static,
) {
    let monitor = monitor().clone();
    if monitor.started.swap(true, Ordering::AcqRel) { return; }
    monitor.status.write().unwrap().current_version = current_version.into();
    tokio::spawn(run_checks(monitor, move || check_latest(current_version), on_first));
}

async fn run_checks<F, Fut, N>(monitor: Arc<UpdateMonitor>, mut check: F, on_first: N)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Option<ReleaseUpdate>>>,
    N: FnOnce(Result<Option<ReleaseUpdate>>),
{
    let mut interval = tokio::time::interval(CHECK_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut on_first = Some(on_first);
    loop {
        interval.tick().await;
        let result = check().await;
        match &result {
            Ok(update) => {
                monitor.record_success(update.clone());
                if on_first.is_none() && let Some(update) = update {
                    tracing::info!(version = %update.version, url = %update.url, "Gateterm update available");
                }
            }
            // Preserve a known update during temporary network errors/rate limits.
            Err(error) => tracing::debug!(%error, "Daily update check failed"),
        }
        if let Some(notify) = on_first.take() { notify(result); }
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

impl Release {
    fn newer_than(self, current: &Version) -> Result<Option<ReleaseUpdate>> {
        if self.draft || self.prerelease {
            return Ok(None);
        }
        // release.yml publishes release-v<semver>; also accept conventional v<semver> tags.
        let tag = self
            .tag_name
            .strip_prefix("release-")
            .unwrap_or(&self.tag_name);
        let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
            .context("invalid GitHub Release version")?;
        if !version.pre.is_empty() || !version.cmp_precedence(current).is_gt() {
            return Ok(None);
        }
        let url = reqwest::Url::parse(&self.html_url).context("invalid GitHub Release URL")?;
        anyhow::ensure!(url.scheme() == "https" && url.host_str() == Some("github.com") &&
            url.username().is_empty() && url.password().is_none() && url.port().is_none() &&
            url.path().starts_with("/CyberTianzun/Gateterm/releases/"), "unexpected GitHub Release URL");
        Ok(Some(ReleaseUpdate {
            version: version.to_string(),
            url: url.to_string(),
        }))
    }
}

/// Checks once, with a bounded request time. Callers run this in a background task and
/// keep failures out of the normal startup path.
pub async fn check_latest(current_version: &str) -> Result<Option<ReleaseUpdate>> {
    let client = Client::builder()
        .user_agent(format!("gateterm/{current_version}"))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()?;
    check_latest_from(&client, LATEST_RELEASE_API, current_version).await
}

async fn check_latest_from(
    client: &Client,
    url: &str,
    current_version: &str,
) -> Result<Option<ReleaseUpdate>> {
    let current = Version::parse(current_version).context("invalid local version")?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    // A repository without a stable release responds with 404.
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    response
        .error_for_status()?
        .json::<Release>()
        .await?
        .newer_than(&current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("https://github.com/CyberTianzun/Gateterm/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn compares_versions_numerically_and_accepts_release_tags() {
        let current = Version::parse("0.2.9").unwrap();
        for tag in ["release-v0.2.10", "v0.2.10", "0.2.10"] {
            let expected_url = release(tag).html_url;
            let update = release(tag).newer_than(&current).unwrap().unwrap();
            assert_eq!(update.version, "0.2.10");
            assert_eq!(update.url, expected_url);
        }
        for tag in ["release-v0.2.9", "release-v0.2.8", "release-v0.1.20"] {
            assert!(release(tag).newer_than(&current).unwrap().is_none());
        }
    }

    #[test]
    fn ignores_build_metadata_when_comparing_versions() {
        let current = Version::parse("0.2.2+local").unwrap();
        assert!(
            release("release-v0.2.2+other")
                .newer_than(&current)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn only_notifies_about_stable_published_releases() {
        let current = Version::parse("0.2.2").unwrap();
        let mut draft = release("release-v0.3.0");
        draft.draft = true;
        assert!(draft.newer_than(&current).unwrap().is_none());
        let mut prerelease = release("release-v0.3.0");
        prerelease.prerelease = true;
        assert!(prerelease.newer_than(&current).unwrap().is_none());
        assert!(
            release("release-v0.3.0-rc.1")
                .newer_than(&current)
                .unwrap()
                .is_none()
        );
        let current_rc = Version::parse("0.3.0-rc.1").unwrap();
        assert!(
            release("release-v0.3.0")
                .newer_than(&current_rc)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn rejects_unrecognized_version_tags() {
        let current = Version::parse("0.2.2").unwrap();
        assert!(release("nightly").newer_than(&current).is_err());
    }

    #[test]
    fn rejects_links_outside_the_repository_release_pages() {
        let current = Version::parse("0.2.2").unwrap();
        for url in ["javascript:alert(1)", "http://github.com/CyberTianzun/Gateterm/releases/tag/v1",
            "https://example.com/CyberTianzun/Gateterm/releases/tag/v1",
            "https://github.com/other/project/releases/tag/v1",
            "https://user@github.com/CyberTianzun/Gateterm/releases/tag/v1"] {
            let mut candidate = release("release-v0.3.0");
            candidate.html_url = url.into();
            assert!(candidate.newer_than(&current).is_err(), "{url}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn checks_at_startup_and_daily_preserves_errors_and_notifies_startup_once() {
        use std::sync::atomic::AtomicUsize;
        let monitor = Arc::new(UpdateMonitor::new("0.2.2"));
        let checks = Arc::new(AtomicUsize::new(0));
        let notifications = Arc::new(AtomicUsize::new(0));
        let calls = checks.clone();
        let notices = notifications.clone();
        let expected = release("release-v0.3.0").newer_than(&Version::parse("0.2.2").unwrap()).unwrap();
        let first = expected.clone();
        let worker = tokio::spawn(run_checks(monitor.clone(), move || {
            let result = match calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(first.clone()),
                1 => Err(anyhow::anyhow!("offline")),
                _ => Ok(None),
            };
            std::future::ready(result)
        }, move |result| {
            assert!(result.unwrap().is_some());
            notices.fetch_add(1, Ordering::SeqCst);
        }));
        tokio::task::yield_now().await;
        assert_eq!(checks.load(Ordering::SeqCst), 1);
        assert_eq!(monitor.snapshot().update, expected);
        assert!(monitor.snapshot().checked_at.is_some());
        tokio::time::advance(CHECK_INTERVAL - Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(checks.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(checks.load(Ordering::SeqCst), 2);
        assert_eq!(monitor.snapshot().update, expected, "a network error must not erase a known release");
        tokio::time::advance(CHECK_INTERVAL).await;
        tokio::task::yield_now().await;
        assert_eq!(checks.load(Ordering::SeqCst), 3);
        assert!(monitor.snapshot().update.is_none());
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        worker.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn missed_days_do_not_cause_a_burst_of_github_requests() {
        use std::sync::atomic::AtomicUsize;
        let checks = Arc::new(AtomicUsize::new(0));
        let calls = checks.clone();
        let worker = tokio::spawn(run_checks(Arc::new(UpdateMonitor::new("0.2.2")), move || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(None))
        }, |_| {}));
        tokio::task::yield_now().await;
        tokio::time::advance(CHECK_INTERVAL * 4).await;
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(checks.load(Ordering::SeqCst), 2);
        worker.abort();
    }

    async fn mock_check(status: StatusCode, body: &'static str) -> Result<Option<ReleaseUpdate>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/latest",
            get(move |headers: axum::http::HeaderMap| async move {
                assert_eq!(headers["accept"], "application/vnd.github+json");
                assert_eq!(headers["user-agent"], "gateterm-test");
                (status, body)
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::builder()
            .no_proxy()
            .user_agent("gateterm-test")
            .build()
            .unwrap();
        let result = check_latest_from(&client, &url, "0.2.2").await;
        server.abort();
        result
    }

    #[tokio::test]
    async fn checks_release_response_and_returns_the_release_page() {
        let update = mock_check(
            StatusCode::OK,
            r#"{
            "tag_name": "release-v0.3.0",
            "html_url": "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v0.3.0",
            "draft": false,
            "prerelease": false
        }"#,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(update.version, "0.3.0");
        assert_eq!(update.url, release("release-v0.3.0").html_url);
    }

    #[tokio::test]
    async fn handles_missing_releases_rate_limits_and_malformed_responses() {
        assert!(
            mock_check(StatusCode::NOT_FOUND, "")
                .await
                .unwrap()
                .is_none()
        );
        for status in [
            StatusCode::FORBIDDEN,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::BAD_GATEWAY,
        ] {
            assert!(mock_check(status, "").await.is_err());
        }
        assert!(mock_check(StatusCode::OK, "not json").await.is_err());
    }

    #[tokio::test]
    async fn times_out_when_the_server_does_not_respond() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        assert!(check_latest_from(&client, &url, "0.2.2").await.is_err());
    }
}
