//! REST `/api/sessions/*` HTTP handlers backed by `crate::mcp::service`.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::app::{bearer_ok, AppState};
use crate::mcp::service::{self, NewSessionInput, SendInputArgs, ServiceError};

fn error_response(err: ServiceError) -> Response {
    let status = match &err {
        ServiceError::BadRequest(_) => StatusCode::BAD_REQUEST,
        ServiceError::NotFound(_) => StatusCode::NOT_FOUND,
        ServiceError::Conflict(_) => StatusCode::CONFLICT,
        ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(serde_json::json!({ "error": err.to_string() }))).into_response()
}

fn parse_session_id(raw: &str) -> Result<u64, ServiceError> {
    raw.parse::<u64>()
        .map_err(|_| ServiceError::BadRequest(format!("invalid session id: {raw}")))
}

fn parse_scrollback_lines(raw: Option<&str>) -> Result<usize, ServiceError> {
    let Some(raw) = raw else {
        return Ok(0);
    };
    let mut value = None;
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == "scrollback_lines" {
            let parsed = v.parse::<usize>().map_err(|_| {
                ServiceError::BadRequest(format!("invalid scrollback_lines: {v}"))
            })?;
            value = Some(parsed);
        }
    }
    Ok(value.unwrap_or(0))
}

fn parse_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ServiceError> {
    serde_json::from_slice(bytes)
        .map_err(|e| ServiceError::BadRequest(format!("invalid JSON body: {e}")))
}

/// `GET /api/sessions`
pub async fn list_sessions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let sessions = service::list_sessions(&state).await;
    (StatusCode::OK, Json(serde_json::json!({ "sessions": sessions }))).into_response()
}

/// `POST /api/sessions`
pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let input = if body.iter().all(u8::is_ascii_whitespace) {
        NewSessionInput::default()
    } else {
        match parse_json(&body) {
            Ok(v) => v,
            Err(e) => return error_response(e),
        }
    };
    match service::new_session(&state, input).await {
        Ok(session) => (StatusCode::CREATED, Json(serde_json::json!({ "session": session }))).into_response(),
        Err(e) => error_response(e),
    }
}

