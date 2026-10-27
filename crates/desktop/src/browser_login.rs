//! Short-lived loopback listener. Only the selected Cloud receives the PKCE verifier.

use std::future::IntoFuture;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::{Router, routing::get};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use gateterm_cloud_client::{CloudClient, Credentials, credentials};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use url::Url;
use crate::i18n::Locale;
use crate::tr;

pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone)]
struct CallbackState {
    state: String,
    host: String,
    sender: mpsc::Sender<(String, oneshot::Sender<Result<(), ()>>)>,
    claimed: Arc<Mutex<bool>>,
}

pub struct BrowserLogin {
    locale: Locale,
    listener: TcpListener,
    cloud_url: String,
    browser_url: Url,
    redirect_uri: String,
    verifier: String,
    state: String,
}

impl BrowserLogin {
    pub async fn bind(cloud_url: &str, label: &str, locale: Locale) -> Result<Self, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|e| tr!(locale, "无法启动本机登录回调：{e}", "Could not start the local sign-in callback: {e}"))?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let redirect_uri = format!("http://{address}/auth/callback");
        // Independent OS-random UUIDv4 values form a 64-character PKCE verifier.
        let verifier = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = uuid::Uuid::new_v4().simple().to_string();
        let mut browser_url = Url::parse(cloud_url).map_err(|_| locale.text("中继地址无效", "Invalid relay address"))?;
        browser_url.set_path("/auth/desktop");
        browser_url.set_query(None);
        browser_url
            .query_pairs_mut()
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("label", label);
        Ok(Self {
            locale,
            listener,
            cloud_url: cloud_url.into(),
            browser_url,
            redirect_uri,
            verifier,
            state,
        })
    }

    pub fn url(&self) -> &str {
        self.browser_url.as_str()
    }

    pub async fn complete(self, path: &Path) -> Result<Credentials, String> {
        self.complete_with_timeout(path, LOGIN_TIMEOUT).await
    }

    async fn complete_with_timeout(
        self,
        path: &Path,
        timeout: Duration,
    ) -> Result<Credentials, String> {
        let locale = self.locale;
        let (sender, mut receiver) = mpsc::channel(1);
        let host = self
            .listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string();
        let state = CallbackState {
            state: self.state,
            host,
            sender,
            claimed: Arc::new(Mutex::new(false)),
        };
        let router = Router::new()
            .route("/auth/callback", get(callback))
            .with_state(state);
        let (shutdown, stopped) = oneshot::channel::<()>();
        // Poll the server in this future: cancellation drops the listener and pending requests.
        let server = axum::serve(self.listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .into_future();
        let work = async {
            let (code, reply) = receiver.recv().await.ok_or(locale.text("登录回调已关闭", "The sign-in callback has closed"))?;
            let result = async {
                let client = CloudClient::new(&self.cloud_url);
                let auth = client
                    .exchange_desktop_code(&code, &self.verifier, &self.redirect_uri)
                    .await
                    .map_err(|e| locale.cloud_error(&e))?;
                let creds = Credentials {
                    url: self.cloud_url,
                    email: auth.email,
                    token: auth.token,
                    server_id: Some(auth.server_id),
                };
                if let Err(e) = credentials::save(path, &creds) {
                    let _ = client.logout(&creds.token).await;
                    return Err(tr!(locale, "无法保存登录状态：{e}", "Could not save sign-in details: {e}"));
                }
                Ok(creds)
            }
            .await;
            let _ = reply.send(result.as_ref().map(|_| ()).map_err(|_| ()));
            result
        };
        tokio::pin!(server);
        let result = tokio::select! {
            result = tokio::time::timeout(timeout, work) => result.unwrap_or_else(|_| Err(locale.text("登录已超时，请重新登录", "Sign-in timed out. Please sign in again.").into())),
            result = &mut server => return Err(tr!(locale, "本机登录回调已停止：{result:?}", "The local sign-in callback stopped: {result:?}")),
        };
        let _ = shutdown.send(());
        // Finish the callback response before releasing the listener.
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut server).await;
        result
    }
}

async fn callback(
    State(state): State<CallbackState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let chinese = callback_language(query.get("lang").map(String::as_str), &headers);
    if headers.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(state.host.as_str())
        || query.get("state") != Some(&state.state)
    {
        return page(
            StatusCode::BAD_REQUEST,
            "登录请求不匹配，请回到 Gateterm 重新登录。",
            "The sign-in request does not match. Start again from Gateterm.",
            chinese,
        );
    }
    let Some(code) = query
        .get("code")
        .filter(|c| !c.is_empty() && c.len() <= 256)
    else {
        return page(
            StatusCode::BAD_REQUEST,
            "缺少登录授权码，请回到 Gateterm 重新登录。",
            "The authorization code is missing. Start again from Gateterm.",
            chinese,
        );
    };
    {
        let mut claimed = state.claimed.lock().unwrap();
        if *claimed {
            return page(
                StatusCode::CONFLICT,
                "此登录回调已处理，请返回 Gateterm。",
                "This sign-in callback has already been handled. Return to Gateterm.",
                chinese,
            );
        }
        *claimed = true;
    }
    let (reply, response) = oneshot::channel();
    if state.sender.send((code.clone(), reply)).await.is_err() {
        return page(
            StatusCode::GONE,
            "登录已结束，请返回 Gateterm。",
            "This sign-in has ended. Return to Gateterm.",
            chinese,
        );
    }
    match response.await {
        Ok(Ok(())) => page(
            StatusCode::OK,
            "登录成功，可以关闭此页面并返回 Gateterm。",
            "Signed in. You can close this page and return to Gateterm.",
            chinese,
        ),
        _ => page(
            StatusCode::BAD_GATEWAY,
            "登录未完成，请返回 Gateterm 查看原因并重试。",
            "Sign-in could not be completed. Return to Gateterm to see the error and try again.",
            chinese,
        ),
    }
}

