use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_protocol::{AgentState, ServerMsg};
use octoterm_server::agent::store::AgentSessionStore;
use octoterm_server::cloud::notifier;
use octoterm_server::session::manager::SessionManager;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Value>>>;

/// 只实现 /v1/notify 的假 Cloud。`status` 决定返回 202 还是 401。
async fn fake_cloud(status: u16) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let app = Router::new()
        .route(
            "/v1/notify",
            post(move |State(seen): State<Seen>, axum::Json(body): axum::Json<Value>| async move {
                seen.lock().unwrap().push(body);
                if status == 401 {
                    (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "unauthorized"})))
                } else {
                    (StatusCode::ACCEPTED, axum::Json(json!({"delivered": 1})))
                }
            }),
        )
        .with_state(seen.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

fn event(state: AgentState, pending: Option<&str>) -> ServerMsg {
    ServerMsg::AgentEvent {
        agent_id: "claude-code".into(),
        agent_session_id: "s-1".into(),
        session: Some(7),
        state,
        pending: pending.map(String::from),
        detail: Some("secret detail".into()),
    }
}

struct Rig {
    manager: Arc<SessionManager>,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new(url: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        if let Some(url) = url {
            credentials::save(
                &path,
                &Credentials { url: url.into(), email: "a@example.com".into(), token: "oct_s".into(), server_id: Some("srv".into()) },
            )
            .unwrap();
        }
        let manager = SessionManager::new(1 << 20, Default::default());
        notifier::spawn(manager.clone(), Arc::new(AgentSessionStore::new()), Some(path));
        Self { manager, dir }
    }

    fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("cloud.json")
    }
}

async fn wait_for(seen: &Seen, n: usize) {
    for _ in 0..100 {
        if seen.lock().unwrap().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("等了 2 秒仍只收到 {} 条", seen.lock().unwrap().len());
}

#[tokio::test]
async fn entering_waiting_notifies_once() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Working, None));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Waiting, None));
    wait_for(&seen, 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec![json!({"session": 7, "agent_id": "claude-code", "kind": "waiting"})]);
}

#[tokio::test]
async fn new_pending_request_notifies_as_permission() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Waiting, Some("p1")));
    wait_for(&seen, 2).await;
    // 两条推送是各自独立 spawn 出去的 HTTP 请求,到达假 Cloud 的顺序不保证跟发出顺序一致;
    // 只断言两种 kind 都收到了,别靠下标。
    let seen = seen.lock().unwrap().clone();
    assert!(seen.iter().any(|v| v["kind"] == "waiting"), "{seen:?}");
    assert!(seen.iter().any(|v| v["kind"] == "permission"), "{seen:?}");
}

#[tokio::test]
async fn not_logged_in_sends_nothing() {
    let (_url, seen) = fake_cloud(202).await;
    let rig = Rig::new(None);
    rig.manager.publish(event(AgentState::Waiting, None));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn revoked_credentials_are_deleted() {
    let (url, seen) = fake_cloud(401).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    wait_for(&seen, 1).await;
    for _ in 0..50 {
        if !rig.path().exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("401 之后 cloud.json 仍然存在");
}
