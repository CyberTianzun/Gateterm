//! Short-lived PKCE loopback OAuth listener for Gateterm Cloud desktop sign-in,
//! aligned with `clients/gateterm-desktop-web/src/browser_login.rs`.

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
            .map_err(|e| {
                format!(
                    "{}: {e}",
                    locale.t(
                        "无法启动本机登录回调",
                        "Could not start the local sign-in callback"
                    )
                )
            })?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let redirect_uri = format!("http://{address}/auth/callback");
        let verifier = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = uuid::Uuid::new_v4().simple().to_string();
        let mut browser_url = Url::parse(cloud_url)
            .map_err(|_| locale.t("中继地址无效", "Invalid relay address").to_string())?;
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
        let server = axum::serve(self.listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .into_future();
        let work = async {
            let (code, reply) = receiver.recv().await.ok_or_else(|| {
                locale
                    .t("登录回调已关闭", "The sign-in callback has closed")
                    .to_string()
            })?;
            let result = async {
                let client = CloudClient::new(&self.cloud_url);
                let auth = client
                    .exchange_desktop_code(&code, &self.verifier, &self.redirect_uri)
                    .await
                    .map_err(|e| e.to_string())?;
                let creds = Credentials {
                    url: self.cloud_url,
                    email: auth.email,
                    token: auth.token,
                    server_id: Some(auth.server_id),
                };
                if let Err(e) = credentials::save(path, &creds) {
                    let _ = client.logout(&creds.token).await;
                    return Err(format!(
                        "{}: {e}",
                        locale.t("无法保存登录状态", "Could not save sign-in details")
                    ));
                }
                Ok(creds)
            }
            .await;
            let _ = reply.send(result.as_ref().map(|_| ()).map_err(|_| ()));
            result
        };
        tokio::pin!(server);
        let result = tokio::select! {
            result = tokio::time::timeout(timeout, work) => result.unwrap_or_else(|_| Err(locale.t("登录已超时，请重新登录", "Sign-in timed out. Please sign in again.").into())),
            result = &mut server => return Err(format!("{}: {result:?}", locale.t("本机登录回调已停止", "The local sign-in callback stopped"))),
        };
        let _ = shutdown.send(());
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
