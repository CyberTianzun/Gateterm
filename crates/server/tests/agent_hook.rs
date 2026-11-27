//! hook 摄入面。
//!
//! 这是第三方入口,鉴权与客户端控制面是分开的两套 —— 客户端用 bearer token,
//! agent 用 hook 密钥,后者只存在于 gateterm 亲手 spawn 的会话的环境变量里。

mod common;

use gateterm_server::agent::store::hook_token;

async fn post_hook(
    addr: std::net::SocketAddr,
    path: &str,
    token: Option<&str>,
    session: Option<&str>,
    body: &str,
) -> reqwest::Response {
    let mut req = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .header("Content-Type", "application/json")
        .body(body.to_string());
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    if let Some(s) = session {
        req = req.header("X-Gateterm-Session", s);
    }
    req.send().await.unwrap()
}

async fn agent_sessions(addr: std::net::SocketAddr, token: &str) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("http://{addr}/api/agents/sessions"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap()
}

async fn server() -> std::net::SocketAddr {
    common::start_test_server_at("tok", 1 << 16, Default::default(), vec![]).await
}

/// 认不出的调用必须**安静地收下**,不能回 4xx。
///
/// 这是线上打回来的教训:原先回 401,结果 Claude Code 每轮都往用户脸上打一行
/// `Stop hook error: HTTP 401 from …/hook/claude-code/stop`,因为它把 hook 的非 2xx
/// 当成错误显示。而「不是托管会话打来的」是**完全正常**的事 —— hook 装在用户级全局
/// 配置里,这台机器上每个 Claude 会话都会触发它。
///
/// 安全性由「什么都不做」保证:下面每一条都同时断言**会话表没有被写进去**。
async fn assert_quietly_ignored(addr: std::net::SocketAddr, r: reqwest::Response) {
    assert_eq!(r.status(), 200, "认不出的调用不能回 4xx —— agent 会把它当错误打给用户");
    let v = agent_sessions(addr, "tok").await;
    assert!(v["sessions"].as_array().unwrap().is_empty(), "不该留下任何状态");
}

#[tokio::test]
async fn no_token_is_ignored_not_an_error() {
    let addr = server().await;
    let r = post_hook(addr, "/hook/claude-code/stop", None, Some("1"), "{}").await;
    assert_quietly_ignored(addr, r).await;
}

#[tokio::test]
async fn wrong_token_is_ignored_not_an_error() {
    let addr = server().await;
    let r = post_hook(addr, "/hook/claude-code/stop", Some("nope"), Some("1"), "{}").await;
    assert_quietly_ignored(addr, r).await;
}

/// 客户端那个 bearer token **打不动** hook 面:两套凭据,两个信任域。
/// 安静地忽略,但绝不接受。
#[tokio::test]
async fn client_token_does_not_work_on_hook_plane() {
    let addr = server().await;
    let r = post_hook(addr, "/hook/claude-code/stop", Some("tok"), Some("1"), "{}").await;
    assert_quietly_ignored(addr, r).await;
}

/// 没有会话头 = 不是从 gateterm 会话里起的 agent。这正是「非托管会话」那条路,
/// 也正是线上天天在发生的那条路。
#[tokio::test]
async fn missing_session_header_is_ignored_not_an_error() {
    let addr = server().await;
    let r = post_hook(addr, "/hook/claude-code/stop", Some(hook_token()), None, "{}").await;
    assert_quietly_ignored(addr, r).await;
}

#[tokio::test]
async fn unknown_agent_is_ignored_not_an_error() {
    let addr = server().await;
    let r =
        post_hook(addr, "/hook/no-such-agent/stop", Some(hook_token()), Some("1"), "{}").await;
    assert_quietly_ignored(addr, r).await;
}

#[tokio::test]
async fn bad_json_is_ignored_not_an_error() {
    let addr = server().await;
    let r = post_hook(addr, "/hook/claude-code/stop", Some(hook_token()), Some("1"), "{ nope").await;
    assert_quietly_ignored(addr, r).await;
}

/// agent 升级会带来新事件。收下、忽略、回 200 —— 绝不能因为多了一个事件名就把
/// Claude 卡住或者报错。
#[tokio::test]
async fn unknown_event_is_ignored_not_an_error() {
    let addr = server().await;
    let r = post_hook(
        addr,
        "/hook/claude-code/some-future-event",
        Some(hook_token()),
        Some("1"),
        r#"{"session_id":"s1"}"#,
    )
    .await;
    assert_eq!(r.status(), 200);
    let v = agent_sessions(addr, "tok").await;
    assert!(v["sessions"].as_array().unwrap().is_empty(), "未知事件不该创建会话");
}

