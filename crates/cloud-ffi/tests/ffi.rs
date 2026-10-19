use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{post, put};
use octoterm_cloud_ffi::{CloudAccount, CloudFfiError, Purpose};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<(String, Option<String>, Value)>>>;

/// 最小假 Cloud:登录成功(密码 "wrongpw" 除外,回 400 wrong_password);
/// 推送登记在 token 为 "revoked" 时回 401 unauthorized;
/// 验证码接口在邮箱为 "ratelimited@example.com" 时回 429 + Retry-After。
async fn fake() -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s1 = seen.clone();
    let s2 = seen.clone();
    let s3 = seen.clone();
    let app = Router::new()
        .route(
            "/v1/auth/login",
            post(move |axum::Json(b): axum::Json<Value>| {
                let s = s1.clone();
                async move {
                    s.lock().unwrap().push(("login".into(), None, b.clone()));
                    if b["password"] == "wrongpw" {
                        return (StatusCode::BAD_REQUEST, axum::Json(json!({"error": "wrong_password"}))).into_response();
                    }
                    (StatusCode::OK, axum::Json(json!({"token": "oct_dev", "account_id": "a1"}))).into_response()
                }
            }),
        )
        .route(
            "/v1/push-target",
            put(move |h: HeaderMap, axum::Json(b): axum::Json<Value>| {
                let s = s2.clone();
                async move {
                    let auth = h.get("authorization").map(|v| v.to_str().unwrap().to_string());
                    let revoked = b["push_token"] == "revoked";
                    s.lock().unwrap().push(("push".into(), auth, b));
                    if revoked {
                        (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "unauthorized"})))
                    } else {
                        (StatusCode::NO_CONTENT, axum::Json(json!(null)))
                    }
                }
            }),
        )
        .route(
            "/v1/auth/email-code",
            post(move |axum::Json(b): axum::Json<Value>| {
                let s = s3.clone();
                async move {
                    s.lock().unwrap().push(("email-code".into(), None, b.clone()));
                    if b["email"] == "ratelimited@example.com" {
                        let mut headers = HeaderMap::new();
                        headers.insert("retry-after", "42".parse().unwrap());
                        return (StatusCode::TOO_MANY_REQUESTS, headers, axum::Json(json!({"error": "rate_limited"}))).into_response();
                    }
                    (StatusCode::OK, axum::Json(json!(null))).into_response()
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

#[tokio::test]
async fn login_as_device_then_register_push_target() {
    let (url, seen) = fake().await;
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "Pixel 9".into());
    assert!(acct.status().is_none());

    let st = acct.login(url.clone(), " A@Example.com ".into(), "pw-pw-pw".into()).await.unwrap();
    assert_eq!(st.email, "a@example.com");
    assert_eq!(acct.status().unwrap().url, url);

    acct.register_push_target("fcm".into(), "fcm-tok".into()).await.unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[0].2["client"], json!({"kind": "device", "label": "Pixel 9"}));
    assert_eq!(seen[1].1.as_deref(), Some("Bearer oct_dev"));
    assert_eq!(seen[1].2, json!({"platform": "fcm", "push_token": "fcm-tok"}));
}

#[tokio::test]
async fn unauthorized_maps_to_error_code_and_clears_login() {
    let (url, _seen) = fake().await;
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "p".into());
    acct.login(url, "a@example.com".into(), "pw-pw-pw".into()).await.unwrap();
    let err = acct.register_push_target("fcm".into(), "revoked".into()).await.unwrap_err();
    assert!(matches!(err, CloudFfiError::Unauthorized), "{err:?}");
    assert!(acct.status().is_none());
}

#[tokio::test]
async fn not_logged_in_is_its_own_error() {
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "p".into());
    assert!(matches!(acct.request_delete_code().await, Err(CloudFfiError::NotLoggedIn)));
}

#[tokio::test]
async fn rate_limited_maps_retry_after() {
    let (url, _seen) = fake().await;
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "p".into());
    let err = acct.request_code(url, "ratelimited@example.com".into(), Purpose::Register).await.unwrap_err();
    assert!(matches!(err, CloudFfiError::RateLimited { retry_after: Some(42) }), "{err:?}");
}

#[tokio::test]
async fn api_error_maps_code() {
    let (url, _seen) = fake().await;
    let dir = tempfile::tempdir().unwrap();
    let acct = CloudAccount::new(dir.path().join("cloud.json").display().to_string(), "p".into());
    let err = acct.login(url, "a@example.com".into(), "wrongpw".into()).await.unwrap_err();
    assert!(matches!(&err, CloudFfiError::Api { code } if code == "wrong_password"), "{err:?}");
}
