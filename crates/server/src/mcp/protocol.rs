//! Stateless Streamable HTTP MCP JSON-RPC 2.0 protocol layer (`/mcp`).

use std::fmt::Write as _;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use gateterm_protocol::{AgentState, SessionInfo};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::store::PendingRequest;
use crate::app::{bearer_ok, AppState};
use crate::launcher::Launcher;
use crate::mcp::service::{
    self, AnswerApprovalArgs, NewSessionInput, SendInputArgs, SendInputResult, ServiceError,
    SessionWithAgent,
};
use crate::mcp::{origin_allowed, SUPPORTED_PROTOCOL_VERSIONS};
use crate::session::pty::ScreenSnapshot;

const JSONRPC_VERSION: &str = "2.0";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

const ERR_PARSE_ERROR: i64 = -32700;
const ERR_INVALID_REQUEST: i64 = -32600;
const ERR_METHOD_NOT_FOUND: i64 = -32601;
const ERR_INVALID_PARAMS: i64 = -32602;

const SERVER_INSTRUCTIONS: &str = "Gateterm MCP server exposing live terminal sessions and hosted coding-agent approvals. \
Call `list_sessions` first to inspect existing sessions and identify any hosted agent in the `waiting` state. \
Use `list_pending_approvals` and `answer_approval` to inspect and resolve pending approval requests. \
Use `read_session` for compact plain-text screen snapshots, and `send_input` with `wait_ms` (e.g. 2000-5000 ms) \
to receive immediate post-command screen output without polling. \
Avoid calling `kill_session` on sessions you did not create or where a user or agent is actively working.";

/// Outcome of dispatching a single JSON-RPC 2.0 request or notification object.
#[derive(Debug, Clone, PartialEq)]
pub enum RpcOutcome {
    Response(Value),
    Notification,
}

#[derive(Deserialize)]
struct EmptyArgs {}

#[derive(Deserialize)]
struct ReadSessionArgs {
    session_id: u64,
    #[serde(default)]
    scrollback_lines: Option<usize>,
}

#[derive(Deserialize)]
struct SendInputToolArgs {
    session_id: u64,
    #[serde(flatten)]
    input: SendInputArgs,
}

#[derive(Deserialize)]
struct SessionIdArgs {
    session_id: u64,
}

#[derive(Deserialize)]
struct ListPendingArgs {
    #[serde(default)]
    session_id: Option<u64>,
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "result": result,
    })
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

fn valid_id(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Null)
}

fn check_guards(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if !bearer_ok(headers, &state.token) {
        return Some(StatusCode::UNAUTHORIZED.into_response());
    }
    if !origin_allowed(headers) || !state.mcp.enabled {
        return Some(StatusCode::FORBIDDEN.into_response());
    }
    None
}

/// Returns the static MCP tool definitions exposed by `tools/list`.
pub fn tool_definitions() -> Vec<Value> {
    let ro = json!({
        "readOnlyHint": true,
        "idempotentHint": true,
        "openWorldHint": false,
    });
    let mut_safe = json!({
        "readOnlyHint": false,
        "destructiveHint": false,
        "openWorldHint": false,
    });
    let mut_destructive = json!({
        "readOnlyHint": false,
        "destructiveHint": true,
        "openWorldHint": false,
    });

    vec![
        json!({
            "name": "list_sessions",
            "description": "List active terminal sessions along with the highest-priority hosted coding agent attached to each session.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            },
            "annotations": ro,
        }),
        json!({
            "name": "list_launchers",
            "description": "List available session launchers discovered from built-in shells, config.toml, and local terminal profiles.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            },
            "annotations": ro,
        }),
        json!({
            "name": "new_session",
            "description": "Create and start a new PTY terminal session, optionally seeded from a launcher_id or explicit command/cwd.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Optional display name for the session." },
                    "launcher_id": { "type": "string", "description": "Optional launcher ID from list_launchers to use as baseline." },
                    "command": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional argv array to run instead of the default shell."
                    },
                    "cwd": { "type": "string", "description": "Optional working directory path." }
                },
                "additionalProperties": false,
            },
            "annotations": mut_safe,
        }),
        json!({
            "name": "read_session",
            "description": "Read a non-intrusive plain-text snapshot of a terminal session screen without altering viewport size or input focus.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer", "minimum": 0, "description": "Target terminal session ID." },
                    "scrollback_lines": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 500,
                        "default": 0,
                        "description": "Optional scrollback lines above the visible screen (0..=500)."
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false,
            },
            "annotations": ro,
        }),
        json!({
            "name": "send_input",
            "description": "Write text and/or named keys into a terminal session, optionally waiting for output to settle and returning the resulting screen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer", "minimum": 0, "description": "Target terminal session ID." },
                    "text": { "type": "string", "description": "UTF-8 text to write before keys (max 64 KiB)." },
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Named keys sent after text (e.g. Enter, Tab, Ctrl+C, Up, Shift+Enter)."
                    },
                    "enter": { "type": "boolean", "default": false, "description": "Append Enter (CR) after text and keys." },
                    "wait_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 15000,
                        "default": 0,
                        "description": "Milliseconds to wait for output to settle (0..=15000); includes screen snapshot when > 0."
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false,
            },
            "annotations": mut_safe,
        }),
        json!({
            "name": "kill_session",
            "description": "Terminate a terminal session and its PTY child process.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer", "minimum": 0, "description": "Target terminal session ID to terminate." }
                },
                "required": ["session_id"],
                "additionalProperties": false,
            },
            "annotations": mut_destructive,
        }),
        json!({
            "name": "list_pending_approvals",
            "description": "List hosted coding-agent tool approval requests currently waiting for a decision.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer", "minimum": 0, "description": "Optional terminal session ID filter." }
                },
                "additionalProperties": false,
            },
            "annotations": ro,
        }),
        json!({
            "name": "answer_approval",
            "description": "Submit an allow or deny decision for a pending hosted coding-agent approval request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pending_id": { "type": "string", "description": "Pending approval request ID." },
                    "decision": {
                        "type": "string",
                        "enum": ["allow", "deny"],
                        "description": "Approval decision: \"allow\" or \"deny\"."
                    },
                    "message": { "type": "string", "description": "Optional explanation or denial reason sent back to the agent." },
                    "updated_input": {
                        "type": "object",
                        "description": "Optional updated tool input object when allowing with modified arguments."
                    }
                },
                "required": ["pending_id", "decision"],
                "additionalProperties": false,
            },
            "annotations": mut_safe,
        }),
    ]
}

