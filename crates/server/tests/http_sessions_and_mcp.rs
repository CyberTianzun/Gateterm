//! End-to-end integration tests for REST `/api/sessions/*`, `/api/mcp/status`,
//! `/api/agents/pending?session_id=`, and the stateless Streamable HTTP `/mcp`
//! JSON-RPC 2.0 endpoint.

mod common;

use common::{
    connect, control, create_session, input_frame, parse_server, start_test_server_with_mcp, Ws,
};
use futures_util::{SinkExt, StreamExt};
use gateterm_protocol::{AttachMode, ClientMsg, ServerMsg};
use gateterm_server::agent::store::hook_token;
use gateterm_server::config::McpConfig;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

fn short_exit_cmd() -> Vec<String> {
    #[cfg(unix)]
    return vec![
        "/bin/sh".into(),
        "-c".into(),
        "echo SHORT_EXIT_MARKER; sleep 0.6".into(),
    ];
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        vec![
            format!(r"{root}\System32\cmd.exe"),
            "/C".into(),
            "echo SHORT_EXIT_MARKER & ping -n 2 127.0.0.1 >nul".into(),
        ]
    }
}

fn long_shell_cmd() -> Vec<String> {
    #[cfg(unix)]
    return vec!["/bin/sh".into(), "-i".into()];
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        vec![format!(r"{root}\System32\cmd.exe")]
    }
}

fn osc52_echo_cmd() -> Option<Vec<String>> {
    #[cfg(unix)]
    return Some(vec![
        "/bin/sh".into(),
        "-c".into(),
        r#"while IFS= read -r b; do printf 'BEGIN_%s\033]52;c;%s\007END_%s\n' "$b" "$b" "$b"; done"#
            .into(),
    ]);
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        Some(vec![
            format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"),
            "-NoProfile".into(),
            "-NoLogo".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            "while (($b = [Console]::In.ReadLine()) -ne $null) { \
             [Console]::Out.Write('BEGIN_' + $b + [char]27 + ']52;c;' + $b + [char]7 + 'END_' + $b + \"`n\") }"
                .into(),
        ])
    }
}

