mod common;

use common::FakeCloud;
use octoterm_cloud_client::{ClientInfo, CloudClient, CloudError, CodePurpose, Notify};
use serde_json::json;

#[tokio::test]
async fn login_parses_auth_and_sends_client_info() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 200, json!({"token": "oct_t", "account_id": "a1", "server_id": "s1"}));
    let ok = CloudClient::new(&format!("{}/", cloud.url))
        .login("a@example.com", "pw", &ClientInfo::server("box"))
        .await
        .unwrap();
    assert_eq!((ok.token.as_str(), ok.account_id.as_str(), ok.server_id.as_deref()), ("oct_t", "a1", Some("s1")));
    assert_eq!(
        cloud.last("POST /v1/auth/login").body,
        json!({"email": "a@example.com", "password": "pw", "client": {"kind": "server", "label": "box"}})
    );
}

#[tokio::test]
async fn api_errors_keep_their_code() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 401, json!({"error": "invalid_credentials"}));
    let err = CloudClient::new(&cloud.url).login("a@example.com", "pw", &ClientInfo::server("b")).await.unwrap_err();
    assert_eq!(err, CloudError::Api { status: 401, code: "invalid_credentials".into() });
    assert_eq!(err.user_message(), "邮箱或密码错误");
}

#[tokio::test]
async fn revoked_token_is_unauthorized() {
    let cloud = FakeCloud::start().await;
    cloud.respond("GET /v1/account", 401, json!({"error": "unauthorized"}));
    let err = CloudClient::new(&cloud.url).account("oct_x").await.unwrap_err();
    assert_eq!(err, CloudError::Unauthorized);
    assert_eq!(cloud.last("GET /v1/account").auth.as_deref(), Some("Bearer oct_x"));
}

#[tokio::test]
async fn rate_limit_carries_retry_after() {
    let cloud = FakeCloud::start().await;
    cloud.respond_retry_after("POST /v1/auth/email-code", 429, json!({"error": "rate_limited"}), 42);
    let err = CloudClient::new(&cloud.url).request_code("a@example.com", CodePurpose::Register).await.unwrap_err();
    assert_eq!(err, CloudError::RateLimited { retry_after: Some(42) });
}

#[tokio::test]
async fn unreachable_cloud() {
    let err = CloudClient::new("http://127.0.0.1:1").logout("t").await.unwrap_err();
    assert!(matches!(err, CloudError::Unreachable(_)), "{err:?}");
}

#[tokio::test]
async fn notify_sends_minimal_payload() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/notify", 202, json!({"delivered": 2}));
    let n = Notify { session: 7, agent_id: "claude-code".into(), kind: "waiting".into(), tool_name: None };
    assert_eq!(CloudClient::new(&cloud.url).notify("oct_s", &n).await.unwrap(), 2);
    let seen = cloud.last("POST /v1/notify");
    assert_eq!(seen.body, json!({"session": 7, "agent_id": "claude-code", "kind": "waiting"}));
    assert_eq!(seen.auth.as_deref(), Some("Bearer oct_s"));
}

#[tokio::test]
async fn delete_account_uses_delete_with_body() {
    let cloud = FakeCloud::start().await;
    cloud.respond("DELETE /v1/account", 204, json!(null));
    CloudClient::new(&cloud.url).delete_account("oct_t", "pw", "123456").await.unwrap();
    assert_eq!(cloud.last("DELETE /v1/account").body, json!({"password": "pw", "code": "123456"}));
}