#[tokio::test]
async fn telemetry_updates_state() {
    let addr = server().await;
    let r = post_hook(
        addr,
        "/hook/claude-code/pre-tool-use",
        Some(hook_token()),
        Some("7"),
        r#"{"session_id":"s1","tool_name":"Bash","cwd":"/tmp/x"}"#,
    )
    .await;
    assert_eq!(r.status(), 200);

    let v = agent_sessions(addr, "tok").await;
    let s = &v["sessions"][0];
    assert_eq!(s["agent_id"], "claude-code");
    assert_eq!(s["agent_session_id"], "s1");
    assert_eq!(s["session"], 7);
    assert_eq!(s["state"], "working");
    assert_eq!(s["detail"], "Bash");
    assert_eq!(s["cwd"], "/tmp/x");
}

/// 「在等人」是整条链路里唯一必须精确的状态,它只从 Notification 来。
#[tokio::test]
async fn notification_marks_waiting() {
    let addr = server().await;
    post_hook(
        addr,
        "/hook/claude-code/notification",
        Some(hook_token()),
        Some("7"),
        r#"{"session_id":"s1","notification_type":"permission_prompt"}"#,
    )
    .await;
    let v = agent_sessions(addr, "tok").await;
    assert_eq!(v["sessions"][0]["state"], "waiting");
    assert_eq!(v["sessions"][0]["detail"], "permission_prompt");
}

/// Grok 兼容读取 `~/.claude/settings.json`。其中 SessionStart 是 command hook,
/// 在 gateterm 终端里继承了会话 id 与密钥,会带着合法凭据打到 `/hook/claude-code/`。
/// 它的 payload 带着 grok 独有的驼峰 `hookEventName`,必须被 claude-code 拒收,
/// 否则 grok 终端会被识别成 Claude Code。
#[tokio::test]
async fn grok_envelope_on_claude_route_is_ignored() {
    let addr = server().await;
    for (path, body) in [
        (
            "/hook/claude-code/session-start",
            r#"{"sessionId":"g1","session_id":"g1","hookEventName":"session_start","hook_event_name":"SessionStart","cwd":"/tmp/x"}"#,
        ),
        (
            "/hook/claude-code/permission-request",
            r#"{"sessionId":"g1","session_id":"g1","hookEventName":"permission_request","tool_name":"Bash"}"#,
        ),
    ] {
        let r = post_hook(addr, path, Some(hook_token()), Some("7"), body).await;
        assert_quietly_ignored(addr, r).await;
    }
}

/// 不假设 `SessionStart` 一定先到 —— Task 3 的端到端里实测过 `-p` 模式下第一个
/// 到达的是 `UserPromptSubmit`。任何事件都要能惰性建出会话。
#[tokio::test]
async fn any_event_can_create_the_session() {
    let addr = server().await;
    post_hook(
        addr,
        "/hook/claude-code/user-prompt-submit",
        Some(hook_token()),
        Some("3"),
        r#"{"session_id":"late"}"#,
    )
    .await;
    let v = agent_sessions(addr, "tok").await;
    assert_eq!(v["sessions"][0]["agent_session_id"], "late");
    assert_eq!(v["sessions"][0]["state"], "thinking");
}

/// 只带 tool_name 的后续事件不该把先前记下的 cwd 抹掉。
#[tokio::test]
async fn partial_update_does_not_clear_known_fields() {
    let addr = server().await;
    post_hook(
        addr,
        "/hook/claude-code/session-start",
        Some(hook_token()),
        Some("5"),
        r#"{"session_id":"s9","cwd":"/work/repo"}"#,
    )
    .await;
    post_hook(
        addr,
        "/hook/claude-code/pre-tool-use",
        Some(hook_token()),
        Some("5"),
        r#"{"session_id":"s9","tool_name":"Edit"}"#,
    )
    .await;
    let v = agent_sessions(addr, "tok").await;
    assert_eq!(v["sessions"][0]["cwd"], "/work/repo", "cwd 被后续事件抹掉了");
    assert_eq!(v["sessions"][0]["state"], "working");
}

/// `transcript_path` 是聊天视图的地基:每个 hook 事件都带着它,我们一直收到、
/// 一直扔掉。存下来。
#[tokio::test]
async fn transcript_path_is_kept_from_the_hook_payload() {
    let addr = server().await;
    post_hook(
        addr,
        "/hook/claude-code/session-start",
        Some(hook_token()),
        Some("1"),
        r#"{"session_id":"s1","transcript_path":"/home/u/.claude/projects/p/s1.jsonl"}"#,
    )
    .await;
    let v = agent_sessions(addr, "tok").await;
    assert_eq!(v["sessions"][0]["transcript"], "/home/u/.claude/projects/p/s1.jsonl");
}

/// `Update` 里的 `None` 是「这次没说」,不是「清空」。
///
/// 这个错在 agent 集成里犯过一次(只带 tool_name 的事件把 cwd 抹掉了),
/// 那次是靠测试抓出来的,这里提前钉住。
#[tokio::test]
async fn a_later_event_without_the_path_does_not_clear_it() {
    let addr = server().await;
    post_hook(
        addr,
        "/hook/claude-code/session-start",
        Some(hook_token()),
        Some("1"),
        r#"{"session_id":"s1","transcript_path":"/tmp/t.jsonl"}"#,
    )
    .await;
    // PreToolUse 的 payload 里同样有 transcript_path,但这里故意不给 —— 模拟任何
    // 不带这个字段的事件
    post_hook(
        addr,
        "/hook/claude-code/pre-tool-use",
        Some(hook_token()),
        Some("1"),
        r#"{"session_id":"s1","tool_name":"Bash"}"#,
    )
    .await;
    let v = agent_sessions(addr, "tok").await;
    assert_eq!(v["sessions"][0]["transcript"], "/tmp/t.jsonl", "后续事件把已知路径抹掉了");
    assert_eq!(v["sessions"][0]["detail"], "Bash");
}