async fn post_mcp(
    client: &reqwest::Client,
    addr: SocketAddr,
    token: &str,
    payload: Value,
) -> reqwest::Response {
    client
        .post(format!("http://{addr}/mcp"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .unwrap()
}

async fn mcp_rpc(
    client: &reqwest::Client,
    addr: SocketAddr,
    token: &str,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let resp = post_mcp(
        client,
        addr,
        token,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await;
    assert_eq!(resp.status(), 200, "expected 200 for {method}");
    let body: Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_none(),
        "unexpected JSON-RPC error for {method}: {body}"
    );
    body["result"].clone()
}

async fn mcp_tool(
    client: &reqwest::Client,
    addr: SocketAddr,
    token: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    mcp_rpc(
        client,
        addr,
        token,
        id,
        "tools/call",
        json!({
            "name": name,
            "arguments": arguments,
        }),
    )
    .await
}

async fn wait_for_prompt(
    client: &reqwest::Client,
    addr: SocketAddr,
    token: &str,
    session_id: u64,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let resp = client
            .get(format!("http://{addr}/api/sessions/{session_id}/screen"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        if resp.status() == 200 {
            let snap: Value = resp.json().await.unwrap();
            let text = snap["text"].as_str().unwrap_or("");
            if !text.trim().is_empty() {
                tokio::time::sleep(Duration::from_millis(150)).await;
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for shell prompt in session {session_id}");
}

async fn attach_and_expect_initial_resize(
    ws: &mut Ws,
    id: u64,
    channel: u32,
    cols: u16,
    rows: u16,
) {
    ws.send(control(&ClientMsg::Attach {
        id,
        channel,
        last_seq: None,
        cols,
        rows,
        colors: None,
    }))
    .await
    .unwrap();

    let mut saw_attached = false;
    let mut saw_resized = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("ws attach frame timeout")
            .unwrap()
            .unwrap();
        match parse_server(msg) {
            Some((0, Ok(ServerMsg::Attached { channel: ch, mode: AttachMode::Resync, .. })))
                if ch == channel =>
            {
                saw_attached = true;
            }
            Some((0, Ok(ServerMsg::Resized { channel: ch, cols: c, rows: r }))) if ch == channel => {
                assert_eq!((c, r), (cols, rows));
                saw_resized = true;
            }
            Some((0, Ok(ServerMsg::ResyncEnd { channel: ch, .. }))) if ch == channel => {
                assert!(saw_attached, "expected Attached before ResyncEnd");
                assert!(saw_resized, "expected initial Resized before ResyncEnd");
                return;
            }
            _ => {}
        }
    }
    panic!("timed out waiting for attach resync on channel {channel}");
}

async fn collect_ws_until(
    ws: &mut Ws,
    channel: u32,
    needle: &str,
) -> (String, Vec<(u32, String)>, Vec<(u32, u16, u16)>) {
    let mut bytes_acc = Vec::new();
    let mut clips = Vec::new();
    let mut resizes = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    while tokio::time::Instant::now() < deadline {
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(250), ws.next()).await
        else {
            continue;
        };
        match parse_server(msg) {
            Some((0, Ok(ServerMsg::Clipboard { channel: ch, text }))) => clips.push((ch, text)),
            Some((0, Ok(ServerMsg::Resized { channel: ch, cols, rows }))) => {
                resizes.push((ch, cols, rows));
            }
            Some((ch, Err(data))) if ch == channel => {
                bytes_acc.extend_from_slice(&data);
                if String::from_utf8_lossy(&bytes_acc).contains(needle) {
                    while let Ok(Some(Ok(extra))) =
                        tokio::time::timeout(Duration::from_millis(100), ws.next()).await
                    {
                        match parse_server(extra) {
                            Some((0, Ok(ServerMsg::Clipboard { channel: ch, text }))) => {
                                clips.push((ch, text));
                            }
                            Some((0, Ok(ServerMsg::Resized { channel: ch, cols, rows }))) => {
                                resizes.push((ch, cols, rows));
                            }
                            Some((ch, Err(more))) if ch == channel => {
                                bytes_acc.extend_from_slice(&more);
                            }
                            _ => {}
                        }
                    }
                    return (
                        String::from_utf8_lossy(&bytes_acc).into_owned(),
                        clips,
                        resizes,
                    );
                }
            }
            _ => {}
        }
    }
    panic!(
        "timed out waiting for {needle:?} on channel {channel}; got {:?}",
        String::from_utf8_lossy(&bytes_acc)
    );
}

#[tokio::test]
async fn auth_origin_and_mcp_enable_gate_are_enforced() {
    let token = "test-secret-token";
    let enabled_addr = start_test_server_with_mcp(token, McpConfig { enabled: true, ..Default::default() }).await;
    let disabled_addr = start_test_server_with_mcp(token, McpConfig { enabled: false, ..Default::default() }).await;
    let client = reqwest::Client::new();

    // 1. Missing, wrong, and ?token= query tokens must all return 401 across REST and /mcp.
    let routes = [
        (reqwest::Method::GET, "/api/mcp/status"),
        (reqwest::Method::GET, "/api/sessions"),
        (reqwest::Method::POST, "/api/sessions"),
        (reqwest::Method::GET, "/api/sessions/1/screen"),
        (reqwest::Method::POST, "/api/sessions/1/input"),
        (reqwest::Method::DELETE, "/api/sessions/1"),
        (reqwest::Method::GET, "/api/agents/pending?session_id=1"),
        (reqwest::Method::POST, "/mcp"),
        (reqwest::Method::GET, "/mcp"),
        (reqwest::Method::DELETE, "/mcp"),
    ];

    for (method, path) in &routes {
        let missing = client
            .request(method.clone(), format!("http://{enabled_addr}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), 401, "{method} {path} without token");

        let wrong = client
            .request(method.clone(), format!("http://{enabled_addr}{path}"))
            .bearer_auth("wrong-token")
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), 401, "{method} {path} with wrong token");

        let sep = if path.contains('?') { '&' } else { '?' };
        let query_tok = client
            .request(
                method.clone(),
                format!("http://{enabled_addr}{path}{sep}token={token}"),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(query_tok.status(), 401, "{method} {path} with ?token= query");
    }

    // 2. /api/mcp/status returns enabled state with Cache-Control: no-store.
    let status_enabled = client
        .get(format!("http://{enabled_addr}/api/mcp/status"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(status_enabled.status(), 200);
    assert_eq!(
        status_enabled
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let status_json: Value = status_enabled.json().await.unwrap();
    assert_eq!(status_json["enabled"], true);
    assert_eq!(status_json["endpoint"], "/mcp");
    assert_eq!(
        status_json["protocol_versions"],
        json!(["2025-11-25", "2025-06-18", "2025-03-26"])
    );

    // 3. Cross-site / spoofed / null Origin headers are rejected with 403 on /mcp.
    let bad_origins = [
        "null",
        "http://evil.example",
        &format!("http://127.0.0.1.evil.com:{}", enabled_addr.port()),
        "http://127.0.0.1:9999",
    ];
    for origin in bad_origins {
        for method in [reqwest::Method::POST, reqwest::Method::GET, reqwest::Method::DELETE] {
            let res = client
                .request(method.clone(), format!("http://{enabled_addr}/mcp"))
                .bearer_auth(token)
                .header("Origin", origin)
                .header("X-Forwarded-Host", "evil.example")
                .send()
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                403,
                "{method} /mcp with Origin={origin:?} must return 403"
            );
        }
    }

    // 4. Same-origin or absent Origin is allowed; GET/DELETE /mcp return 405 with Allow: POST.
    let ping_body = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    let same_origin_post = client
        .post(format!("http://{enabled_addr}/mcp"))
        .bearer_auth(token)
        .header("Origin", format!("http://{enabled_addr}"))
        .json(&ping_body)
        .send()
        .await
        .unwrap();
    assert_eq!(same_origin_post.status(), 200);
    assert_eq!(same_origin_post.json::<Value>().await.unwrap()["result"], json!({}));

    let no_origin_post = client
        .post(format!("http://{enabled_addr}/mcp"))
        .bearer_auth(token)
        .json(&ping_body)
        .send()
        .await
        .unwrap();
    assert_eq!(no_origin_post.status(), 200);

    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        for origin in [None, Some(format!("http://{enabled_addr}"))] {
            let mut req = client
                .request(method.clone(), format!("http://{enabled_addr}/mcp"))
                .bearer_auth(token);
            if let Some(o) = origin {
                req = req.header("Origin", o);
            }
            let res = req.send().await.unwrap();
            assert_eq!(res.status(), 405, "{method} /mcp when enabled must be 405");
            assert_eq!(
                res.headers().get("allow").and_then(|v| v.to_str().ok()),
                Some("POST"),
                "{method} /mcp 405 must carry Allow: POST"
            );
        }
    }

    // 5. [mcp].enabled = false gates /mcp to 403 while /api/mcp/status reports enabled: false.
    let status_disabled = client
        .get(format!("http://{disabled_addr}/api/mcp/status"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(status_disabled.status(), 200);
    assert_eq!(status_disabled.json::<Value>().await.unwrap()["enabled"], false);

    let unauth_disabled = client
        .post(format!("http://{disabled_addr}/mcp"))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth_disabled.status(), 401);

    for method in [reqwest::Method::POST, reqwest::Method::GET, reqwest::Method::DELETE] {
        let res = client
            .request(method.clone(), format!("http://{disabled_addr}/mcp"))
            .bearer_auth(token)
            .header("Origin", format!("http://{disabled_addr}"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            403,
            "{method} /mcp when [mcp].enabled=false must return 403"
        );
    }
}

#[tokio::test]
async fn mcp_and_rest_full_session_lifecycle_and_exited_session_error() {
    let token = "tok-lifecycle";
    let addr = start_test_server_with_mcp(token, McpConfig { enabled: true, ..Default::default() }).await;
    let client = reqwest::Client::new();

    // 1. initialize + notifications/initialized + tools/list + list_launchers
    let init = mcp_rpc(
        &client,
        addr,
        token,
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "e2e-test", "version": "1.0" }
        }),
    )
    .await;
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(init["serverInfo"]["name"], "gateterm");
    assert_eq!(init["capabilities"]["tools"], json!({}));
    assert!(!init["instructions"].as_str().unwrap_or("").is_empty());

    let notif = post_mcp(
        &client,
        addr,
        token,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(notif.status(), 202);
    assert!(notif.bytes().await.unwrap().is_empty());

    let tools_res = mcp_rpc(&client, addr, token, 2, "tools/list", json!({})).await;
    let tools = tools_res["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec![
            "list_sessions",
            "list_launchers",
            "new_session",
            "read_session",
            "send_input",
            "kill_session",
            "list_pending_approvals",
            "answer_approval",
        ]
    );

    let launchers_res = mcp_tool(&client, addr, token, 3, "list_launchers", json!({})).await;
    let launchers = launchers_res["structuredContent"]["launchers"]
        .as_array()
        .unwrap();
    assert!(!launchers.is_empty());
    assert_eq!(launchers[0]["provider"], "builtin");
    let first_launcher_id = launchers[0]["id"].as_str().unwrap().to_string();

    // Verify new_session via launcher_id creates a session and kill_session removes it.
    let from_launcher = mcp_tool(
        &client,
        addr,
        token,
        4,
        "new_session",
        json!({ "launcher_id": first_launcher_id, "name": "launcher-check" }),
    )
    .await;
    let launcher_sid = from_launcher["structuredContent"]["session"]["id"]
        .as_u64()
        .unwrap();
    let killed_launcher = mcp_tool(
        &client,
        addr,
        token,
        5,
        "kill_session",
        json!({ "session_id": launcher_sid }),
    )
    .await;
    assert_eq!(
        killed_launcher["structuredContent"],
        json!({ "killed": true, "id": launcher_sid })
    );

    // 2. Short-lived explicit command: verify output, wait for natural exit, then verify
    //    send_input(wait_ms: 5000) returns immediately (< 500ms) with isError / 409|404.
    let short_created = mcp_tool(
        &client,
        addr,
        token,
        6,
        "new_session",
        json!({
            "name": "short-exit",
            "command": short_exit_cmd(),
        }),
    )
    .await;
    let short_id = short_created["structuredContent"]["session"]["id"]
        .as_u64()
        .unwrap();

    let mut saw_short_output = false;
    let mut saw_exited = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let screen_resp = client
            .get(format!("http://{addr}/api/sessions/{short_id}/screen"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        match screen_resp.status().as_u16() {
            200 => {
                let snap: Value = screen_resp.json().await.unwrap();
                if snap["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("SHORT_EXIT_MARKER")
                {
                    saw_short_output = true;
                }
                if snap["exited"] == true {
                    saw_exited = true;
                    break;
                }
            }
            404 => {
                saw_exited = true;
                break;
            }
            other => panic!("unexpected screen status for short session: {other}"),
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(saw_short_output, "short command never printed SHORT_EXIT_MARKER");
    assert!(saw_exited, "short command never exited");

    let t_mcp = Instant::now();
    let mcp_exited_input = mcp_tool(
        &client,
        addr,
        token,
        7,
        "send_input",
        json!({
            "session_id": short_id,
            "text": "after-exit",
            "enter": true,
            "wait_ms": 5000,
        }),
    )
    .await;
    assert!(
        t_mcp.elapsed() < Duration::from_millis(500),
        "MCP send_input on exited session must not block (took {:?})",
        t_mcp.elapsed()
    );
    assert_eq!(mcp_exited_input["isError"], json!(true));

    let t_rest = Instant::now();
    let rest_exited_input = client
        .post(format!("http://{addr}/api/sessions/{short_id}/input"))
        .bearer_auth(token)
        .json(&json!({
            "text": "after-exit",
            "enter": true,
            "wait_ms": 5000,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        t_rest.elapsed() < Duration::from_millis(500),
        "REST send_input on exited session must not block (took {:?})",
        t_rest.elapsed()
    );
    assert!(
        matches!(rest_exited_input.status().as_u16(), 409 | 404),
        "REST send_input on exited session must return 409 (pre-reap) or 404 (post-reap), got {}",
        rest_exited_input.status()
    );

    // 3. Long-lived interactive shell: send_input(wait_ms), scrollback read_session, kill_session -> 404/isError.
    let created = mcp_tool(
        &client,
        addr,
        token,
        8,
        "new_session",
        json!({
            "name": "long-shell",
            "command": long_shell_cmd(),
        }),
    )
    .await;
    let sid = created["structuredContent"]["session"]["id"]
        .as_u64()
        .unwrap();
    wait_for_prompt(&client, addr, token, sid).await;

    let echo_res = mcp_tool(
        &client,
        addr,
        token,
        9,
        "send_input",
        json!({
            "session_id": sid,
            "text": "echo MCP_LIFECYCLE_MARK",
            "enter": true,
            "wait_ms": 5000,
        }),
    )
    .await;
    assert_ne!(echo_res.get("isError"), Some(&json!(true)));
    let echo_screen_text = echo_res["structuredContent"]["screen"]["text"]
        .as_str()
        .unwrap();
    assert!(
        echo_screen_text.contains("MCP_LIFECYCLE_MARK"),
        "expected marker in send_input screen: {echo_screen_text:?}"
    );
    assert!(
        echo_res["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("MCP_LIFECYCLE_MARK")
    );

    // Emit 40 lines to push SL_1 into scrollback beyond the 24-row viewport.
    #[cfg(unix)]
    let scroll_cmd = "i=1; while [ $i -le 40 ]; do echo SL_$i; i=$((i+1)); done";
    #[cfg(windows)]
    let scroll_cmd = "for /L %i in (1,1,40) do @echo SL_%i";

    let scroll_send = mcp_tool(
        &client,
        addr,
        token,
        10,
        "send_input",
        json!({
            "session_id": sid,
            "text": scroll_cmd,
            "enter": true,
            "wait_ms": 5000,
        }),
    )
    .await;
    assert_ne!(scroll_send.get("isError"), Some(&json!(true)));

    // Ensure SL_40 is visible before asserting scrollback contents.
    let wait_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let visible = mcp_tool(
            &client,
            addr,
            token,
            11,
            "read_session",
            json!({ "session_id": sid, "scrollback_lines": 0 }),
        )
        .await;
        if visible["structuredContent"]["text"]
            .as_str()
            .unwrap_or("")
            .contains("SL_40")
        {
            assert!(
                !visible["structuredContent"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("SL_1\n"),
                "SL_1 should have scrolled off visible 24-row viewport"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < wait_deadline,
            "timed out waiting for SL_40 on screen"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let with_history = mcp_tool(
        &client,
        addr,
        token,
        12,
        "read_session",
        json!({ "session_id": sid, "scrollback_lines": 50 }),
    )
    .await;
    let hist_sc = &with_history["structuredContent"];
    assert!(hist_sc["history_available"].as_u64().unwrap() > 0);
    assert!(hist_sc["history_returned"].as_u64().unwrap() > 0);
    let hist_text = hist_sc["text"].as_str().unwrap();
    assert!(hist_text.contains("SL_1"), "scrollback missing SL_1: {hist_text:?}");
    assert!(hist_text.contains("SL_40"), "screen missing SL_40: {hist_text:?}");

    // Also verify REST GET /api/sessions/{id}/screen?scrollback_lines=50 matches.
    let rest_screen = client
        .get(format!(
            "http://{addr}/api/sessions/{sid}/screen?scrollback_lines=50"
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(rest_screen.status(), 200);
    let rest_snap: Value = rest_screen.json().await.unwrap();
    assert!(rest_snap["history_returned"].as_u64().unwrap() > 0);
    assert!(rest_snap["text"].as_str().unwrap().contains("SL_1"));

    // Verify sending exit with wait_ms returns promptly once session exits (before 5000ms).
    let t_exit = Instant::now();
    let exit_res = mcp_tool(
        &client,
        addr,
        token,
        13,
        "send_input",
        json!({
            "session_id": sid,
            "text": "exit",
            "enter": true,
            "wait_ms": 5000,
        }),
    )
    .await;
    assert!(
        t_exit.elapsed() < Duration::from_millis(2000),
        "send_input(exit) should stop waiting on Exited, took {:?}",
        t_exit.elapsed()
    );
    // Depending on exact timing, either Exited fired immediately (isError=false, screen.exited=true)
    // or the session was already exited (isError=true). Both are immediate; if still present, kill it.
    if exit_res.get("isError") != Some(&json!(true)) {
        let _ = client
            .delete(format!("http://{addr}/api/sessions/{sid}"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
    }

    // Also verify explicit kill_session + subsequent read_session returns 404 / isError: true.
    let create_rest = client
        .post(format!("http://{addr}/api/sessions"))
        .bearer_auth(token)
        .json(&json!({ "name": "kill-target", "command": long_shell_cmd() }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_rest.status(), 201);
    let kill_id = create_rest.json::<Value>().await.unwrap()["session"]["id"]
        .as_u64()
        .unwrap();

    let killed = mcp_tool(
        &client,
        addr,
        token,
        14,
        "kill_session",
        json!({ "session_id": kill_id }),
    )
    .await;
    assert_eq!(
        killed["structuredContent"],
        json!({ "killed": true, "id": kill_id })
    );

    let mcp_after_kill = mcp_tool(
        &client,
        addr,
        token,
        15,
        "read_session",
        json!({ "session_id": kill_id }),
    )
    .await;
    assert_eq!(mcp_after_kill["isError"], json!(true));

    let rest_after_kill = client
        .get(format!("http://{addr}/api/sessions/{kill_id}/screen"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(rest_after_kill.status(), 404);
}

#[tokio::test]
async fn session_list_surfaces_waiting_agent_and_approval_flow_unblocks_hook() {
    let token = "tok-approval";
    let addr = start_test_server_with_mcp(token, McpConfig { enabled: true, ..Default::default() }).await;
    let client = reqwest::Client::new();

    // 1. Create a real long-lived PTY session.
    let created = mcp_tool(
        &client,
        addr,
        token,
        1,
        "new_session",
        json!({
            "name": "agent-host",
            "command": long_shell_cmd(),
        }),
    )
    .await;
    let sid = created["structuredContent"]["session"]["id"]
        .as_u64()
        .unwrap();

    // 2. Spawn a blocking Claude Code permission-request hook attached to that session.
    let hook_client = client.clone();
    let hook_task = tokio::spawn(async move {
        hook_client
            .post(format!("http://{addr}/hook/claude-code/permission-request"))
            .header("Authorization", format!("Bearer {}", hook_token()))
            .header("X-Gateterm-Session", sid.to_string())
            .header("Content-Type", "application/json")
            .body(
                r#"{"session_id":"mcp-ag-1","tool_name":"Bash","tool_input":{"command":"git status"}}"#,
            )
            .send()
            .await
            .unwrap()
    });

    // 3. Poll GET /api/sessions until agent.state == "waiting" with non-empty pending_id.
    let mut pending_id = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let resp = client
            .get(format!("http://{addr}/api/sessions"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        if let Some(sess) = body["sessions"]
            .as_array()
            .and_then(|arr| arr.iter().find(|s| s["id"] == sid))
            && sess["agent"]["state"] == "waiting"
            && let Some(pid) = sess["agent"]["pending_id"].as_str()
            && !pid.is_empty()
        {
            assert_eq!(sess["agent"]["agent_id"], "claude-code");
            assert_eq!(sess["agent"]["detail"], "Bash");
            pending_id = pid.to_string();
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !pending_id.is_empty(),
        "timed out waiting for session {sid} to surface waiting agent"
    );

    // 4. MCP list_sessions surfaces the same waiting agent and pending_id.
    let mcp_sessions = mcp_tool(&client, addr, token, 2, "list_sessions", json!({})).await;
    let mcp_sess = mcp_sessions["structuredContent"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == sid)
        .expect("session missing from MCP list_sessions");
    assert_eq!(mcp_sess["agent"]["state"], "waiting");
    assert_eq!(mcp_sess["agent"]["pending_id"], pending_id);
    assert!(
        mcp_sessions["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(&pending_id)
    );

    // 5. list_pending_approvals filters accurately by session_id.
    let filtered = mcp_tool(
        &client,
        addr,
        token,
        3,
        "list_pending_approvals",
        json!({ "session_id": sid }),
    )
    .await;
    let items = filtered["structuredContent"]["pending"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], pending_id);
    assert_eq!(items[0]["session"], sid);
    assert_eq!(items[0]["tool_name"], "Bash");
    assert_eq!(items[0]["tool_input"]["command"], "git status");

    let empty_other = mcp_tool(
        &client,
        addr,
        token,
        4,
        "list_pending_approvals",
        json!({ "session_id": sid + 999 }),
    )
    .await;
    assert!(
        empty_other["structuredContent"]["pending"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // 6. MCP answer_approval(allow) unblocks the hook request with 200 allow output.
    let approved = mcp_tool(
        &client,
        addr,
        token,
        5,
        "answer_approval",
        json!({
            "pending_id": pending_id,
            "decision": "allow",
        }),
    )
    .await;
    assert_ne!(approved.get("isError"), Some(&json!(true)));
    assert_eq!(
        approved["structuredContent"],
        json!({ "ok": true, "pending_id": pending_id, "decision": "allow" })
    );

    let hook_resp = tokio::time::timeout(Duration::from_secs(3), hook_task)
        .await
        .expect("hook request did not unblock promptly")
        .unwrap();
    assert_eq!(hook_resp.status(), 200);
    let hook_json: Value = hook_resp.json().await.unwrap();
    assert_eq!(
        hook_json["hookSpecificOutput"]["hookEventName"],
        "PermissionRequest"
    );
    assert_eq!(
        hook_json["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );

    // 7. Duplicate answer returns isError: true via MCP and 409 Conflict via REST.
    let dup_mcp = mcp_tool(
        &client,
        addr,
        token,
        6,
        "answer_approval",
        json!({
            "pending_id": pending_id,
            "decision": "deny",
        }),
    )
    .await;
    assert_eq!(dup_mcp["isError"], json!(true));

    let dup_rest = client
        .post(format!("http://{addr}/api/agents/answer"))
        .bearer_auth(token)
        .json(&json!({
            "pending_id": pending_id,
            "decision": "deny",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(dup_rest.status(), 409);

    let _ = mcp_tool(
        &client,
        addr,
        token,
        7,
        "kill_session",
        json!({ "session_id": sid }),
    )
    .await;
}

#[tokio::test]
async fn http_and_mcp_never_resize_attached_human_viewport_or_steal_clipboard_owner() {
    let token = "t";
    let addr = start_test_server_with_mcp(token, McpConfig { enabled: true, ..Default::default() }).await;
    let ws_url = format!("ws://{addr}/ws");
    let mut ws = connect(&ws_url).await;
    let id = create_session(&mut ws, osc52_echo_cmd()).await;

    // 1. Attach human WS client with custom viewport (110x32) and become input_owner via 1 byte.
    attach_and_expect_initial_resize(&mut ws, id, 1, 110, 32).await;
    ws.send(input_frame(1, b"\r")).await.unwrap();
    let (init_out, _, init_resizes) = collect_ws_until(&mut ws, 1, "END_").await;
    assert!(init_out.contains("END_"));
    assert!(
        init_resizes.is_empty(),
        "unexpected resize after attach: {init_resizes:?}"
    );

    // 2. Call MCP read_session, MCP send_input (triggering OSC 52 "from-mcp"),
    //    REST screen, and REST input (triggering OSC 52 "via-rest").
    let client = reqwest::Client::new();
    let mcp_read = mcp_tool(
        &client,
        addr,
        token,
        1,
        "read_session",
        json!({ "session_id": id }),
    )
    .await;
    assert_eq!(mcp_read["structuredContent"]["cols"], 110);
    assert_eq!(mcp_read["structuredContent"]["rows"], 32);

    let rest_screen = client
        .get(format!("http://{addr}/api/sessions/{id}/screen"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(rest_screen.status(), 200);
    let rest_screen_json: Value = rest_screen.json().await.unwrap();
    assert_eq!(rest_screen_json["cols"], 110);
    assert_eq!(rest_screen_json["rows"], 32);

    // "ZnJvbS1tY3A=" == base64("from-mcp")
    let mcp_send = mcp_tool(
        &client,
        addr,
        token,
        2,
        "send_input",
        json!({
            "session_id": id,
            "text": "ZnJvbS1tY3A=",
            "enter": true,
            "wait_ms": 2000,
        }),
    )
    .await;
    assert_ne!(mcp_send.get("isError"), Some(&json!(true)));
    assert_eq!(mcp_send["structuredContent"]["screen"]["cols"], 110);
    assert_eq!(mcp_send["structuredContent"]["screen"]["rows"], 32);
    assert!(
        mcp_send["structuredContent"]["screen"]["text"]
            .as_str()
            .unwrap()
            .contains("END_ZnJvbS1tY3A=")
    );

    // "dmlhLXJlc3Q=" == base64("via-rest")
    let rest_send = client
        .post(format!("http://{addr}/api/sessions/{id}/input"))
        .bearer_auth(token)
        .json(&json!({
            "text": "dmlhLXJlc3Q=",
            "enter": true,
            "wait_ms": 2000,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(rest_send.status(), 200);
    let rest_send_json: Value = rest_send.json().await.unwrap();
    assert_eq!(rest_send_json["screen"]["cols"], 110);
    assert_eq!(rest_send_json["screen"]["rows"], 32);
    assert!(
        rest_send_json["screen"]["text"]
            .as_str()
            .unwrap()
            .contains("END_dmlhLXJlc3Q=")
    );

    // 3. Authoritative size in /api/sessions remains (110, 32).
    let list_json: Value = client
        .get(format!("http://{addr}/api/sessions"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sess = list_json["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap();
    assert_eq!(sess["cols"], 110);
    assert_eq!(sess["rows"], 32);

    // 4. Verify WS never received any extra Resized frame, and OSC 52 copies from both
    //    MCP/REST-triggered output AND subsequent WS input still route to WS channel 1.
    let (ws_out, clips, resizes) = collect_ws_until(&mut ws, 1, "END_dmlhLXJlc3Q=").await;
    assert!(
        !ws_out.contains("]52;"),
        "raw OSC 52 must stay stripped: {ws_out:?}"
    );
    assert!(
        resizes.is_empty(),
        "HTTP/MCP operations must not trigger ServerMsg::Resized: {resizes:?}"
    );
    assert!(
        clips.contains(&(1, "from-mcp".to_string())),
        "clipboard owner must still receive OSC 52 after MCP send_input: {clips:?}"
    );
    assert!(
        clips.contains(&(1, "via-rest".to_string())),
        "clipboard owner must still receive OSC 52 after REST input: {clips:?}"
    );

    // "ZnJvbS13cw==" == base64("from-ws")
    ws.send(input_frame(1, b"ZnJvbS13cw==\r")).await.unwrap();
    let (_, clips_ws, resizes_ws) = collect_ws_until(&mut ws, 1, "END_ZnJvbS13cw==").await;
    assert!(resizes_ws.is_empty(), "unexpected resize on WS echo: {resizes_ws:?}");
    assert_eq!(clips_ws, vec![(1, "from-ws".to_string())]);

    ws.send(control(&ClientMsg::KillSession { id })).await.unwrap();
}
