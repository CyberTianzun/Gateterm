//! Shared, best-effort GitHub Release checks for the server and desktop app.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use semver::Version;
use serde::Deserialize;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/CyberTianzun/Octoterm/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseUpdate {
    pub version: String,
    pub url: String,
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
        Ok(Some(ReleaseUpdate {
            version: version.to_string(),
            url: self.html_url,
        }))
    }
}

/// Checks once, with a bounded request time. Callers run this in a background task and
/// keep failures out of the normal startup path.
pub async fn check_latest(current_version: &str) -> Result<Option<ReleaseUpdate>> {
    let client = Client::builder()
        .user_agent(format!("octoterm/{current_version}"))
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
            html_url: format!("https://github.com/CyberTianzun/Octoterm/releases/tag/{tag}"),
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

    async fn mock_check(status: StatusCode, body: &'static str) -> Result<Option<ReleaseUpdate>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/latest",
            get(move |headers: axum::http::HeaderMap| async move {
                assert_eq!(headers["accept"], "application/vnd.github+json");
                assert_eq!(headers["user-agent"], "octoterm-test");
                (status, body)
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::builder()
            .no_proxy()
            .user_agent("octoterm-test")
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
            "html_url": "https://github.com/CyberTianzun/Octoterm/releases/tag/release-v0.3.0",
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