fn format_agent_state(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "idle",
        AgentState::Thinking => "thinking",
        AgentState::Working => "working",
        AgentState::Waiting => "waiting",
        AgentState::Done => "done",
        AgentState::Error => "error",
    }
}

fn format_sessions_text(sessions: &[SessionWithAgent]) -> String {
    if sessions.is_empty() {
        return "No active terminal sessions.".to_string();
    }
    let mut out = format!("Sessions ({}):", sessions.len());
    for s in sessions {
        let _ = write!(
            out,
            "\n- #{} {:?} ({}x{})",
            s.info.id, s.info.name, s.info.cols, s.info.rows
        );
        if let Some(ref cwd) = s.info.cwd {
            let _ = write!(out, " cwd={cwd}");
        }
        if let Some(ref agent) = s.agent {
            let _ = write!(
                out,
                " [agent: {} ({})",
                agent.agent_id,
                format_agent_state(agent.state)
            );
            if let Some(ref detail) = agent.detail {
                let _ = write!(out, ", detail={detail}");
            }
            if let Some(ref pending_id) = agent.pending_id {
                let _ = write!(out, ", pending_id={pending_id}");
            }
            out.push(']');
        }
    }
    out
}

fn format_launchers_text(launchers: &[Launcher]) -> String {
    if launchers.is_empty() {
        return "No launchers available.".to_string();
    }
    let mut out = format!("Launchers ({}):", launchers.len());
    for l in launchers {
        let _ = write!(out, "\n- {}: {} — {}", l.id, l.name, l.detail);
        if let Some(ref cwd) = l.cwd {
            let _ = write!(out, " (cwd: {cwd})");
        }
    }
    out
}

fn format_new_session_text(session: &SessionInfo) -> String {
    let mut out = format!(
        "Created session #{} {:?} ({}x{}).",
        session.id, session.name, session.cols, session.rows
    );
    if let Some(ref cwd) = session.cwd {
        let _ = write!(out, " cwd={cwd}");
    }
    out
}

fn format_screen_text(snap: &ScreenSnapshot) -> String {
    let mut header = format!(
        "[session #{} {}x{} cursor=({},{}) seq={}",
        snap.session_id, snap.cols, snap.rows, snap.cursor.row, snap.cursor.col, snap.seq
    );
    if snap.alt_screen {
        header.push_str(" alt_screen");
    }
    if snap.exited {
        header.push_str(" exited");
    }
    if snap.history_returned > 0 || snap.history_available > 0 {
        let _ = write!(
            header,
            " history={}/{}",
            snap.history_returned, snap.history_available
        );
    }
    header.push(']');
    if snap.text.is_empty() {
        header
    } else {
        format!("{header}\n{}", snap.text)
    }
}

fn format_send_input_text(res: &SendInputResult) -> String {
    match res.screen {
        Some(ref snap) => format!("Input sent (seq={}).\n{}", res.seq, format_screen_text(snap)),
        None => format!("Input sent (seq={}).", res.seq),
    }
}

fn format_pending_text(pending: &[PendingRequest]) -> String {
    if pending.is_empty() {
        return "No pending approval requests.".to_string();
    }
    let mut out = format!("Pending approvals ({}):", pending.len());
    for p in pending {
        let _ = write!(out, "\n- id={} agent={}", p.id, p.agent_id);
        if let Some(sid) = p.session {
            let _ = write!(out, " session=#{sid}");
        }
        if let Some(ref tool) = p.tool_name {
            let _ = write!(out, " tool={tool}");
        }
        if !p.tool_input.is_null() {
            let _ = write!(out, " input={}", p.tool_input);
        }
        let _ = write!(out, " expires_at={}", p.expires_at);
    }
    out
}

