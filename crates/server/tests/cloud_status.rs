mod common;

use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_server::cloud::creds::SharedCreds;
use serde_json::json;

#[tokio::test]
async fn cloud_status_requires_the_local_server_token_and_never_returns_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cloud.json");
    credentials::save(
        &path,
        &Credentials {
            url: "https://cloud.example.com".into(),
            email: "a@example.com".into(),
            token: "oct_cloud_secret".into(),
            server_id: Some("srv-1".into()),
        },
    )
    .unwrap();
    let addr = common::spawn_server_with_cloud(SharedCreds::new(Some(path)), "local-secret").await;
    let http = reqwest::Client::new();
    let url = format!("http://{addr}/api/cloud/status");
    for token in [None, Some("wrong")] {
        let mut request = http.get(&url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert!(!response.text().await.unwrap().contains("a@example.com"));
    }
    let response = http
        .get(&url)
        .bearer_auth("local-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body, json!({"connected":true,"email":"a@example.com"}));
    assert!(!body.to_string().contains("oct_cloud_secret"));
}

#[tokio::test]
async fn cloud_status_tracks_login_account_changes_and_logout_without_restarting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cloud.json");
    let addr = common::spawn_server_with_cloud(SharedCreds::new(Some(path.clone())), "tok").await;
    let http = reqwest::Client::new();
    let read = || async {
        http.get(format!("http://{addr}/api/cloud/status"))
            .bearer_auth("tok")
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()
    };
    assert_eq!(read().await, json!({"connected":false,"email":null}));
    for email in ["a@example.com", "another-account@example.com"] {
        credentials::save(
            &path,
            &Credentials {
                url: "https://cloud.example.com".into(),
                email: email.into(),
                token: "private".into(),
                server_id: Some("srv-1".into()),
            },
        )
        .unwrap();
        assert_eq!(read().await, json!({"connected":true,"email":email}));
    }
    credentials::delete(&path).unwrap();
    assert_eq!(read().await, json!({"connected":false,"email":null}));
}