/// `PostToolUse:Read` 会把完整文件内容或 base64 图片塞进 `tool_response`,
/// 很容易超过 512 KiB。服务端必须正常解析其余元数据、更新会话状态并返回 200,
/// 不能由 body limit 拦成 HTTP 413。
#[tokio::test]
async fn large_post_tool_use_updates_state_instead_of_413() {
    let addr = server().await;
    let big_response = "a".repeat(1024 * 1024);
    let body = format!(
        r#"{{"session_id":"s-big","tool_name":"Read","tool_input":{{"file_path":"/work/big.png"}},"tool_response":"{big_response}","cwd":"/work/repo","transcript_path":"/tmp/big.jsonl"}}"#
    );
    let r = post_hook(
        addr,
        "/hook/claude-code/post-tool-use",
        Some(hook_token()),
        Some("7"),
        &body,
    )
    .await;
    assert_eq!(r.status(), 200, "超过 512 KiB 的 PostToolUse 不能返回 413");

    let v = agent_sessions(addr, "tok").await;
    let s = &v["sessions"][0];
    assert_eq!(s["agent_session_id"], "s-big");
    assert_eq!(s["state"], "working");
    assert_eq!(s["detail"], "Read");
    assert_eq!(s["cwd"], "/work/repo");
    assert_eq!(s["transcript"], "/tmp/big.jsonl");
}

/// 非托管会话(没有 hook token)触发的超大 `PostToolUse` 同样不能回 413 ——
/// 必须先鉴权、排空 body、安静回 200 且不留状态。
#[tokio::test]
async fn large_unauthenticated_hook_is_quietly_ignored_instead_of_413() {
    let addr = server().await;
    let big_response = "b".repeat(1024 * 1024);
    let body = format!(r#"{{"session_id":"s-ext","tool_name":"Read","tool_response":"{big_response}"}}"#);
    let r = post_hook(addr, "/hook/claude-code/post-tool-use", None, None, &body).await;
    assert_quietly_ignored(addr, r).await;
}

/// 即使 payload 超出服务端内存解析上限(16 MiB),也必须排空连接并安静回 200 `{}`,
/// 绝不能回 413 或直接断连把报错打到 agent TUI 上。
#[tokio::test]
async fn oversized_hook_payload_beyond_parse_cap_is_ignored_not_413() {
    let addr = server().await;
    let huge = "c".repeat(17 * 1024 * 1024);
    let body = format!(r#"{{"session_id":"s-huge","tool_name":"Read","tool_response":"{huge}"}}"#);
    let r = post_hook(
        addr,
        "/hook/claude-code/post-tool-use",
        Some(hook_token()),
        Some("7"),
        &body,
    )
    .await;
    assert_quietly_ignored(addr, r).await;
}

/// chunked 传输(无 Content-Length)下超过解析上限时,同样必须排空并回 200 `{}`。
#[tokio::test]
async fn oversized_chunked_hook_payload_is_ignored_not_413() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = server().await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "POST /hook/claude-code/post-tool-use HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Authorization: Bearer {}\r\n\
         X-Gateterm-Session: 7\r\n\
         Content-Type: application/json\r\n\
         Transfer-Encoding: chunked\r\n\
         Connection: close\r\n\r\n",
        hook_token(),
    );
    stream.write_all(head.as_bytes()).await.unwrap();

    let prefix = br#"{"session_id":"s-chunked","tool_response":""#;
    stream
        .write_all(format!("{:x}\r\n", prefix.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(prefix).await.unwrap();
    stream.write_all(b"\r\n").await.unwrap();

    let chunk = vec![b'z'; 1024 * 1024];
    let chunk_head = format!("{:x}\r\n", chunk.len());
    for _ in 0..17 {
        stream.write_all(chunk_head.as_bytes()).await.unwrap();
        stream.write_all(&chunk).await.unwrap();
        stream.write_all(b"\r\n").await.unwrap();
    }

    let suffix = br#""}"#;
    stream
        .write_all(format!("{:x}\r\n", suffix.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(suffix).await.unwrap();
    stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
    stream.flush().await.unwrap();

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let resp = String::from_utf8_lossy(&raw);
    assert!(
        resp.starts_with("HTTP/1.1 200 OK\r\n"),
        "expected HTTP 200 OK for oversized chunked hook, got: {}",
        resp.lines().next().unwrap_or("")
    );
    let v = agent_sessions(addr, "tok").await;
    assert!(v["sessions"].as_array().unwrap().is_empty());
}