fn callback_language(selected: Option<&str>, headers: &HeaderMap) -> bool {
    match selected {
        Some("zh-CN") => return true,
        Some("en") => return false,
        _ => {}
    }
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    for language in accept.split(',') {
        match language
            .trim()
            .split(['-', ';'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "zh" => return true,
            "en" => return false,
            _ => {}
        }
    }
    false
}

fn page(status: StatusCode, zh: &str, en: &str, chinese: bool) -> Response {
    let message = if chinese { zh } else { en };
    let lang = if chinese { "zh-CN" } else { "en" };
    let html = format!(
        "<!doctype html><html lang=\"{lang}\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Gateterm</title><body><main><h1>Gateterm</h1><p>{message}</p></main></body></html>"
    );
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; frame-ancestors 'none'",
            ),
        ],
        Html(html),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn callback_preserves_the_web_pages_language_choice() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_LANGUAGE, "zh-TW,en;q=0.8".parse().unwrap());
        assert!(callback_language(None, &headers));
        assert!(!callback_language(Some("en"), &headers));
        headers.insert(
            header::ACCEPT_LANGUAGE,
            "en-US,zh-CN;q=0.8".parse().unwrap(),
        );
        assert!(!callback_language(None, &headers));
        assert!(callback_language(Some("zh-CN"), &headers));
    }

    async fn request(url: &Url, host: Option<&str>) -> String {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
            .await
            .unwrap();
        let address = format!("127.0.0.1:{}", url.port().unwrap());
        let host = host.unwrap_or(&address);
        let target = format!("{}?{}", url.path(), url.query().unwrap_or_default());
        stream
            .write_all(
                format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn callback_validates_state_and_host_then_exchanges_proof_and_saves_credentials() {
        let cloud = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cloud_url = format!("http://{}", cloud.local_addr().unwrap());
        let login = BrowserLogin::bind(&cloud_url, "Test Desktop", Locale::ZhCn)
            .await
            .unwrap();
        let params: std::collections::HashMap<_, _> =
            login.browser_url.query_pairs().into_owned().collect();
        let expected_challenge = params["code_challenge"].clone();
        let expected_redirect = params["redirect_uri"].clone();
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(!login.url().contains(&login.verifier));
        let router = Router::new().route(
            "/v1/auth/desktop/token",
            axum::routing::post(move |Json(body): Json<Value>| async move {
                assert_eq!(body["code"], "one-time-code");
                assert_eq!(body["redirect_uri"], expected_redirect);
                let proof = URL_SAFE_NO_PAD.encode(Sha256::digest(
                    body["code_verifier"].as_str().unwrap().as_bytes(),
                ));
                assert_eq!(proof, expected_challenge);
                Json(json!({"token":"oct_test-secret", "email":"a@example.com", "server_id":"s1"}))
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(cloud, router).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        let saved_path = path.clone();
        let task = tokio::spawn(async move { login.complete(&saved_path).await });
        let mut callback = Url::parse(&params["redirect_uri"]).unwrap();
        callback
            .query_pairs_mut()
            .append_pair("state", "wrong-state")
            .append_pair("code", "one-time-code");
        assert!(request(&callback, None).await.starts_with("HTTP/1.1 400"));
        callback.set_query(None);
        callback
            .query_pairs_mut()
            .append_pair("state", &params["state"])
            .append_pair("code", "one-time-code");
        assert!(
            request(&callback, Some("evil.example"))
                .await
                .starts_with("HTTP/1.1 400")
        );
        let response = request(&callback, None).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(!response.contains("oct_test-secret"));
        assert!(response.contains("no-store"));
        let creds = task.await.unwrap().unwrap();
        assert_eq!(creds.email, "a@example.com");
        assert_eq!(credentials::load(&path).unwrap(), Some(creds));
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", callback.port().unwrap()))
                .await
                .is_err()
        );
        server.abort();
    }

    #[tokio::test]
    async fn timeout_and_cancellation_release_the_listener_without_saving() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        let login = BrowserLogin::bind("https://gateterm.signal.top/", "Desktop", Locale::ZhCn)
            .await
            .unwrap();
        let address = login.listener.local_addr().unwrap();
        let result = login
            .complete_with_timeout(&path, Duration::from_millis(25))
            .await;
        assert!(result.unwrap_err().contains("超时"));
        assert!(!path.exists());
        assert!(tokio::net::TcpStream::connect(address).await.is_err());

        let login = BrowserLogin::bind("https://gateterm.signal.top/", "Desktop", Locale::ZhCn)
            .await
            .unwrap();
        let address = login.listener.local_addr().unwrap();
        let task = tokio::spawn(async move { login.complete(&path).await });
        tokio::task::yield_now().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }
}
