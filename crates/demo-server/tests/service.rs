use futures_util::{SinkExt, StreamExt};
use gateterm_protocol::{ClientMsg, Frame, ServerMsg};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
struct Service {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn service() -> Service {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = gateterm_demo::app(vec!["token-a".into(), "token-b".into()]).unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Service { base, task }
}
async fn socket(s: &Service) -> Ws {
    tokio_tungstenite::connect_async(format!("{}/ws", s.base.replace("http:", "ws:")))
        .await
        .unwrap()
        .0
}
async fn send(ws: &mut Ws, msg: ClientMsg) {
    ws.send(Message::Binary(
        Frame::new(0, serde_json::to_vec(&msg).unwrap())
            .encode()
            .into(),
    ))
    .await
    .unwrap();
}
async fn frame(ws: &mut Ws) -> Frame {
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(3), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            Message::Binary(bytes) => return Frame::decode(&bytes).unwrap(),
            Message::Ping(_) => {}
            m => panic!("unexpected {m:?}"),
        }
    }
}
async fn msg(ws: &mut Ws) -> ServerMsg {
    serde_json::from_slice(&frame(ws).await.payload).unwrap()
}
async fn login(s: &Service, token: &str) -> Ws {
    let mut ws = socket(s).await;
    send(
        &mut ws,
        ClientMsg::Hello {
            token: token.into(),
            proto: 1,
        },
    )
    .await;
    assert!(matches!(
        msg(&mut ws).await,
        ServerMsg::HelloOk {
            proto: 1,
            win32_input_mode: false,
            ..
        }
    ));
    ws
}
async fn create(ws: &mut Ws) -> u64 {
    send(
        ws,
        ClientMsg::NewSession {
            name: None,
            command: Some(vec!["demo".into()]),
            cwd: None,
        },
    )
    .await;
    match msg(ws).await {
        ServerMsg::SessionEvent { session, .. } => session.id,
        m => panic!("{m:?}"),
    }
}
async fn attach(ws: &mut Ws, id: u64, seq: Option<u64>) -> u64 {
    send(
        ws,
        ClientMsg::Attach {
            id,
            channel: 1,
            last_seq: seq,
            cols: 80,
            rows: 24,
            colors: None,
        },
    )
    .await;
    assert!(matches!(msg(ws).await, ServerMsg::Attached { .. }));
    assert!(matches!(msg(ws).await, ServerMsg::Resized { .. }));
    if seq.is_some() {
        return 0;
    }
    assert!(matches!(msg(ws).await, ServerMsg::ResyncBegin { .. }));
    assert!(String::from_utf8_lossy(&frame(ws).await.payload).contains("Virtual demo"));
    match msg(ws).await {
        ServerMsg::ResyncEnd { seq, .. } => seq,
        m => panic!("{m:?}"),
    }
}

