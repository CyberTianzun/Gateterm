use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, put};
use octoterm_cloud_ffi::{CloudAccount, CloudFfiError};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<(String, Option<String>, Value)>>>;

/// 最小假 Cloud:登录成功;推送登记在 token 为 "revoked" 时回 401 unauthorized。
async fn fake() -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s1 = seen.clone();
    let s2 = seen.clone();
    let app = Router::new()
        .route(
            "/v1/auth/login",
            post(move |axum::Json(b): axum::Json<Value>| {
                let s = s1.clone();
                async move {
                    s.lock().unwrap().push(("login".into(), None, b));
                    axum::Json(json!({"token": "oct_dev", "account_id": "a1"}))
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
