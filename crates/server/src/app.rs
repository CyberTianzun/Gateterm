use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use futures_util::SinkExt;
use gateterm_protocol::{ClientMsg, Frame, ServerMsg, CONTROL_CHANNEL, PROTO_VERSION};

use crate::launcher::LauncherProvider;
use crate::session::manager::SessionManager;
use tokio::time::timeout;

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<SessionManager>,
    pub token: String,
    /// 新建会话菜单的来源。进程启动时装配一次,每次请求重新扫描(见 launcher 模块)。
    pub launchers: Arc<Vec<Box<dyn LauncherProvider>>>,
    /// 实际监听的端口。装 hook 时要把它写进 URL,判定「装了却端口对不上」也要它。
    /// 取的是 listener 的 `local_addr()` 而不是配置值 —— 配 `:0` 时两者不同。
    pub listen_port: u16,
    /// `[agents]` 配置。装 hook 的门控就在这里,默认关。
    pub agents: crate::config::AgentsConfig,
    /// agent 会话表。内存态,与 HTTP 层的生死无关(desktop 重建 HTTP 层时要沿用同一个)。
    pub agent_sessions: Arc<crate::agent::store::AgentSessionStore>,
    /// Cloud 登录态(只读)。握手时回带 `cloud_server_id`;未登录时为空。
    pub cloud: crate::cloud::creds::SharedCreds,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ws", any(ws_handler))
        .route("/api/launchers", get(launchers_handler))
        .route("/api/cloud/status", get(cloud_status_handler))
        .route("/api/update", get(update_status_handler))
        .route("/api/agents", get(crate::agent::routes::list))
        .route("/api/agents/{id}/plan", get(crate::agent::routes::plan))
        .route("/api/agents/{id}/install", post(crate::agent::routes::install))
        .route("/api/agents/{id}/uninstall", post(crate::agent::routes::uninstall))
        .route("/api/agents/{id}/purge", post(crate::agent::routes::purge))
        .route("/api/agents/sessions", get(crate::agent::routes::sessions))
        .route("/api/agents/history", get(crate::agent::history::list))
        .route("/api/agents/history/messages", get(crate::agent::history::messages))
        .route("/api/agents/history/open", post(crate::agent::history::open))
        .route("/api/agents/pending", get(crate::agent::routes::pending))
        .route("/api/agents/messages", get(crate::agent::routes::messages))
        .route("/api/agents/answer", post(crate::agent::routes::answer))
        .route(
            "/api/paste-image",
            post(paste_image_handler)
                .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route(
            "/hook/{agent}/{event}",
            post(crate::agent::routes::hook)
                // hook payload 里有 tool_input(命令原文、文件路径),给它一个明确上界
                .layer(axum::extract::DefaultBodyLimit::max(512 * 1024)),
        )
        .fallback(crate::assets::static_handler)
        .with_state(state)
}

/// Inspect image magic bytes so only valid image files can be staged.
pub fn detect_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.len() >= 26 && &bytes[0..2] == b"BM" {
        Some("bmp")
    } else {
        None
    }
}

pub fn save_pasted_image_bytes(
    dir: &std::path::Path,
    bytes: &[u8],
) -> anyhow::Result<std::path::PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ext = detect_image_extension(bytes).ok_or_else(|| anyhow::anyhow!("unsupported image format"))?;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }

    // Best-effort cleanup of staged pastes older than 24h.
    let now = SystemTime::now();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
                && let Ok(modified) = meta.modified()
                && now.duration_since(modified).is_ok_and(|age| age > Duration::from_secs(86_400))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let millis = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("paste-{millis:x}-{seq:x}.{ext}"));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// `POST /api/paste-image`: stage a pasted screenshot/image into the server's temporary
/// directory and return its local path so coding agents (Claude Code, Codex, etc.) can read it.
async fn paste_image_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty image body").into_response();
    }
    if detect_image_extension(&body).is_none() {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported image format").into_response();
    }
    let dir = std::env::temp_dir().join("gateterm-paste");
    match tokio::task::spawn_blocking(move || save_pasted_image_bytes(&dir, &body)).await {
        Ok(Ok(path)) => Json(serde_json::json!({
            "path": path.to_string_lossy(),
        }))
        .into_response(),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "failed to save pasted image");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to save image").into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "image task panicked").into_response(),
    }
}

/// Cached process-wide result; requesting it does not initiate another GitHub check.
async fn update_status_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) { return StatusCode::UNAUTHORIZED.into_response(); }
    ([(header::CACHE_CONTROL, "no-store")], Json(crate::update::cached_status())).into_response()
}

/// The account used by this server. Cloud credentials never leave the process.
async fn cloud_status_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let cloud = state.cloud.clone();
    match tokio::task::spawn_blocking(move || cloud.get()).await {
        Ok(creds) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({
                "connected": creds.is_some(),
                "email": creds.map(|c| c.email),
            })),
        ).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "cloud status unavailable").into_response(),
    }
}