fn call_tool_ok(text: String, structured: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
    })
}

fn call_tool_err(err: ServiceError) -> Value {
    json!({
        "content": [{ "type": "text", "text": format!("Error: {err}") }],
        "isError": true,
    })
}

fn finish_tool<T: Serialize>(
    res: Result<T, ServiceError>,
    fmt_text: impl FnOnce(&T) -> String,
    wrap_structured: impl FnOnce(&T) -> Value,
) -> Value {
    match res {
        Ok(val) => call_tool_ok(fmt_text(&val), wrap_structured(&val)),
        Err(err) => call_tool_err(err),
    }
}

fn parse_tool_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("Invalid params: {e}"))
}

fn handle_initialize(id: Value, params: Option<Value>) -> RpcOutcome {
    if let Some(Value::Array(_)) = params {
        return RpcOutcome::Response(rpc_error(
            id,
            ERR_INVALID_PARAMS,
            "Invalid params: expected object",
        ));
    }
    let negotiated = params
        .as_ref()
        .and_then(|v| v.get("protocolVersion"))
        .and_then(Value::as_str)
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);

    RpcOutcome::Response(rpc_result(
        id,
        json!({
            "protocolVersion": negotiated,
            "capabilities": {
                "tools": {},
            },
            "serverInfo": {
                "name": "gateterm",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": SERVER_INSTRUCTIONS,
        }),
    ))
}

fn handle_object_or_empty_method(
    id: Value,
    params: Option<Value>,
    result: Value,
) -> RpcOutcome {
    if let Some(Value::Array(_)) = params {
        return RpcOutcome::Response(rpc_error(
            id,
            ERR_INVALID_PARAMS,
            "Invalid params: expected object",
        ));
    }
    RpcOutcome::Response(rpc_result(id, result))
}

async fn execute_tool(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    match name {
        "list_sessions" => {
            let _: EmptyArgs = parse_tool_args(args)?;
            let sessions = service::list_sessions(state).await;
            Ok(call_tool_ok(
                format_sessions_text(&sessions),
                json!({ "sessions": sessions }),
            ))
        }
        "list_launchers" => {
            let _: EmptyArgs = parse_tool_args(args)?;
            let res = service::list_launchers(state).await;
            Ok(finish_tool(
                res,
                |l| format_launchers_text(l),
                |l| json!({ "launchers": l }),
            ))
        }
        "new_session" => {
            let input: NewSessionInput = parse_tool_args(args)?;
            let res = service::new_session(state, input).await;
            Ok(finish_tool(
                res,
                format_new_session_text,
                |s| json!({ "session": s }),
            ))
        }
        "read_session" => {
            let parsed: ReadSessionArgs = parse_tool_args(args)?;
            let res = service::read_session(
                state,
                parsed.session_id,
                parsed.scrollback_lines.unwrap_or(0),
            );
            Ok(finish_tool(
                res,
                format_screen_text,
                |s| serde_json::to_value(s).expect("ScreenSnapshot serialization"),
            ))
        }
        "send_input" => {
            let parsed: SendInputToolArgs = parse_tool_args(args)?;
            let res = service::send_input(state, parsed.session_id, parsed.input).await;
            Ok(finish_tool(
                res,
                format_send_input_text,
                |r| serde_json::to_value(r).expect("SendInputResult serialization"),
            ))
        }
        "kill_session" => {
            let parsed: SessionIdArgs = parse_tool_args(args)?;
            let res = service::kill_session(state, parsed.session_id);
            Ok(finish_tool(
                res,
                |r| format!("Killed session #{}.", r.id),
                |r| serde_json::to_value(r).expect("KillResult serialization"),
            ))
        }
        "list_pending_approvals" => {
            let parsed: ListPendingArgs = parse_tool_args(args)?;
            let pending = service::list_pending_approvals(state, parsed.session_id);
            Ok(call_tool_ok(
                format_pending_text(&pending),
                json!({ "pending": pending }),
            ))
        }
        "answer_approval" => {
            let parsed: AnswerApprovalArgs = parse_tool_args(args)?;
            if let Some(ref updated) = parsed.updated_input
                && !updated.is_object()
            {
                return Err("Invalid params: updated_input must be an object".to_string());
            }
            let res = service::answer_approval(state, parsed);
            Ok(finish_tool(
                res,
                |r| format!("Submitted '{}' for pending request {}.", r.decision, r.pending_id),
                |r| serde_json::to_value(r).expect("AnswerApprovalResult serialization"),
            ))
        }
        _ => Err(format!("Invalid params: unknown tool '{name}'")),
    }
}

