mod common;

#[tokio::test]
async fn status_is_authenticated_uncached_and_does_not_trigger_github_checks() {
    let addr = common::start_test_server_at("update-test-token", 1 << 16, Default::default(), vec![]).await;
    let url = format!("http://{addr}/api/update");
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().unwrap();
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    assert_eq!(client.get(&url).bearer_auth("wrong").send().await.unwrap().status(), 401);
    for _ in 0..3 {
        let response = client.get(&url).bearer_auth("update-test-token").send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let status: serde_json::Value = response.json().await.unwrap();
        assert_eq!(status["current_version"], env!("CARGO_PKG_VERSION"));
        assert!(status["update"].is_null());
        assert!(status["checked_at"].is_null(), "reading the cache must not start a check");
    }
}
