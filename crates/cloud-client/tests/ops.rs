mod common;

use common::FakeCloud;
use gateterm_cloud_client::ClientKind;
use gateterm_cloud_client::credentials;
use gateterm_cloud_client::ops::{self, OpsError};
use serde_json::json;

fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("cloud.json")
}

#[tokio::test]
async fn push_configuration_revocation_clears_saved_login() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("GET /v1/push-config", 200, json!({"platform":"aliyun","app_key":"123456"}));
    assert_eq!(ops::push_config(&path(&dir)).await.unwrap().platform, "aliyun");
    assert!(path(&dir).is_file());
    cloud.respond("GET /v1/push-config", 401, json!({"error":"unauthorized"}));
    assert!(ops::push_config(&path(&dir)).await.is_err());
    assert!(!path(&dir).exists());
}

#[tokio::test]
async fn register_saves_credentials_with_normalized_email() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/register", 201, json!({"token": "oct_t", "account_id": "a", "server_id": "s"}));
    let dir = tempfile::tempdir().unwrap();
    let c = ops::register(
        &path(&dir),
        &format!("{}/", cloud.url),
        " A@Example.com ",
        "123456",
        "pw-pw-pw",
        ClientKind::Server,
        "box",
    )
    .await
    .unwrap();
    assert_eq!(c.email, "a@example.com");
    assert_eq!(c.url, cloud.url);
    assert_eq!(credentials::load(&path(&dir)).unwrap(), Some(c));
    assert_eq!(cloud.last("POST /v1/auth/register").body["client"], json!({"kind": "server", "label": "box"}));
}

#[tokio::test]
async fn failed_login_writes_nothing() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 401, json!({"error": "invalid_credentials"}));
    let dir = tempfile::tempdir().unwrap();
    let err = ops::login(&path(&dir), &cloud.url, "a@example.com", "nope", ClientKind::Server, "box").await.unwrap_err();
    assert_eq!(err.user_message(), "邮箱或密码错误");
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

async fn logged_in(cloud: &FakeCloud, dir: &tempfile::TempDir) {
    cloud.respond("POST /v1/auth/login", 200, json!({"token": "oct_t", "account_id": "a", "server_id": "s"}));
    ops::login(&path(dir), &cloud.url, "a@example.com", "pw-pw-pw", ClientKind::Server, "box").await.unwrap();
}

#[tokio::test]
async fn logout_removes_file_even_when_cloud_is_down() {
    let dir = tempfile::tempdir().unwrap();
    credentials::save(
        &path(&dir),
        &credentials::Credentials { url: "http://127.0.0.1:1".into(), email: "a@example.com".into(), token: "t".into(), server_id: None },
    )
    .unwrap();
    let r = ops::logout(&path(&dir)).await.unwrap();
    assert!(r.is_some(), "Cloud 不可达应当如实返回");
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
    assert!(matches!(ops::logout(&path(&dir)).await, Err(OpsError::NotLoggedIn)));
}

#[tokio::test]
async fn logout_corrupt_credentials_file_logs_out_locally() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(path(&dir), b"{not json").unwrap();
    assert_eq!(ops::logout(&path(&dir)).await.unwrap(), None);
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn logout_clean_when_cloud_accepts_or_already_revoked() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/auth/logout", 401, json!({"error": "unauthorized"}));
    assert_eq!(ops::logout(&path(&dir)).await.unwrap(), None);
    assert_eq!(cloud.last("POST /v1/auth/logout").auth.as_deref(), Some("Bearer oct_t"));
}

#[tokio::test]
async fn reset_password_drops_matching_local_login() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/auth/password-reset", 204, json!(null));
    ops::reset_password(&path(&dir), &cloud.url, "other@example.com", "123456", "new-pass-1").await.unwrap();
    assert!(credentials::load(&path(&dir)).unwrap().is_some(), "别的邮箱不该影响本机登录态");
    ops::reset_password(&path(&dir), &cloud.url, "A@example.com", "123456", "new-pass-1").await.unwrap();
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn request_delete_code_401_unauthorized_removes_credentials() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/account/delete-code", 401, json!({"error": "unauthorized"}));
    let err = ops::request_delete_code(&path(&dir)).await.unwrap_err();
    assert_eq!(err.user_message(), "Cloud 登录已失效,请重新登录");
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn request_delete_code_400_keeps_credentials() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/account/delete-code", 400, json!({"error": "wrong_password"}));
    assert!(ops::request_delete_code(&path(&dir)).await.is_err());
    assert!(credentials::load(&path(&dir)).unwrap().is_some());
}