async fn handle_tools_call(state: &AppState, id: Value, params: Option<Value>) -> RpcOutcome {
    let Some(Value::Object(mut obj)) = params else {
        return RpcOutcome::Response(rpc_error(
            id,
            ERR_INVALID_PARAMS,
            "Invalid params: expected object",
        ));
    };
    let Some(name) = obj.get("name").and_then(Value::as_str).map(str::to_string) else {
        return RpcOutcome::Response(rpc_error(
            id,
            ERR_INVALID_PARAMS,
            "Invalid params: missing tool name",
        ));
    };
    let args = match obj.remove("arguments") {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(v @ Value::Object(_)) => v,
        Some(_) => {
            return RpcOutcome::Response(rpc_error(
                id,
                ERR_INVALID_PARAMS,
                "Invalid params: arguments must be an object",
            ));
        }
    };

    match execute_tool(state, &name, args).await {
        Ok(call_result) => RpcOutcome::Response(rpc_result(id, call_result)),
        Err(msg) => RpcOutcome::Response(rpc_error(id, ERR_INVALID_PARAMS, msg)),
    }
}

/// Dispatches a single parsed JSON-RPC 2.0 request or notification value.
pub async fn dispatch_rpc(state: &AppState, value: Value) -> RpcOutcome {
    let Value::Object(mut obj) = value else {
        return RpcOutcome::Response(rpc_error(
            Value::Null,
            ERR_INVALID_REQUEST,
            "Invalid Request",
        ));
    };

    let id = match obj.remove("id") {
        None => None,
        Some(v) if valid_id(&v) => Some(v),
        Some(_) => {
            return RpcOutcome::Response(rpc_error(
                Value::Null,
                ERR_INVALID_REQUEST,
                "Invalid Request: invalid id",
            ));
        }
    };
    let reply_id = id.clone().unwrap_or(Value::Null);

    if obj.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC_VERSION) {
        return RpcOutcome::Response(rpc_error(
            reply_id,
            ERR_INVALID_REQUEST,
            "Invalid Request: jsonrpc must be \"2.0\"",
        ));
    }

    let Some(method) = obj.get("method").and_then(Value::as_str).map(str::to_string) else {
        return RpcOutcome::Response(rpc_error(
            reply_id,
            ERR_INVALID_REQUEST,
            "Invalid Request: method must be a string",
        ));
    };

    let params = obj.remove("params");
    if let Some(ref p) = params
        && !matches!(p, Value::Object(_) | Value::Array(_) | Value::Null)
    {
        return RpcOutcome::Response(rpc_error(
            reply_id,
            ERR_INVALID_REQUEST,
            "Invalid Request: params must be structured",
        ));
    }

    let Some(id) = id else {
        return RpcOutcome::Notification;
    };

    match method.as_str() {
        "initialize" => handle_initialize(id, params),
        "ping" => handle_object_or_empty_method(id, params, json!({})),
        "tools/list" => {
            handle_object_or_empty_method(id, params, json!({ "tools": tool_definitions() }))
        }
        "tools/call" => handle_tools_call(state, id, params).await,
        _ => RpcOutcome::Response(rpc_error(
            id,
            ERR_METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )),
    }
}

