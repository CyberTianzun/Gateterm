//! Inbound-only virtual terminal demo. No host shell or host files exist here.
mod review;
mod screen;
mod virtual_shell;
mod workspace;

use axum::{
    Json, Router,
    extract::{
        Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use gateterm_protocol::{ClientMsg, Frame, PROTO_VERSION, ServerMsg};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio::time::{Instant, timeout};
use workspace::{Workspace, control_frame};

const MAX_FRAME: usize = 65536;
type SharedWorkspace = Arc<Mutex<Workspace>>;
#[derive(Clone)]
struct AppState {
    workspaces: Arc<BTreeMap<String, SharedWorkspace>>,
    slots: Arc<Semaphore>,
}

/// Constructs an independent service; callers supply all bearer tokens.
pub fn app(tokens: Vec<String>) -> Result<Router, String> {
    if tokens.is_empty()
        || tokens.len() > 16
        || tokens
            .iter()
            .any(|t| t.is_empty() || t.len() > 256 || t.chars().any(char::is_control))
    {
        return Err("supply 1–16 distinct nonempty tokens, at most 256 bytes each".into());
    }
    let mut workspaces = BTreeMap::new();
    for token in tokens {
        if workspaces
            .insert(token, Arc::new(Mutex::new(Workspace::default())))
            .is_some()
        {
            return Err("duplicate demo token".into());
        }
    }
    let state = AppState {
        workspaces: Arc::new(workspaces),
        slots: Arc::new(Semaphore::new(16)),
    };
    Ok(Router::new()
        .route("/ws", get(upgrade))
        .fallback(http)
        .with_state(state))
}

async fn http(State(state): State<AppState>, request: Request) -> Response {
    let path = request.uri().path();
    if path == "/api" || path.starts_with("/api/") {
        let token = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "));
        let Some(workspace) = token.and_then(|t| state.workspaces.get(t)) else {
            return StatusCode::UNAUTHORIZED.into_response();
        };
        if request.method() != Method::GET {
            return StatusCode::FORBIDDEN.into_response();
        }
        let value = match path {
            "/api/launchers" => {
                serde_json::json!({"launchers":[{"id":"demo:terminal","provider":"demo","name":"Virtual demo terminal","detail":"Virtual demo — no host shell / no real files","command":["demo"],"cwd":"/workspace"}]})
            }
            "/api/cloud/status" => serde_json::json!({"connected":false,"email":null}),
            "/api/agents" => review::agents(),
            "/api/agents/sessions" => workspace.lock().unwrap().review_snapshot(),
            "/api/agents/messages" => {
                let Ok(axum::extract::Query(query)) =
                    axum::extract::Query::<BTreeMap<String, String>>::try_from_uri(request.uri())
                else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                if !query.contains_key("agent_id")
                    || !query.contains_key("agent_session_id")
                    || (query.contains_key("after") && query.contains_key("before"))
                {
                    return StatusCode::BAD_REQUEST.into_response();
                }
                let Some(value) = workspace.lock().unwrap().review_messages(&query) else {
                    return StatusCode::NOT_FOUND.into_response();
                };
                value
            }
            "/api/agents/pending" => serde_json::json!({"pending":[]}),
            "/api/update" => {
                serde_json::json!({"current_version":env!("CARGO_PKG_VERSION"),"update":null,"checked_at":null})
            }
            _ => return StatusCode::NOT_FOUND.into_response(),
        };
        return ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response();
    }
    if request.method() != Method::GET {
        return StatusCode::NOT_FOUND.into_response();
    }
    match path {
        "/"=>([(header::CONTENT_TYPE,"text/html; charset=utf-8")],"<!doctype html><title>Virtual demo</title><h1>Virtual demo — no host shell / no real files</h1><p>Connect a Gateterm client using this listener and an operator-supplied token. All commands and files exist only in memory. No real AI execution.</p>").into_response(),
        "/healthz"=>"ok".into_response(),
        _=>StatusCode::NOT_FOUND.into_response(),
    }
}

async fn upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let Ok(permit) = state.slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(MAX_FRAME + 5)
        .max_frame_size(MAX_FRAME + 5)
        .write_buffer_size(65536)
        .max_write_buffer_size(131072)
        .on_upgrade(move |socket| connection(socket, state, permit))
        .into_response()
}

async fn write(socket: &mut WebSocket, frame: Frame) -> bool {
    matches!(
        timeout(
            Duration::from_secs(5),
            socket.send(Message::Binary(frame.encode().into()))
        )
        .await,
        Ok(Ok(()))
    )
}
async fn reject(socket: &mut WebSocket, message: &str) {
    write(
        socket,
        control_frame(ServerMsg::Error {
            message: message.into(),
            channel: None,
        }),
    )
    .await;
    let _ = timeout(Duration::from_secs(5), socket.send(Message::Close(None))).await;
}

async fn connection(mut socket: WebSocket, state: AppState, _permit: OwnedSemaphorePermit) {
    let first = timeout(Duration::from_secs(5), socket.recv()).await;
    let bytes = match first {
        Err(_) => {
            reject(&mut socket, "hello timeout").await;
            return;
        }
        Ok(Some(Ok(Message::Binary(bytes)))) => bytes,
        _ => {
            reject(&mut socket, "expected binary hello frame").await;
            return;
        }
    };
    let hello = Frame::decode(&bytes)
        .ok()
        .filter(|f| f.channel == 0)
        .and_then(|f| serde_json::from_slice::<ClientMsg>(&f.payload).ok());
    let workspace = match hello {
        Some(ClientMsg::Hello { token, proto }) if proto == PROTO_VERSION => {
            state.workspaces.get(&token).cloned()
        }
        _ => None,
    };
    let Some(workspace) = workspace else {
        reject(&mut socket, "bad hello").await;
        return;
    };
    if !write(
        &mut socket,
        control_frame(ServerMsg::HelloOk {
            proto: PROTO_VERSION,
            os: std::env::consts::OS.into(),
            win32_input_mode: false,
            cloud_server_id: None,
        }),
    )
    .await
    {
        return;
    }
    let (tx, mut rx) = mpsc::channel(64);
    let (close, mut closed) = watch::channel(false);
    let conn = workspace.lock().unwrap().connect(tx, close);
    let (mut sink, mut source) = socket.split();
    let mut ping = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(30),
        Duration::from_secs(30),
    );
    let mut last_read = Instant::now();
    loop {
        tokio::select! {
            biased;
            _=closed.changed()=>break,
            _=tokio::time::sleep_until(last_read+Duration::from_secs(90))=>break,
            _=ping.tick()=>{
                if !matches!(timeout(Duration::from_secs(5),sink.send(Message::Ping(Vec::new().into()))).await,Ok(Ok(()))){break;}
            },
            outgoing=rx.recv()=>{
                let Some(frame)=outgoing else{break;};
                if !matches!(timeout(Duration::from_secs(5),sink.send(Message::Binary(frame.encode().into()))).await,Ok(Ok(()))){break;}
            },
            incoming=source.next()=>{
                last_read=Instant::now();
                match incoming {
                    Some(Ok(Message::Binary(bytes)))=>{
                        if bytes.len()>MAX_FRAME+5 {break;}
                        let Ok(frame)=Frame::decode(&bytes) else{continue;};
                        let mut ws=workspace.lock().unwrap();
                        if frame.channel==0 {
                            match serde_json::from_slice::<ClientMsg>(&frame.payload) {
                                Ok(msg)=>ws.control(conn,msg),Err(_)=>ws.error(conn,None,"malformed control message"),
                            }
                        } else {ws.input(conn,frame.channel,&frame.payload);}
                    },
                    Some(Ok(Message::Close(_)))|Some(Err(_))|None=>break,
                    Some(Ok(Message::Ping(payload)))=>{
                        if !matches!(timeout(Duration::from_secs(5),sink.send(Message::Pong(payload))).await,Ok(Ok(()))){break;}
                    },
                    _=>{},
                }
            },
        }
    }
    workspace.lock().unwrap().disconnect(conn);
    let _ = timeout(Duration::from_secs(1), sink.send(Message::Close(None))).await;
}