#[tokio::test]
async fn http_auth_shapes_and_host_features_fail_closed() {
    let s = service().await;
    let http = reqwest::Client::new();
    for path in ["/api/launchers", "/api/agents/history", "/api/unknown"] {
        let response = http.get(format!("{}{path}", s.base)).send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert!(response.bytes().await.unwrap().is_empty());
        assert_eq!(
            http.get(format!("{}{path}?token=token-a", s.base))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    for (path, key) in [
        ("/api/launchers", "launchers"),
        ("/api/agents", "agents"),
        ("/api/agents/sessions", "sessions"),
        ("/api/agents/pending", "pending"),
    ] {
        let response = http
            .get(format!("{}{path}", s.base))
            .bearer_auth("token-a")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let json: serde_json::Value = response.json().await.unwrap();
        assert!(json[key].is_array());
        if path.ends_with("sessions") {
            assert_eq!(json["transcript_enabled"], true);
        }
    }
    let cloud: serde_json::Value = http
        .get(format!("{}/api/cloud/status", s.base))
        .bearer_auth("token-a")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cloud["connected"], false);
    for path in ["/api/agents/history", "/api/files", "/api/paste-image"] {
        assert_eq!(
            http.get(format!("{}{path}", s.base))
                .bearer_auth("token-a")
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    for path in [
        "/api/agents/codex/install",
        "/api/agents/answer",
        "/api/cloud/register",
        "/api/files",
    ] {
        assert_eq!(
            http.post(format!("{}{path}", s.base))
                .bearer_auth("token-a")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert!(
        http.get(&s.base)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
            .contains("Virtual demo")
    );
    assert_eq!(
        http.get(format!("{}/healthz", s.base))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    assert_eq!(
        http.get(format!("{}/etc/passwd", s.base))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn binary_handshake_rejects_wrong_credentials_and_all_non_hello_first_frames() {
    let s = service().await;
    for first in [
        Message::Text("hello".into()),
        Message::Binary(vec![0, 0].into()),
        Message::Binary(
            Frame::new(0, br#"{"type":"hello","token":"bad","proto":1}"#.to_vec())
                .encode()
                .into(),
        ),
        Message::Binary(
            Frame::new(
                0,
                br#"{"type":"hello","token":"token-a","proto":2}"#.to_vec(),
            )
            .encode()
            .into(),
        ),
        Message::Binary(
            Frame::new(
                2,
                br#"{"type":"hello","token":"token-a","proto":1}"#.to_vec(),
            )
            .encode()
            .into(),
        ),
    ] {
        let mut ws = socket(&s).await;
        ws.send(first).await.unwrap();
        assert!(matches!(
            msg(&mut ws).await,
            ServerMsg::Error { channel: None, .. }
        ));
        assert!(matches!(ws.next().await, Some(Ok(Message::Close(_)))));
    }
    let mut ws = login(&s, "token-a").await;
    send(
        &mut ws,
        ClientMsg::Hello {
            token: "token-a".into(),
            proto: 1,
        },
    )
    .await;
    assert!(matches!(msg(&mut ws).await, ServerMsg::Error { .. }));
    ws.send(Message::Text("ignored".into())).await.unwrap();
    ws.send(Message::Binary(vec![0, 0].into())).await.unwrap();
    send(&mut ws, ClientMsg::ListSessions).await;
    assert!(matches!(msg(&mut ws).await,ServerMsg::Sessions{sessions} if sessions.is_empty()));
}

#[tokio::test]
async fn real_websocket_io_token_isolation_and_reconnect_preserve_files_and_bytes() {
    let s = service().await;
    let mut a = login(&s, "token-a").await;
    let id = create(&mut a).await;
    let seq = attach(&mut a, id, None).await;
    a.send(Message::Binary(
        Frame::new(
            1,
            b"mkdir notes\recho 'hello world' > notes/hello.txt\rcat notes/hello.txt\r".to_vec(),
        )
        .encode()
        .into(),
    ))
    .await
    .unwrap();
    let data = frame(&mut a).await;
    assert_eq!(data.channel, 1);
    assert!(String::from_utf8_lossy(&data.payload).contains("hello world\r\n"));
    a.close(None).await.unwrap();
    let mut b = login(&s, "token-b").await;
    send(&mut b, ClientMsg::ListSessions).await;
    assert!(matches!(msg(&mut b).await,ServerMsg::Sessions{sessions} if sessions.is_empty()));
    let b_id = create(&mut b).await;
    attach(&mut b, b_id, None).await;
    b.send(Message::Binary(
        Frame::new(1, b"cat /workspace/notes/hello.txt\r".to_vec())
            .encode()
            .into(),
    ))
    .await
    .unwrap();
    assert!(String::from_utf8_lossy(&frame(&mut b).await.payload).contains("demo: "));
    let mut a = login(&s, "token-a").await;
    attach(&mut a, id, Some(seq)).await;
    assert_eq!(frame(&mut a).await.payload, data.payload);
    for byte in "cat notes/hello.txt\r".as_bytes() {
        a.send(Message::Binary(Frame::new(1, vec![*byte]).encode().into()))
            .await
            .unwrap();
    }
    let mut text = String::new();
    while !text.contains("hello world\r\n") {
        text.push_str(&String::from_utf8_lossy(&frame(&mut a).await.payload));
    }
}

#[tokio::test]
async fn frame_and_global_connection_limits_are_enforced() {
    let s = service().await;
    let mut sockets = vec![];
    for _ in 0..16 {
        sockets.push(login(&s, "token-a").await);
    }
    let result =
        tokio_tungstenite::connect_async(format!("{}/ws", s.base.replace("http:", "ws:"))).await;
    assert!(
        matches!(result,Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status()==503)
    );
    let ws = &mut sockets[0];
    ws.send(Message::Binary(
        Frame::new(1, vec![b'x'; 65537]).encode().into(),
    ))
    .await
    .unwrap();
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(3), ws.next())
            .await
            .unwrap(),
        Some(Err(_)) | Some(Ok(Message::Close(_))) | None
    ));
}

#[test]
fn no_unauthenticated_or_duplicate_token_configuration() {
    assert!(gateterm_demo::app(vec![]).is_err());
    assert!(gateterm_demo::app(vec!["".into()]).is_err());
    assert!(gateterm_demo::app(vec!["x".into(), "x".into()]).is_err());
}

#[tokio::test]
async fn hello_timeout_reports_error_then_closes() {
    let s = service().await;
    let mut ws = socket(&s).await;
    let incoming = tokio::time::timeout(std::time::Duration::from_secs(7), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Binary(bytes) = incoming else {
        panic!("expected binary error")
    };
    let f = Frame::decode(&bytes).unwrap();
    assert!(matches!(
        serde_json::from_slice::<ServerMsg>(&f.payload).unwrap(),
        ServerMsg::Error { channel: None, .. }
    ));
    assert!(matches!(ws.next().await, Some(Ok(Message::Close(_)))));
}

#[tokio::test]
async fn shared_socket_session_events_resize_unicode_preview_and_scoped_errors() {
    use base64::Engine;
    let s = service().await;
    let mut a = login(&s, "token-a").await;
    let mut b = login(&s, "token-a").await;
    let id = create(&mut a).await;
    assert!(matches!(msg(&mut b).await, ServerMsg::SessionEvent { .. }));
    attach(&mut a, id, None).await;
    attach(&mut b, id, None).await;
    send(
        &mut b,
        ClientMsg::Resize {
            channel: 1,
            cols: 0,
            rows: u16::MAX,
            colors: None,
        },
    )
    .await;
    for ws in [&mut a, &mut b] {
        assert!(matches!(
            msg(ws).await,
            ServerMsg::Resized {
                cols: 20,
                rows: 24,
                ..
            }
        ));
        assert!(matches!(msg(ws).await, ServerMsg::ResyncBegin { .. }));
        assert_eq!(frame(ws).await.channel, 1);
        assert!(matches!(msg(ws).await, ServerMsg::ResyncEnd { .. }));
    }
    let mut out_a = vec![];
    let mut out_b = vec![];
    for scalar in "echo 你好🙂\r".chars() {
        for byte in scalar.to_string().as_bytes() {
            a.send(Message::Binary(Frame::new(1, vec![*byte]).encode().into()))
                .await
                .unwrap();
        }
        let fa = frame(&mut a).await;
        let fb = frame(&mut b).await;
        assert_eq!(fa.payload, fb.payload);
        out_a.extend(fa.payload);
        out_b.extend(fb.payload);
    }
    assert_eq!(out_a, out_b);
    assert!(String::from_utf8(out_a).unwrap().contains("你好🙂\r\n"));
    send(&mut a, ClientMsg::Preview { id }).await;
    let ServerMsg::PreviewData { data, .. } = msg(&mut a).await else {
        panic!("preview missing")
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert!(bytes.len() <= 65536);
    assert!(String::from_utf8_lossy(&bytes).contains("你好🙂"));
    send(
        &mut a,
        ClientMsg::RenameSession {
            id,
            name: "Review".into(),
        },
    )
    .await;
    for ws in [&mut a, &mut b] {
        assert!(
            matches!(msg(ws).await,ServerMsg::SessionEvent{event:gateterm_protocol::SessionEventKind::Renamed,session} if session.name.contains("Virtual demo")&&session.name.contains("Review"))
        );
    }
    send(
        &mut a,
        ClientMsg::Attach {
            id,
            channel: 1,
            last_seq: None,
            cols: 80,
            rows: 24,
            colors: None,
        },
    )
    .await;
    assert!(matches!(
        msg(&mut a).await,
        ServerMsg::Error {
            channel: Some(1),
            ..
        }
    ));
    a.send(Message::Binary(
        Frame::new(42, b"anything".to_vec()).encode().into(),
    ))
    .await
    .unwrap();
    assert!(matches!(
        msg(&mut a).await,
        ServerMsg::Error {
            channel: Some(42),
            ..
        }
    ));
    send(&mut a, ClientMsg::KillSession { id }).await;
    for ws in [&mut a, &mut b] {
        assert!(matches!(
            msg(ws).await,
            ServerMsg::SessionExited { channel: 1, .. }
        ));
        assert!(matches!(
            msg(ws).await,
            ServerMsg::SessionEvent {
                event: gateterm_protocol::SessionEventKind::Closed,
                ..
            }
        ));
    }
}
