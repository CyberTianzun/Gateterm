use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_protocol::{AgentState, ServerMsg};
use octoterm_server::agent::store::AgentSessionStore;
use octoterm_server::cloud::creds::SharedCreds;
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

/// 每个请求在假 Cloud 侧的 (收到时刻, 应答完成时刻),按到达顺序排列。
type Timing = Arc<Mutex<Vec<(Instant, Instant)>>>;

/// 像 `fake_cloud`,但第一个 /v1/notify 请求会先 `sleep(delay)` 才回包,
/// 并记录每个请求的收到/完成时刻——用来验证发送方真的在等前一个请求完成后才发下一个,
/// 而不是把多条请求并发甩出去,靠网络竞速决定到达顺序。
async fn fake_cloud_delay_first(status: u16, delay: Duration) -> (String, Seen, Timing) {
    let seen: Seen = Arc::default();
    let timing: Timing = Arc::default();
    let counter = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/notify",
            post(
                move |State((seen, timing, counter)): State<(Seen, Timing, Arc<AtomicUsize>)>,
                      axum::Json(body): axum::Json<Value>| async move {
                    let start = Instant::now();
                    let idx = counter.fetch_add(1, Ordering::SeqCst);
                    if idx == 0 {
                        tokio::time::sleep(delay).await;
                    }
                    seen.lock().unwrap().push(body);
                    timing.lock().unwrap().push((start, Instant::now()));
                    if status == 401 {
                        (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "unauthorized"})))
                    } else {
                        (StatusCode::ACCEPTED, axum::Json(json!({"delivered": 1})))
                    }
                },
            ),
        )
        .with_state((seen.clone(), timing.clone(), counter));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen, timing)
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

fn save_login(path: &std::path::Path, url: &str) {
    credentials::save(
        path,
        &Credentials { url: url.into(), email: "a@example.com".into(), token: "oct_s".into(), server_id: Some("srv".into()) },
    )
    .unwrap();
}

struct Rig {
    manager: Arc<SessionManager>,
    store: Arc<AgentSessionStore>,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new(url: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        if let Some(url) = url {
            save_login(&path, url);
        }
        let manager = SessionManager::new(1 << 20, Default::default());
        let store = Arc::new(AgentSessionStore::new());
        notifier::spawn(manager.clone(), store.clone(), SharedCreds::new(Some(path)));
        Self { manager, store, dir }
    }

    fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("cloud.json")
    }

    /// 补登录:构造时未登录,事后再写 `cloud.json`。
    fn login(&self, url: &str) {
        save_login(&self.path(), url);
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
    // 发送方是单一队列、逐条顺序处理,到达假 Cloud 的顺序跟发出顺序一致:先 waiting 再 permission。
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[0]["kind"], "waiting", "{seen:?}");
    assert_eq!(seen[1]["kind"], "permission", "{seen:?}");
}

#[tokio::test]
async fn sends_are_delivered_in_order_even_if_first_is_slow() {
    // 第一个请求故意卡 300ms:如果发送方仍然各自 spawn 一个任务去发请求,
    // 第二个请求(resolved)就可能在网络竞速里比第一个(waiting)先到。
    let (url, seen, timing) = fake_cloud_delay_first(202, Duration::from_millis(300)).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Working, None));
    wait_for(&seen, 2).await;

    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies[0]["kind"], "waiting", "{bodies:?}");
    assert_eq!(bodies[1]["kind"], "resolved", "{bodies:?}");

    let timing = timing.lock().unwrap().clone();
    assert_eq!(timing.len(), 2);
    // 第二个请求的「收到时刻」不能早于第一个请求的「应答完成时刻」——
    // 否则说明发送方没有等前一个请求真正结束就把下一个发出去了。
    assert!(
        timing[1].0 >= timing[0].1,
        "第二个请求在第一个应答完成前就已经发出: req1=(start={:?}, finish={:?}), req2 start={:?}",
        timing[0].0,
        timing[0].1,
        timing[1].0
    );
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

#[tokio::test]
async fn leaving_waiting_sends_resolved() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    rig.manager.publish(event(AgentState::Waiting, None));
    rig.manager.publish(event(AgentState::Working, None));
    wait_for(&seen, 2).await;
    let seen = seen.lock().unwrap().clone();
    let resolved: Vec<_> = seen.iter().filter(|b| b["kind"] == "resolved").collect();
    assert_eq!(resolved.len(), 1);
    assert_eq!(*resolved[0], json!({"session": 7, "agent_id": "claude-code", "kind": "resolved"}));
}

#[tokio::test]
async fn no_resolved_for_a_wait_that_was_never_notified() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(None);
    rig.manager.publish(event(AgentState::Waiting, None));
    tokio::time::sleep(Duration::from_millis(100)).await;
    rig.login(&url);
    rig.manager.publish(event(AgentState::Working, None));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn permission_carries_expires_at() {
    let (url, seen) = fake_cloud(202).await;
    let rig = Rig::new(Some(&url));
    let expires = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 590;
    let _rx = rig.store.insert_pending(octoterm_server::agent::store::PendingRequest {
        id: "p1".into(),
        agent_id: "claude-code".into(),
        agent_session_id: "s-1".into(),
        session: Some(7),
        tool_name: Some("Bash".into()),
        tool_input: json!({"command": "rm -rf /tmp/x"}),
        created_at: expires - 590,
        expires_at: expires,
    });
    rig.manager.publish(event(AgentState::Waiting, Some("p1")));
    wait_for(&seen, 1).await;
    let body = seen.lock().unwrap()[0].clone();
    assert_eq!(body["kind"], "permission");
    assert_eq!(body["tool_name"], "Bash");
    assert_eq!(body["expires_at"], expires as i64);
    assert!(body.get("tool_input").is_none());
}