#[tokio::test]
async fn account_status_401_unauthorized_removes_credentials() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("GET /v1/account", 401, json!({"error": "unauthorized"}));
    let err = ops::account_status(&path(&dir)).await.unwrap_err();
    assert_eq!(err.user_message(), "Cloud 登录已失效,请重新登录");
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn account_status_400_keeps_credentials() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("GET /v1/account", 400, json!({"error": "wrong_password"}));
    assert!(ops::account_status(&path(&dir)).await.is_err());
    assert!(credentials::load(&path(&dir)).unwrap().is_some());
}

#[tokio::test]
async fn account_status_success_returns_account_info() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("GET /v1/account", 200, json!({"email": "a@example.com", "servers": [], "devices": []}));
    let info = ops::account_status(&path(&dir)).await.unwrap();
    assert_eq!(info.email.as_deref(), Some("a@example.com"));
}

#[tokio::test]
async fn reset_password_keeps_login_for_a_different_cloud_url() {
    let cloud = FakeCloud::start().await;
    let other_cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    other_cloud.respond("POST /v1/auth/password-reset", 204, json!(null));
    // 同一个邮箱,但重置发生在另一个 Cloud 地址上:不该影响本机在 `cloud.url` 的登录态。
    ops::reset_password(&path(&dir), &other_cloud.url, "a@example.com", "123456", "new-pass-1").await.unwrap();
    assert!(credentials::load(&path(&dir)).unwrap().is_some());
}

#[tokio::test]
async fn reset_password_matches_url_ignoring_trailing_slash() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/auth/password-reset", 204, json!(null));
    ops::reset_password(&path(&dir), &format!("{}/", cloud.url), "a@example.com", "123456", "new-pass-1")
        .await
        .unwrap();
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn reset_password_succeeds_even_if_local_file_is_corrupt() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(path(&dir), b"{not json").unwrap();
    cloud.respond("POST /v1/auth/password-reset", 204, json!(null));
    // 远端重置已经成功,本地清理只是 best-effort:文件损坏不该让用户以为重置失败而去重试已消费的验证码。
    ops::reset_password(&path(&dir), &cloud.url, "a@example.com", "123456", "new-pass-1").await.unwrap();
    assert_eq!(cloud.last("POST /v1/auth/password-reset").body["email"], json!("a@example.com"));
}

#[tokio::test]
async fn delete_account_flow() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("POST /v1/account/delete-code", 202, json!(null));
    assert_eq!(ops::request_delete_code(&path(&dir)).await.unwrap(), "a@example.com");

    cloud.respond("DELETE /v1/account", 400, json!({"error": "wrong_password"}));
    let err = ops::delete_account(&path(&dir), "bad", "123456").await.unwrap_err();
    assert_eq!(err.user_message(), "密码错误");
    assert!(credentials::load(&path(&dir)).unwrap().is_some());

    cloud.respond("DELETE /v1/account", 204, json!(null));
    ops::delete_account(&path(&dir), "pw-pw-pw", "123456").await.unwrap();
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}

#[tokio::test]
async fn device_login_sends_device_kind() {
    let cloud = FakeCloud::start().await;
    cloud.respond("POST /v1/auth/login", 200, json!({"token": "oct_d", "account_id": "a"}));
    let dir = tempfile::tempdir().unwrap();
    ops::login(&path(&dir), &cloud.url, "a@example.com", "pw-pw-pw", ClientKind::Device, "Pixel 9").await.unwrap();
    assert_eq!(cloud.last("POST /v1/auth/login").body["client"], json!({"kind": "device", "label": "Pixel 9"}));
}

#[tokio::test]
async fn register_push_target_revokes_on_401() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    logged_in(&cloud, &dir).await;
    cloud.respond("PUT /v1/push-target", 204, json!(null));
    ops::register_push_target(&path(&dir), "fcm", "tok").await.unwrap();
    cloud.respond("PUT /v1/push-target", 401, json!({"error": "unauthorized"}));
    assert!(ops::register_push_target(&path(&dir), "fcm", "tok").await.is_err());
    assert_eq!(credentials::load(&path(&dir)).unwrap(), None);
}