/// `GET /mcp` and `DELETE /mcp` handler after security/config pre-guards.
pub async fn handle_method_not_allowed(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(err) = check_guards(&state, &headers) {
        return err;
    }
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

/// `POST /mcp` Streamable HTTP JSON-RPC 2.0 entrypoint.
pub async fn handle_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(err) = check_guards(&state, &headers) {
        return err;
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::OK,
                Json(rpc_error(Value::Null, ERR_PARSE_ERROR, "Parse error")),
            )
                .into_response();
        }
    };

    match payload {
        Value::Array(items) => {
            if items.is_empty() {
                return (
                    StatusCode::OK,
                    Json(rpc_error(Value::Null, ERR_INVALID_REQUEST, "Invalid Request")),
                )
                    .into_response();
            }
            let mut responses = Vec::new();
            for item in items {
                if let RpcOutcome::Response(resp) = dispatch_rpc(&state, item).await {
                    responses.push(resp);
                }
            }
            if responses.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                (StatusCode::OK, Json(Value::Array(responses))).into_response()
            }
        }
        single => match dispatch_rpc(&state, single).await {
            RpcOutcome::Notification => StatusCode::ACCEPTED.into_response(),
            RpcOutcome::Response(resp) => (StatusCode::OK, Json(resp)).into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::header;
    use gateterm_protocol::AgentState;
    use serde_json::{json, Value};

    use super::*;
    use crate::agent::store::{PendingRequest, Update};
    use crate::launcher::{Launcher, LauncherProvider};
    use crate::mcp::SUPPORTED_PROTOCOL_VERSIONS;
    use crate::session::pty::test_support;

    fn test_state(enabled: bool) -> AppState {
        struct StaticLauncher;
        impl LauncherProvider for StaticLauncher {
            fn id(&self) -> &'static str {
                "static"
            }
            fn discover(&self) -> anyhow::Result<Vec<Launcher>> {
                Ok(vec![
                    Launcher::new("static", "echo", "Static Shell", vec!["sh".into()])
                        .with_cwd(Some("/tmp".into())),
                ])
            }
        }

        AppState {
            manager: crate::session::manager::SessionManager::new(1 << 20, Default::default()),
            token: "secret".into(),
            launchers: Arc::new(vec![Box::new(StaticLauncher)]),
            listen_port: 0,
            agents: Default::default(),
            agent_sessions: Arc::new(crate::agent::store::AgentSessionStore::new()),
            cloud: Default::default(),
            web_title: Default::default(),
            mcp: crate::config::McpConfig {
                enabled,
                ..Default::default()
            },
        }
    }

    fn auth_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        h
    }

    async fn resp_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn initialize_negotiates_supported_version_and_falls_back_for_unknown() {
        let state = test_state(true);

        for requested in ["2025-11-25", "2025-06-18", "2025-03-26"] {
            assert!(SUPPORTED_PROTOCOL_VERSIONS.contains(&requested));
            let outcome = dispatch_rpc(
                &state,
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": requested,
                        "capabilities": {},
                        "clientInfo": { "name": "test-client", "version": "1.0" }
                    }
                }),
            )
            .await;

            let RpcOutcome::Response(body) = outcome else {
                panic!("expected response for initialize");
            };
            assert_eq!(body["jsonrpc"], "2.0");
            assert_eq!(body["id"], 1);
            let result = &body["result"];
            assert_eq!(result["protocolVersion"], requested);
            assert_eq!(result["capabilities"]["tools"], json!({}));
            assert_eq!(result["serverInfo"]["name"], "gateterm");
            assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
            let instructions = result["instructions"].as_str().unwrap_or("");
            assert!(!instructions.is_empty());
            assert!(instructions.contains("list_sessions"));
            assert!(instructions.contains("list_pending_approvals"));
            assert!(instructions.contains("answer_approval"));
            assert!(instructions.contains("send_input"));
            assert!(instructions.contains("wait_ms"));
            assert!(instructions.contains("kill_session"));
        }

        // Unknown future version -> fallback "2025-06-18"
        let RpcOutcome::Response(unknown) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": "init-2",
                "method": "initialize",
                "params": { "protocolVersion": "2099-01-01" }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(unknown["id"], "init-2");
        assert_eq!(unknown["result"]["protocolVersion"], "2025-06-18");

        // Omitted params -> fallback "2025-06-18"
        let RpcOutcome::Response(omitted) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "initialize"
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(omitted["result"]["protocolVersion"], "2025-06-18");
    }

    #[tokio::test]
    async fn tools_list_exposes_exact_eight_tools_with_schemas_and_annotations() {
        let state = test_state(true);
        let RpcOutcome::Response(resp) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/list"
            }),
        )
        .await
        else {
            panic!("expected tools/list response");
        };

        let tools = resp["result"]["tools"]
            .as_array()
            .expect("tools must be an array");
        assert_eq!(tools.len(), 8);
        assert_eq!(tools, &tool_definitions());

        let expected_names = [
            "list_sessions",
            "list_launchers",
            "new_session",
            "read_session",
            "send_input",
            "kill_session",
            "list_pending_approvals",
            "answer_approval",
        ];
        let actual_names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("tool name"))
            .collect();
        assert_eq!(actual_names, expected_names);

        let find = |name: &str| -> &Value {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("missing tool {name}"))
        };

        for tool in tools {
            assert!(!tool["description"].as_str().unwrap_or("").is_empty());
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["annotations"]["openWorldHint"], json!(false));
        }

        for ro in [
            "list_sessions",
            "list_launchers",
            "read_session",
            "list_pending_approvals",
        ] {
            let ann = &find(ro)["annotations"];
            assert_eq!(ann["readOnlyHint"], json!(true), "{ro} readOnlyHint");
            assert_eq!(ann["idempotentHint"], json!(true), "{ro} idempotentHint");
        }

        for mut_non_destr in ["new_session", "send_input", "answer_approval"] {
            let ann = &find(mut_non_destr)["annotations"];
            assert_eq!(
                ann["readOnlyHint"],
                json!(false),
                "{mut_non_destr} readOnlyHint"
            );
            assert_eq!(
                ann["destructiveHint"],
                json!(false),
                "{mut_non_destr} destructiveHint"
            );
        }

        let kill_ann = &find("kill_session")["annotations"];
        assert_eq!(kill_ann["readOnlyHint"], json!(false));
        assert_eq!(kill_ann["destructiveHint"], json!(true));

        let req_of = |name: &str| -> Vec<String> {
            find(name)["inputSchema"]
                .get("required")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|v| v.as_str().unwrap().to_string())
                        .collect()
                })
                .unwrap_or_default()
        };
        assert!(req_of("list_sessions").is_empty());
        assert!(req_of("list_launchers").is_empty());
        assert!(req_of("new_session").is_empty());
        assert!(req_of("list_pending_approvals").is_empty());
        assert_eq!(req_of("read_session"), vec!["session_id"]);
        assert_eq!(req_of("send_input"), vec!["session_id"]);
        assert_eq!(req_of("kill_session"), vec!["session_id"]);
        assert_eq!(req_of("answer_approval"), vec!["pending_id", "decision"]);
    }

    #[tokio::test]
    async fn notifications_return_202_and_empty_or_mixed_batches_behave_per_jsonrpc2() {
        let state = test_state(true);

        // Single notifications -> 202 Accepted with empty body
        for method in ["notifications/initialized", "notifications/cancelled"] {
            let payload = serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "method": method
            }))
            .unwrap();
            let resp = handle_post(State(state.clone()), auth_headers(), Bytes::from(payload)).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
            assert!(body.is_empty(), "{method} must return empty 202 body");
        }

        // All-notification batch -> 202 Accepted empty body
        let all_notif = serde_json::to_vec(&json!([
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 1 } }
        ]))
        .unwrap();
        let resp = handle_post(State(state.clone()), auth_headers(), Bytes::from(all_notif)).await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert!(body.is_empty());

        // Empty batch [] -> 200 with -32600 Invalid Request
        let empty_batch =
            handle_post(State(state.clone()), auth_headers(), Bytes::from_static(b"[]")).await;
        assert_eq!(empty_batch.status(), StatusCode::OK);
        let err = resp_json(empty_batch).await;
        assert_eq!(err["jsonrpc"], "2.0");
        assert_eq!(err["id"], Value::Null);
        assert_eq!(err["error"]["code"], -32600);

        // Mixed batch [notification, ping request] -> 200 with length-1 array
        let mixed = serde_json::to_vec(&json!([
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "id": 1, "method": "ping" }
        ]))
        .unwrap();
        let mixed_resp = handle_post(State(state.clone()), auth_headers(), Bytes::from(mixed)).await;
        assert_eq!(mixed_resp.status(), StatusCode::OK);
        let arr = resp_json(mixed_resp).await;
        let items = arr.as_array().expect("batch response must be array");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0],
            json!({ "jsonrpc": "2.0", "id": 1, "result": {} })
        );

        // Batch containing invalid item + notification -> returns only invalid item error
        let mixed_invalid = handle_post(
            State(state.clone()),
            auth_headers(),
            Bytes::from_static(br#"[{"jsonrpc":"2.0","method":"notifications/initialized"},42]"#),
        )
        .await;
        assert_eq!(mixed_invalid.status(), StatusCode::OK);
        let invalid_arr = resp_json(mixed_invalid).await;
        let invalid_items = invalid_arr.as_array().unwrap();
        assert_eq!(invalid_items.len(), 1);
        assert_eq!(invalid_items[0]["error"]["code"], -32600);
        assert_eq!(invalid_items[0]["id"], Value::Null);

        // Invalid JSON -> 200 with -32700 Parse error
        let parse_err = handle_post(
            State(state.clone()),
            auth_headers(),
            Bytes::from_static(b"{not-valid-json"),
        )
        .await;
        assert_eq!(parse_err.status(), StatusCode::OK);
        let err = resp_json(parse_err).await;
        assert_eq!(err["id"], Value::Null);
        assert_eq!(err["error"]["code"], -32700);

        // Wrong jsonrpc version -> 200 with -32600 echoing id
        let bad_ver = handle_post(
            State(state.clone()),
            auth_headers(),
            Bytes::from_static(br#"{"jsonrpc":"1.0","id":"v1","method":"ping"}"#),
        )
        .await;
        assert_eq!(bad_ver.status(), StatusCode::OK);
        let err = resp_json(bad_ver).await;
        assert_eq!(err["id"], "v1");
        assert_eq!(err["error"]["code"], -32600);

        // Explicit id: null is a Request, not a notification
        let null_id = handle_post(
            State(state.clone()),
            auth_headers(),
            Bytes::from_static(br#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#),
        )
        .await;
        assert_eq!(null_id.status(), StatusCode::OK);
        assert_eq!(
            resp_json(null_id).await,
            json!({ "jsonrpc": "2.0", "id": null, "result": {} })
        );

        // Unknown method -> -32601
        let RpcOutcome::Response(unknown_method) = dispatch_rpc(
            &state,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(unknown_method["id"], 2);
        assert_eq!(unknown_method["error"]["code"], -32601);

        // Unknown tool -> -32602
        let RpcOutcome::Response(unknown_tool) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "no_such_tool", "arguments": {} }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(unknown_tool["error"]["code"], -32602);

        // Missing session_id -> -32602
        let RpcOutcome::Response(missing_sid) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "read_session", "arguments": {} }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(missing_sid["error"]["code"], -32602);

        // Wrong type session_id -> -32602
        let RpcOutcome::Response(bad_sid_type) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": { "name": "read_session", "arguments": { "session_id": "abc" } }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(bad_sid_type["error"]["code"], -32602);

        // Missing decision on answer_approval -> -32602
        let RpcOutcome::Response(missing_dec) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": { "name": "answer_approval", "arguments": { "pending_id": "p1" } }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(missing_dec["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn read_session_formats_compact_text_and_structured_content() {
        let state = test_state(true);
        #[cfg(unix)]
        let cmd: Vec<String> = vec!["/bin/sh".into(), "-c".into(), "sleep 5".into()];
        #[cfg(windows)]
        let cmd: Vec<String> = vec!["cmd.exe".into(), "/C".into(), "ping -n 6 127.0.0.1 >nul".into()];
        let session = state
            .manager
            .create(Some("mcp-read".into()), Some(cmd), None)
            .expect("spawn test session");
        let sid = session.id;
        test_support::output(&session, b"hello mcp\r\n$ ");

        let RpcOutcome::Response(resp) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "tools/call",
                "params": {
                    "name": "read_session",
                    "arguments": { "session_id": sid, "scrollback_lines": 0 }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };

        assert!(resp.get("error").is_none(), "unexpected rpc error: {resp}");
        let result = &resp["result"];
        assert_ne!(result.get("isError"), Some(&json!(true)));
        let text = result["content"][0]["text"]
            .as_str()
            .expect("content[0].text");
        assert!(text.contains(&format!("#{sid}")), "missing session id in {text:?}");
        assert!(text.contains("hello mcp"), "missing screen text in {text:?}");

        let sc = &result["structuredContent"];
        assert_eq!(sc["session_id"], sid);
        assert_eq!(sc["cols"], 80);
        assert_eq!(sc["rows"], 24);
        assert!(sc["seq"].as_u64().unwrap() > 0);
        assert_eq!(sc["exited"], false);
        assert!(sc["text"].as_str().unwrap().contains("hello mcp"));

        // Kill and verify missing session returns CallToolResult { isError: true }
        let RpcOutcome::Response(kill_resp) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "tools/call",
                "params": {
                    "name": "kill_session",
                    "arguments": { "session_id": sid }
                }
            }),
        )
        .await
        else {
            panic!("expected kill response");
        };
        assert_eq!(
            kill_resp["result"]["structuredContent"],
            json!({ "killed": true, "id": sid })
        );

        let RpcOutcome::Response(missing) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 13,
                "method": "tools/call",
                "params": {
                    "name": "read_session",
                    "arguments": { "session_id": sid }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert!(missing.get("error").is_none());
        assert_eq!(missing["result"]["isError"], json!(true));
        let err_text = missing["result"]["content"][0]["text"].as_str().unwrap();
        assert!(err_text.starts_with("Error: "), "got {err_text:?}");
    }

    #[tokio::test]
    async fn all_eight_tools_dispatch_to_service_and_map_domain_errors_to_is_error() {
        let state = test_state(true);
        #[cfg(unix)]
        let cmd: Vec<String> = vec!["/bin/sh".into(), "-c".into(), "sleep 5".into()];
        #[cfg(windows)]
        let cmd: Vec<String> = vec!["cmd.exe".into(), "/C".into(), "ping -n 6 127.0.0.1 >nul".into()];

        // new_session (success)
        let RpcOutcome::Response(created) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "new_session",
                    "arguments": { "name": "worker", "command": cmd }
                }
            }),
        )
        .await
        else {
            panic!("expected new_session response");
        };
        let sid = created["result"]["structuredContent"]["session"]["id"]
            .as_u64()
            .expect("created session id");

        // new_session domain error (empty command) -> isError: true
        let RpcOutcome::Response(bad_new) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "new_session",
                    "arguments": { "command": [] }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(bad_new["result"]["isError"], json!(true));

        // Attach waiting agent + pending approval
        let _ = state.agent_sessions.apply(
            "claude-code",
            "ag-1",
            Some(sid),
            Update {
                state: Some(AgentState::Waiting),
                detail: Some("Bash".into()),
                ..Default::default()
            },
        );
        let _rx = state.agent_sessions.insert_pending(PendingRequest {
            id: "pend-1".into(),
            agent_id: "claude-code".into(),
            agent_session_id: "ag-1".into(),
            session: Some(sid),
            tool_name: Some("Bash".into()),
            tool_input: json!({ "command": "ls" }),
            created_at: 1,
            expires_at: 9999,
        });

        // list_sessions
        let RpcOutcome::Response(listed) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "list_sessions" }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(
            listed["result"]["structuredContent"]["sessions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(listed["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("claude-code"));

        // list_launchers
        let RpcOutcome::Response(launchers) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "list_launchers" }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(
            launchers["result"]["structuredContent"]["launchers"][0]["id"],
            "static:echo"
        );

        // send_input (null optional fields, wait_ms = 0 -> no screen)
        let RpcOutcome::Response(sent_fast) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "send_input",
                    "arguments": {
                        "session_id": sid,
                        "text": "echo hi",
                        "keys": null,
                        "enter": true,
                        "wait_ms": null
                    }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(sent_fast["result"]["structuredContent"]["ok"], json!(true));
        assert!(sent_fast["result"]["structuredContent"].get("screen").is_none());

        // send_input with wait_ms > 0 -> screen included
        let session_arc = state.manager.get(sid).unwrap();
        let emit_handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            test_support::output(&session_arc, b"output-line\r\n");
        });
        let RpcOutcome::Response(sent_wait) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "send_input",
                    "arguments": { "session_id": sid, "keys": ["Enter"], "wait_ms": 1500 }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        emit_handle.await.unwrap();
        assert!(sent_wait["result"]["structuredContent"]["screen"]["text"]
            .as_str()
            .unwrap()
            .contains("output-line"));
        assert!(sent_wait["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("output-line"));

        // send_input domain error (unknown key / empty input) -> isError: true
        let RpcOutcome::Response(bad_key) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": {
                    "name": "send_input",
                    "arguments": { "session_id": sid, "keys": ["NotAKey"] }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(bad_key["result"]["isError"], json!(true));

        let RpcOutcome::Response(empty_input) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 70,
                "method": "tools/call",
                "params": {
                    "name": "send_input",
                    "arguments": { "session_id": sid }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(empty_input["result"]["isError"], json!(true));

        // list_pending_approvals + answer_approval
        let RpcOutcome::Response(pending) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {
                    "name": "list_pending_approvals",
                    "arguments": { "session_id": sid }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(
            pending["result"]["structuredContent"]["pending"][0]["id"],
            "pend-1"
        );

        let RpcOutcome::Response(bad_updated_input) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 81,
                "method": "tools/call",
                "params": {
                    "name": "answer_approval",
                    "arguments": {
                        "pending_id": "pend-1",
                        "decision": "allow",
                        "updated_input": "not-an-object"
                    }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(bad_updated_input["error"]["code"], -32602);

        let RpcOutcome::Response(bad_decision) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 82,
                "method": "tools/call",
                "params": {
                    "name": "answer_approval",
                    "arguments": { "pending_id": "pend-1", "decision": "maybe" }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(bad_decision["result"]["isError"], json!(true));

        let RpcOutcome::Response(answered) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": {
                    "name": "answer_approval",
                    "arguments": { "pending_id": "pend-1", "decision": "allow" }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(
            answered["result"]["structuredContent"],
            json!({ "ok": true, "pending_id": "pend-1", "decision": "allow" })
        );

        // Answering again -> 409 Conflict mapped to isError: true
        let RpcOutcome::Response(dup_answer) = dispatch_rpc(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": {
                    "name": "answer_approval",
                    "arguments": { "pending_id": "pend-1", "decision": "deny" }
                }
            }),
        )
        .await
        else {
            panic!("expected response");
        };
        assert_eq!(dup_answer["result"]["isError"], json!(true));

        let _ = state.manager.kill(sid);
    }

    #[tokio::test]
    async fn mcp_http_route_enforces_pre_guards_method_not_allowed_and_body_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = test_state(true);
        let server = tokio::spawn(async move {
            let _ = crate::app::serve(listener, state).await;
        });

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/mcp");

        // 1. Missing bearer -> 401 (checked BEFORE Origin or enabled)
        for method in [
            reqwest::Method::POST,
            reqwest::Method::GET,
            reqwest::Method::DELETE,
        ] {
            let res = client
                .request(method.clone(), &url)
                .header("Origin", "http://evil.example")
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 401, "{method} /mcp without auth must be 401");
        }

        // 2. Valid bearer + bad Origin -> 403
        for method in [
            reqwest::Method::POST,
            reqwest::Method::GET,
            reqwest::Method::DELETE,
        ] {
            let res = client
                .request(method.clone(), &url)
                .bearer_auth("secret")
                .header("Origin", "http://evil.example")
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 403, "{method} /mcp bad Origin must be 403");
        }

        // 3. Valid bearer + same-origin + GET/DELETE -> 405
        for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
            let res = client
                .request(method.clone(), &url)
                .bearer_auth("secret")
                .header("Origin", format!("http://{addr}"))
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 405, "{method} /mcp must be 405");
        }

        // 4. Valid POST initialize & notification
        let init = client
            .post(&url)
            .bearer_auth("secret")
            .header("Origin", format!("http://{addr}"))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2025-11-25" }
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(init.status(), 200);
        let init_json: Value = init.json().await.unwrap();
        assert_eq!(init_json["result"]["protocolVersion"], "2025-11-25");

        let notif = client
            .post(&url)
            .bearer_auth("secret")
            .json(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .send()
            .await
            .unwrap();
        assert_eq!(notif.status(), 202);
        assert!(notif.bytes().await.unwrap().is_empty());

        // 5. Payload > 512 KiB -> 413
        let oversized = vec![b' '; 513 * 1024];
        let big = client
            .post(&url)
            .bearer_auth("secret")
            .body(oversized)
            .send()
            .await
            .unwrap();
        assert_eq!(big.status(), 413);

        server.abort();

        // 6. Disabled [mcp].enabled = false -> 403 after bearer + origin
        let disabled_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let disabled_addr = disabled_listener.local_addr().unwrap();
        let disabled_state = test_state(false);
        let disabled_server = tokio::spawn(async move {
            let _ = crate::app::serve(disabled_listener, disabled_state).await;
        });

        // Unauth even when disabled must still be 401
        let unauth_disabled = client
            .post(format!("http://{disabled_addr}/mcp"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth_disabled.status(), 401);

        for method in [
            reqwest::Method::POST,
            reqwest::Method::GET,
            reqwest::Method::DELETE,
        ] {
            let res = client
                .request(method.clone(), format!("http://{disabled_addr}/mcp"))
                .bearer_auth("secret")
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 403, "{method} /mcp when disabled must be 403");
        }

        disabled_server.abort();
    }
}
