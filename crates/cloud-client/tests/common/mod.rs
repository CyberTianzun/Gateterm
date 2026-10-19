#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct Seen {
    pub key: String,
    pub auth: Option<String>,
    pub body: Value,
}

type Canned = (u16, Value, Option<u64>);

#[derive(Clone, Default)]
struct Shared {
    seen: Arc<Mutex<Vec<Seen>>>,
    canned: Arc<Mutex<HashMap<String, Canned>>>,
}

/// 假 Cloud:按 "METHOD /path" 返回预设响应,并记下每个请求。
pub struct FakeCloud {
    pub url: String,
    shared: Shared,
}

impl FakeCloud {
    pub async fn start() -> Self {
        let shared = Shared::default();
        let app = Router::new().fallback(handle).with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, shared }
    }

    pub fn respond(&self, key: &str, status: u16, body: Value) {
        self.shared.canned.lock().unwrap().insert(key.into(), (status, body, None));
    }

    pub fn respond_retry_after(&self, key: &str, status: u16, body: Value, secs: u64) {
        self.shared.canned.lock().unwrap().insert(key.into(), (status, body, Some(secs)));
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.shared.seen.lock().unwrap().clone()
    }

    pub fn last(&self, key: &str) -> Seen {
        self.seen().into_iter().rev().find(|s| s.key == key).unwrap_or_else(|| panic!("没有收到 {key}"))
    }
}

async fn handle(State(sh): State<Shared>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let key = format!("{method} {}", uri.path());
    sh.seen.lock().unwrap().push(Seen {
        key: key.clone(),
        auth: headers.get("authorization").and_then(|v| v.to_str().ok()).map(String::from),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    let (status, json, retry) =
        sh.canned.lock().unwrap().get(&key).cloned().unwrap_or((404, json!({"error": "not_found"}), None));
    let mut resp = (StatusCode::from_u16(status).unwrap(), axum::Json(json)).into_response();
    if let Some(secs) = retry {
        resp.headers_mut().insert("retry-after", secs.to_string().parse().unwrap());
    }
    resp
}
