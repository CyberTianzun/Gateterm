use futures_util::{SinkExt, StreamExt};
use gateterm_protocol::{ClientMsg, Frame, ServerMsg};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

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
    let app = gateterm_demo::app(vec!["a".into(), "b".into()]).unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Service { base, task }
}
async fn get(s: &Service, token: &str, path: &str) -> Value {
    let r = reqwest::Client::new()
        .get(format!("{}{path}", s.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{path}");
    assert_eq!(r.headers()["cache-control"], "no-store");
    r.json().await.unwrap()
}
async fn socket(s: &Service, token: &str) -> impl_socket::Ws {
    let mut ws = tokio_tungstenite::connect_async(format!("{}/ws", s.base.replace("http:", "ws:")))
        .await
        .unwrap()
        .0;
    send(
        &mut ws,
        ClientMsg::Hello {
            token: token.into(),
            proto: 1,
        },
    )
    .await;
    assert!(matches!(msg(&mut ws).await, ServerMsg::HelloOk { .. }));
    ws
}
mod impl_socket {
    pub type Ws = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;
}
async fn send(ws: &mut impl_socket::Ws, msg: ClientMsg) {
    ws.send(Message::Binary(
        Frame::new(0, serde_json::to_vec(&msg).unwrap())
            .encode()
            .into(),
    ))
    .await
    .unwrap();
}
async fn frame(ws: &mut impl_socket::Ws) -> Frame {
    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(3), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Binary(bytes) = next {
            return Frame::decode(&bytes).unwrap();
        }
    }
}
async fn msg(ws: &mut impl_socket::Ws) -> ServerMsg {
    serde_json::from_slice(&frame(ws).await.payload).unwrap()
}
fn messages_path(snapshot: &Value) -> String {
    format!(
        "/api/agents/messages?agent_id={}&agent_session_id={}",
        snapshot["sessions"][0]["agent_id"].as_str().unwrap(),
        snapshot["sessions"][0]["agent_session_id"]
            .as_str()
            .unwrap()
    )
}
async fn attach(ws: &mut impl_socket::Ws, id: u64) {
    send(
        ws,
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
    assert!(matches!(msg(ws).await, ServerMsg::Attached { .. }));
    assert!(matches!(msg(ws).await, ServerMsg::Resized { .. }));
    assert!(matches!(msg(ws).await, ServerMsg::ResyncBegin { .. }));
    assert!(String::from_utf8_lossy(&frame(ws).await.payload).contains("review allow"));
    assert!(matches!(msg(ws).await, ServerMsg::ResyncEnd { .. }));
}

#[tokio::test]
async fn android_shapes_are_synthetic_authenticated_bounded_and_cursor_compatible() {
    let s = service().await;
    let unauthorized = reqwest::Client::new()
        .get(format!("{}/api/agents/sessions", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), 401);
    let mut untouched = socket(&s, "a").await;
    send(&mut untouched, ClientMsg::ListSessions).await;
    assert!(
        matches!(msg(&mut untouched).await, ServerMsg::Sessions { sessions } if sessions.is_empty())
    );
    untouched.close(None).await.unwrap();
    let snapshot = get(&s, "a", "/api/agents/sessions").await;
    assert_eq!(snapshot["transcript_enabled"], true);
    assert_eq!(snapshot["sessions"].as_array().unwrap().len(), 1);
    let agent = &snapshot["sessions"][0];
    assert_eq!(agent["agent_id"], "simulated-review");
    assert_eq!(agent["supports_transcript"], true);
    assert_eq!(agent["state"], "waiting");
    assert!(agent["detail"].as_str().unwrap().contains("Simulated"));
    assert_eq!(get(&s, "a", "/api/agents/sessions").await, snapshot);
    let agents = get(&s, "a", "/api/agents").await;
    assert!(
        agents["agents"][0]["name"]
            .as_str()
            .unwrap()
            .contains("Simulated")
    );
    assert_eq!(agents["agents"][0]["detected"]["installed"], false);
    let path = messages_path(&snapshot);
    let transcript = get(&s, "a", &path).await;
    assert_eq!(transcript["source"], "transcript");
    assert_eq!(transcript["reset"], true);
    assert_eq!(transcript["more"], false);
    assert!(transcript.to_string().len() < 4096);
    for m in transcript["messages"].as_array().unwrap() {
        assert!(m["id"].is_string());
        assert!(
            m["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|b| b.to_string().contains("Simulated"))
        );
    }
    let cursor = transcript["cursor"].as_str().unwrap();
    let next = get(&s, "a", &format!("{path}&after={cursor}")).await;
    assert_eq!(next["messages"], json!([]));
    assert_eq!(next["reset"], false);
    let mut ws = socket(&s, "a").await;
    send(&mut ws, ClientMsg::ListSessions).await;
    assert!(
        matches!(msg(&mut ws).await, ServerMsg::Sessions { sessions } if sessions.len() == 1 && sessions[0].id == agent["session"].as_u64().unwrap() && sessions[0].name.contains("Simulated"))
    );
    for route in ["/api/agents/sessions", &path] {
        let r = reqwest::Client::new()
            .get(format!("{}{route}", s.base))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
    }
    for query in [
        "",
        "?agent_id=host&agent_session_id=anything",
        "?agent_id=simulated-review&agent_session_id=../../etc/passwd",
        "?agent_id=simulated-review&agent_session_id=bad&after=x&before=x",
    ] {
        let r = reqwest::Client::new()
            .get(format!("{}/api/agents/messages{query}", s.base))
            .bearer_auth("a")
            .send()
            .await
            .unwrap();
        assert!(matches!(r.status().as_u16(), 400 | 404));
    }
}

#[tokio::test]
async fn terminal_choices_only_change_token_local_simulation_and_survive_reconnect() {
    let s = service().await;
    for (token, choice) in [("a", "allow"), ("b", "deny")] {
        let snapshot = get(&s, token, "/api/agents/sessions").await;
        let path = messages_path(&snapshot);
        let initial = get(&s, token, &path).await;
        let mut ws = socket(&s, token).await;
        attach(
            &mut ws,
            snapshot["sessions"][0]["session"].as_u64().unwrap(),
        )
        .await;
        ws.send(Message::Binary(
            Frame::new(1, format!("review {choice}\r").into_bytes())
                .encode()
                .into(),
        ))
        .await
        .unwrap();
        let out = frame(&mut ws).await;
        assert_eq!(out.channel, 1);
        assert!(String::from_utf8_lossy(&out.payload).contains("nothing was executed"));
        assert!(matches!(
            msg(&mut ws).await,
            ServerMsg::AgentEvent {
                state: gateterm_protocol::AgentState::Idle,
                ..
            }
        ));
        let next = get(
            &s,
            token,
            &format!("{path}&after={}", initial["cursor"].as_str().unwrap()),
        )
        .await;
        assert_eq!(next["reset"], false);
        assert_eq!(next["messages"].as_array().unwrap().len(), 1);
        assert!(next["messages"][0].to_string().contains(choice));
        let full = get(&s, token, &path).await;
        for _ in 0..20 {
            ws.send(Message::Binary(
                Frame::new(1, b"review allow\rreview deny\rreview reset\r".to_vec())
                    .encode()
                    .into(),
            ))
            .await
            .unwrap();
            assert_eq!(frame(&mut ws).await.channel, 1);
        }
        assert_eq!(get(&s, token, &path).await, full);
        ws.close(None).await.unwrap();
        let mut ws = socket(&s, token).await;
        attach(
            &mut ws,
            snapshot["sessions"][0]["session"].as_u64().unwrap(),
        )
        .await;
        assert_eq!(
            get(&s, token, "/api/agents/sessions").await["sessions"][0]["state"],
            "idle"
        );
        if token == "a" {
            assert_eq!(
                get(&s, "b", "/api/agents/sessions").await["sessions"][0]["state"],
                "waiting"
            );
        }
    }
    for path in [
        "/api/agents/answer",
        "/api/agents/simulated-review/install",
        "/api/agents/history/open",
        "/api/files",
    ] {
        for method in [
            reqwest::Method::POST,
            reqwest::Method::PUT,
            reqwest::Method::DELETE,
        ] {
            let r = reqwest::Client::new()
                .request(method, format!("{}{path}", s.base))
                .bearer_auth("a")
                .json(&json!({"decision":"allow"}))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 403);
        }
    }
}

#[tokio::test]
async fn review_terminal_recreates_after_deletion_without_exceeding_session_quota() {
    let s = service().await;
    let first = get(&s, "a", "/api/agents/sessions").await;
    let mut ws = socket(&s, "a").await;
    send(
        &mut ws,
        ClientMsg::KillSession {
            id: first["sessions"][0]["session"].as_u64().unwrap(),
        },
    )
    .await;
    assert!(matches!(msg(&mut ws).await, ServerMsg::SessionEvent { .. }));
    let next = get(&s, "a", "/api/agents/sessions").await;
    assert_ne!(
        next["sessions"][0]["session"],
        first["sessions"][0]["session"]
    );
    assert!(matches!(msg(&mut ws).await, ServerMsg::SessionEvent { .. }));
    for _ in 0..7 {
        send(
            &mut ws,
            ClientMsg::NewSession {
                name: None,
                command: None,
                cwd: None,
            },
        )
        .await;
        assert!(matches!(msg(&mut ws).await, ServerMsg::SessionEvent { .. }));
    }
    for _ in 0..20 {
        assert_eq!(get(&s, "a", "/api/agents/sessions").await, next);
    }
    send(
        &mut ws,
        ClientMsg::NewSession {
            name: None,
            command: None,
            cwd: None,
        },
    )
    .await;
    assert!(matches!(msg(&mut ws).await, ServerMsg::Error { .. }));
    send(&mut ws, ClientMsg::ListSessions).await;
    assert!(matches!(msg(&mut ws).await, ServerMsg::Sessions { sessions } if sessions.len() == 8));
}