/// `GET /api/launchers` —— 新建会话时可选的启动项。
///
/// 走 HTTP 而不是控制消息:这是一份**与会话无关的静态清单**,客户端在页面加载时
/// 就要用(那时 WebSocket 可能还没握手完),而且它天然适合被当成一次性请求 ——
/// 塞进控制通道只会给协议增加一对无状态的请求/响应,并且要为它发明一个关联键
/// (协议里没有 request id,见 docs/protocol.md C5)。
async fn launchers_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) {
        // 和 WebSocket 握手同一个 token。用 Authorization 头而不是查询参数:
        // 查询参数会进日志/历史记录,而且带头部的请求必须由 JS 发出,顺手挡掉了
        // 从别的页面用 <form>/<img> 打过来的跨站请求。
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    // 扫描要读若干配置文件,是阻塞 IO,不能占着 async 执行器
    let providers = state.launchers.clone();
    match tokio::task::spawn_blocking(move || crate::launcher::discover_all(&providers)).await {
        Ok(list) => Json(serde_json::json!({ "launchers": list })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "launcher 扫描任务失败");
            (StatusCode::INTERNAL_SERVER_ERROR, "launcher discovery failed").into_response()
        }
    }
}

pub(crate) fn bearer_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|v| v == token)
}

pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> anyhow::Result<()> {
    // 带上 ConnectInfo:`/hook/*` 要能看到对端地址,非回环一律拒收。主监听即使是
    // 0.0.0.0,hook 面也只认 127.0.0.1 —— 那条路上跑的是 tool_input,不能对外开。
    state.agent_sessions.clone().start_sweeper(
        state.manager.clone(),
        state.agents.session_stale_secs,
        state.agents.working_stale_secs,
    );
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| async move {
        let _ = handle_socket(socket, state).await;
    })
}

pub(crate) fn control_msg(msg: &ServerMsg) -> Message {
    Message::Binary(Frame::new(CONTROL_CHANNEL, serde_json::to_vec(msg).unwrap()).encode().into())
}

async fn handshake(socket: &mut WebSocket, state: &AppState) -> bool {
    let first = timeout(Duration::from_secs(5), socket.recv()).await;
    let data = match first {
        Err(_) => return reject(socket, "hello timeout").await,
        Ok(None) | Ok(Some(Err(_))) => return false, // 连接已断,无处可回
        Ok(Some(Ok(Message::Binary(data)))) => data,
        Ok(Some(Ok(_))) => return reject(socket, "expected binary hello frame").await,
    };
    let hello = Frame::decode(&data)
        .ok()
        .filter(|f| f.channel == CONTROL_CHANNEL)
        .and_then(|f| serde_json::from_slice::<ClientMsg>(&f.payload).ok());
    match hello {
        Some(ClientMsg::Hello { token, proto })
            if token == state.token && proto == PROTO_VERSION =>
        {
            let _ = socket.send(control_msg(&ServerMsg::HelloOk {
                proto: PROTO_VERSION,
                os: std::env::consts::OS.to_string(),
                win32_input_mode: crate::keyboard::win32_input_mode(),
                cloud_server_id: state.cloud.server_id(),
            })).await;
            true
        }
        _ => reject(socket, "bad hello").await,
    }
}

async fn reject(socket: &mut WebSocket, message: &str) -> bool {
    tracing::warn!(reason = message, "websocket handshake rejected");
    let _ = socket.send(control_msg(&ServerMsg::Error { message: message.into(), channel: None })).await;
    false
}

async fn handle_socket(mut socket: WebSocket, state: AppState) -> anyhow::Result<()> {
    if !handshake(&mut socket, &state).await {
        let _ = socket.close().await;
        return Ok(());
    }
    crate::conn::run(socket, state).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_image_formats_and_rejects_text() {
        assert_eq!(detect_image_extension(b"\x89PNG\r\n\x1a\n1234"), Some("png"));
        assert_eq!(detect_image_extension(b"\xFF\xD8\xFF\xE000"), Some("jpg"));
        assert_eq!(detect_image_extension(b"GIF89a1234"), Some("gif"));
        assert_eq!(detect_image_extension(b"RIFF\x00\x00\x00\x00WEBP"), Some("webp"));
        assert_eq!(detect_image_extension(&[b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some("bmp"));
        assert_eq!(detect_image_extension(b"not an image"), None);
    }

    #[test]
    fn saves_pasted_image_to_staging_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let png = b"\x89PNG\r\n\x1a\nfake-png-payload";
        let path = save_pasted_image_bytes(tmp.path(), png).unwrap();
        assert!(path.exists());
        assert_eq!(path.extension().and_then(|s| s.to_str()), Some("png"));
        assert_eq!(std::fs::read(&path).unwrap(), png);
    }
}