/// `GET /api/sessions/{id}/screen?scrollback_lines=0`
pub async fn read_screen(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let id = match parse_session_id(&raw_id) {
        Ok(id) => id,
        Err(e) => return error_response(e),
    };
    let scrollback_lines = match parse_scrollback_lines(raw_query.as_deref()) {
        Ok(n) => n,
        Err(e) => return error_response(e),
    };
    match service::read_session(&state, id, scrollback_lines) {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `POST /api/sessions/{id}/input`
pub async fn send_input(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let id = match parse_session_id(&raw_id) {
        Ok(id) => id,
        Err(e) => return error_response(e),
    };
    let args: SendInputArgs = match parse_json(&body) {
        Ok(v) => v,
        Err(e) => return error_response(e),
    };
    match service::send_input(&state, id, args).await {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `DELETE /api/sessions/{id}`
pub async fn kill_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_id): Path<String>,
) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let id = match parse_session_id(&raw_id) {
        Ok(id) => id,
        Err(e) => return error_response(e),
    };
    match service::kill_session(&state, id) {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(e) => error_response(e),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::header;

    use super::*;

    fn test_state() -> AppState {
        AppState {
            manager: crate::session::manager::SessionManager::new(1 << 20, Default::default()),
            token: "secret".into(),
            launchers: Arc::new(Vec::new()),
            listen_port: 0,
            agents: Default::default(),
            agent_sessions: Arc::new(crate::agent::store::AgentSessionStore::new()),
            cloud: Default::default(),
            web_title: Default::default(),
            mcp: Default::default(),
        }
    }

    fn auth_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        h
    }

    #[tokio::test]
    async fn endpoints_require_bearer_and_validate_inputs() {
        let state = test_state();

        assert_eq!(
            list_sessions(State(state.clone()), HeaderMap::new()).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            create_session(State(state.clone()), HeaderMap::new(), Bytes::from_static(b"{}"))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            read_screen(
                State(state.clone()),
                HeaderMap::new(),
                Path("bad".into()),
                RawQuery(None)
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            send_input(
                State(state.clone()),
                HeaderMap::new(),
                Path("1".into()),
                Bytes::from_static(b"{}")
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            kill_session(State(state.clone()), HeaderMap::new(), Path("1".into()))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let ok_list = list_sessions(State(state.clone()), auth_headers()).await;
        assert_eq!(ok_list.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(ok_list.into_body(), 1024 * 16).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json, serde_json::json!({ "sessions": [] }));

        let bad_cmd = create_session(
            State(state.clone()),
            auth_headers(),
            Bytes::from_static(br#"{"command":[]}"#),
        )
        .await;
        assert_eq!(bad_cmd.status(), StatusCode::BAD_REQUEST);

        let bad_launcher = create_session(
            State(state.clone()),
            auth_headers(),
            Bytes::from_static(br#"{"launcher_id":"missing"}"#),
        )
        .await;
        assert_eq!(bad_launcher.status(), StatusCode::NOT_FOUND);

        let bad_screen = read_screen(
            State(state.clone()),
            auth_headers(),
            Path("1".into()),
            RawQuery(Some("scrollback_lines=abc".into())),
        )
        .await;
        assert_eq!(bad_screen.status(), StatusCode::BAD_REQUEST);

        let missing_screen = read_screen(
            State(state.clone()),
            auth_headers(),
            Path("999".into()),
            RawQuery(Some("scrollback_lines=10".into())),
        )
        .await;
        assert_eq!(missing_screen.status(), StatusCode::NOT_FOUND);

        let missing_kill = kill_session(State(state), auth_headers(), Path("999".into())).await;
        assert_eq!(missing_kill.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn api_sessions_routes_enforce_auth_and_wire_handlers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = test_state();
        let server = tokio::spawn(async move {
            let _ = crate::app::serve(listener, state).await;
        });

        let client = reqwest::Client::new();
        for (method, path) in [
            (reqwest::Method::GET, "/api/sessions"),
            (reqwest::Method::POST, "/api/sessions"),
            (reqwest::Method::GET, "/api/sessions/1/screen"),
            (reqwest::Method::POST, "/api/sessions/1/input"),
            (reqwest::Method::DELETE, "/api/sessions/1"),
            (reqwest::Method::GET, "/api/agents/pending?session_id=1"),
        ] {
            let unauth = client
                .request(method.clone(), format!("http://{addr}{path}"))
                .send()
                .await
                .unwrap();
            assert_eq!(unauth.status(), 401, "{method} {path} must require auth");

            let sep = if path.contains('?') { '&' } else { '?' };
            let query_tok = client
                .request(method.clone(), format!("http://{addr}{path}{sep}token=secret"))
                .send()
                .await
                .unwrap();
            assert_eq!(query_tok.status(), 401, "{method} {path} must reject query token");
        }

        let list = client
            .get(format!("http://{addr}/api/sessions"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap();
        assert_eq!(list.status(), 200);
        assert_eq!(
            list.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({ "sessions": [] })
        );

        let bad_cmd = client
            .post(format!("http://{addr}/api/sessions"))
            .bearer_auth("secret")
            .json(&serde_json::json!({ "command": [""] }))
            .send()
            .await
            .unwrap();
        assert_eq!(bad_cmd.status(), 400);

        let missing_launcher = client
            .post(format!("http://{addr}/api/sessions"))
            .bearer_auth("secret")
            .json(&serde_json::json!({ "launcher_id": "unknown" }))
            .send()
            .await
            .unwrap();
        assert_eq!(missing_launcher.status(), 404);

        let missing_screen = client
            .get(format!("http://{addr}/api/sessions/999/screen?scrollback_lines=10"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap();
        assert_eq!(missing_screen.status(), 404);

        let missing_input = client
            .post(format!("http://{addr}/api/sessions/999/input"))
            .bearer_auth("secret")
            .json(&serde_json::json!({ "text": "hi", "enter": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(missing_input.status(), 404);

        let missing_kill = client
            .delete(format!("http://{addr}/api/sessions/999"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap();
        assert_eq!(missing_kill.status(), 404);

        server.abort();
    }
}
