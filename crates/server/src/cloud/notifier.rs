//! 把「agent 在等你」转成 Cloud 推送。
//!
//! 订阅的是和 WS 推送同一条 `AgentEvent` 广播,所以不需要改 agent 链路的任何一处。
//! 推送是尽力而为:Cloud 不可达只记 debug 日志,绝不反压到 hook 或 WS。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_cloud_client::{CloudClient, CloudError, Notify};
use octoterm_protocol::{AgentState, ServerMsg};
use tokio::sync::broadcast::error::RecvError;

use crate::agent::store::AgentSessionStore;
use crate::cloud::creds::SharedCreds;
use crate::session::manager::{AgentNotice, SessionManager};

/// Only an explicit successful Stop hook means a completed turn. Idle can also mean
/// startup, interruption, process cleanup or stale state; none should notify completion.
pub fn is_turn_completed(event: &str, payload: &serde_json::Value, state: AgentState) -> bool {
    event == "Stop" && state == AgentState::Idle
        && !matches!(payload.get("reason").and_then(serde_json::Value::as_str),
            Some("shutdown" | "channel_closed" | "cancelled" | "interrupted" | "error"))
}

/// 这条事件要不要推,推哪一种。
///
/// 只在**迁移**时推:刚进入 waiting,或 waiting 中冒出一个新的 pending。
/// 同一个 waiting 状态的重复事件(hook 会反复上报)不再打扰用户。
pub fn classify(prev: Option<&(AgentState, Option<String>)>, state: AgentState, pending: &Option<String>) -> Option<&'static str> {
    if state != AgentState::Waiting {
        return None;
    }
    let was_waiting = matches!(prev, Some((AgentState::Waiting, _)));
    let new_pending = pending.is_some() && prev.map(|(_, p)| p) != Some(pending);
    (!was_waiting || new_pending).then_some(if pending.is_some() { "permission" } else { "waiting" })
}

/// 每个 agent 会话上次的状态,以及是否真的为它发出过「在等你」——
/// 决定离开 waiting 时要不要撤回:从没通知过的等待,撤回只会是噪音。
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub state: Option<AgentState>,
    pub pending: Option<String>,
    pub notified: bool,
    pub completed: bool,
}

/// 在 `classify`(进入/新 pending)之上补「离开 waiting → resolved」。
pub fn decide(prev: &Track, state: AgentState, pending: &Option<String>) -> Option<&'static str> {
    if state == AgentState::Waiting {
        let p = prev.state.map(|s| (s, prev.pending.clone()));
        return classify(p.as_ref(), state, pending);
    }
    (prev.state == Some(AgentState::Waiting) && prev.notified).then_some("resolved")
}

/// Cloud 只接受 `now < exp <= now + 3600` 的过期时刻,整段校验失败会让整条 notify 被拒。
/// 留 60 秒安全余量:时间太紧(或已经过期)就宁可不带这个字段,也别因为卡着临界值被整条拒掉。
pub fn clamp_expiry(exp: i64, now: i64) -> Option<i64> {
    if exp - now < 60 { None } else { Some(exp.min(now + 3600 - 60)) }
}

/// 一次待发送的推送,按产生顺序排队,由唯一的发送任务顺序处理——
/// 避免「resolved」在网络竞速里超过更早的「waiting」/「permission」。
struct Job {
    path: PathBuf,
    client: Arc<CloudClient>,
    creds: Credentials,
    note: Notify,
}

pub fn spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, creds: SharedCreds) {
    // 没有可用配置目录(等同「永远未登录」)时不启动:与旧行为「无路径不启动」一致。
    let Some(path) = creds.path().map(Path::to_path_buf) else { return };
    let mut rx = manager.agent_notices();
    // unbounded 是有意的:事件速率低(只在 waiting/permission/resolved 迁移时才排队),
    // 且每条请求都有客户端 15s 超时托底,不会无限堆积。
    let (tx, mut job_rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
    // 唯一的发送任务:逐条 await,保证到达 Cloud 的顺序跟产生顺序一致。
    tokio::spawn(async move {
        while let Some(job) = job_rx.recv().await {
            send(job.path, job.client, job.creds, job.note).await;
        }
    });
    tokio::spawn(async move {
        // 同一个 Cloud url 复用一个 reqwest::Client(连接池),而不是每次推送都新建一个。
        let mut clients: HashMap<String, Arc<CloudClient>> = HashMap::new();
        let mut last: HashMap<(String, String), Track> = HashMap::new();
        loop {
            let msg = match rx.recv().await {
                Ok(m) => m,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            let msg = match msg {
                AgentNotice::TurnCompleted { session, agent_id, agent_session_id } => {
                    let prev = last.entry((agent_id.clone(), agent_session_id)).or_default();
                    if prev.completed { continue; }
                    prev.completed = true;
                    let Some(login) = creds.get() else { continue };
                    let client = clients.entry(login.url.clone()).or_insert_with(|| Arc::new(CloudClient::new(&login.url))).clone();
                    let note = Notify { session, agent_id, kind: "done".into(), tool_name: None, expires_at: None };
                    let _ = tx.send(Job { path: path.clone(), client, creds: login, note });
                    continue;
                }
                AgentNotice::State(msg) => msg,
            };
            let ServerMsg::AgentEvent { agent_id, agent_session_id, session, state, pending, .. } = msg else { continue };
            let key = (agent_id.clone(), agent_session_id);
            let prev = last.get(&key).cloned().unwrap_or_default();
            let kind = decide(&prev, state, &pending);
            // 只有真要发时才读 cloud.json;login 为 None = 未登录
            let login = kind.and(session).and_then(|_| creds.get());
            let sending = kind.is_some() && login.is_some();
            let notified = match kind {
                Some("waiting" | "permission") => sending || prev.notified,
                _ if state != AgentState::Waiting => false,
                _ => prev.notified,
            };
            if state == AgentState::Done {
                last.remove(&key);
            } else {
                let completed = prev.completed && !matches!(state, AgentState::Working | AgentState::Thinking);
                last.insert(key, Track { state: Some(state), pending: pending.clone(), notified, completed });
            }
            let (Some(kind), Some(session), Some(login)) = (kind, session, login) else { continue };
            let pending_req = (kind == "permission")
                .then(|| pending.as_ref().and_then(|id| store.list_pending().into_iter().find(|p| &p.id == id)))
                .flatten();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let note = Notify {
                session,
                agent_id,
                kind: kind.into(),
                tool_name: pending_req.as_ref().and_then(|p| p.tool_name.clone()),
                expires_at: pending_req.and_then(|p| clamp_expiry(p.expires_at as i64, now)),
            };
            let client = clients.entry(login.url.clone()).or_insert_with(|| Arc::new(CloudClient::new(&login.url))).clone();
            // 只是把 job 推进内存队列,不等网络 I/O——事件循环不能被 Cloud 拖慢。
            let _ = tx.send(Job { path: path.clone(), client, creds: login, note });
        }
    });
}

async fn send(path: PathBuf, client: Arc<CloudClient>, creds: Credentials, note: Notify) {
    match client.notify(&creds.token, &note).await {
        Ok(0) => tracing::warn!("Cloud 没有投递到设备，请检查手机推送登记及 Cloud 推送日志"),
        Ok(_) => {}
        Err(CloudError::Unauthorized) => {
            // 只删「还是这把 token」的文件:期间用户可能已经重新登录了。
            if let Ok(Some(cur)) = credentials::load(&path)
                && cur.token == creds.token
            {
                let _ = credentials::delete(&path);
                tracing::warn!("Cloud 凭据已失效(在别处退出、删号或重置了密码),本机已退出 Cloud 登录");
            }
        }
        // Cloud 明确拒绝(4xx/5xx):只记状态码和错误码,绝不带 token 或请求体。
        Err(CloudError::Api { status, code }) => {
            tracing::warn!(status, code = %code, "Cloud 拒绝了推送");
        }
        Err(e) => tracing::debug!(error = %e, "Cloud 推送失败"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentState::*;

    #[test]
    fn classify_transitions() {
        assert_eq!(classify(None, Working, &None), None);
        assert_eq!(classify(None, Waiting, &None), Some("waiting"));
        assert_eq!(classify(Some(&(Working, None)), Waiting, &None), Some("waiting"));
        assert_eq!(classify(Some(&(Waiting, None)), Waiting, &None), None);
        assert_eq!(classify(Some(&(Waiting, None)), Waiting, &Some("p1".into())), Some("permission"));
        assert_eq!(classify(Some(&(Waiting, Some("p1".into()))), Waiting, &Some("p1".into())), None);
        assert_eq!(classify(Some(&(Waiting, Some("p1".into()))), Waiting, &Some("p2".into())), Some("permission"));
        // 回答完、pending 被摘掉但仍在 waiting:不是新的打扰
        assert_eq!(classify(Some(&(Waiting, Some("p1".into()))), Waiting, &None), None);
        assert_eq!(classify(Some(&(Waiting, None)), Done, &None), None);
    }

    fn track(state: Option<AgentState>, pending: Option<&str>, notified: bool) -> Track {
        Track { state, pending: pending.map(String::from), notified, completed: false }
    }

    #[test]
    fn completion_requires_a_successful_stop_not_just_idle_or_session_exit() {
        let empty = serde_json::json!({});
        assert!(is_turn_completed("Stop", &empty, Idle));
        for event in ["SessionStart", "SessionEnd", "Interrupt", "StopFailure", "StopCancelled", "Notification"] {
            assert!(!is_turn_completed(event, &empty, Idle));
        }
        for reason in ["shutdown", "channel_closed", "cancelled", "interrupted", "error"] {
            assert!(!is_turn_completed("Stop", &serde_json::json!({"reason": reason}), Idle));
        }
        assert!(!is_turn_completed("Stop", &empty, Done));
        assert!(!is_turn_completed("Stop", &empty, Working));
    }

    #[tokio::test]
    async fn completion_is_ordered_after_withdrawal_and_deduplicated_per_turn() {
        use axum::{Json, Router, routing::post};
        use serde_json::{Value, json};
        use std::time::Duration;

        let (notes, mut received) = tokio::sync::mpsc::unbounded_channel::<Value>();
        let app = Router::new().route("/v1/notify", post(move |Json(note): Json<Value>| {
            let notes = notes.clone();
            async move {
                notes.send(note).unwrap();
                (axum::http::StatusCode::ACCEPTED, Json(json!({"delivered": 1})))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        credentials::save(&path, &Credentials {
            url, email: "test@example.com".into(), token: "test".into(), server_id: Some("srv".into()),
        }).unwrap();
        let manager = SessionManager::new(1 << 20, Default::default());
        spawn(manager.clone(), Arc::new(AgentSessionStore::new()), SharedCreds::new(Some(path)));
        let publish = |state| manager.publish(ServerMsg::AgentEvent {
            agent_id: "codex".into(), agent_session_id: "turns".into(), session: Some(7),
            state, pending: None, detail: None,
        });
        publish(Waiting);
        publish(Idle); // Withdrawal must precede completion, otherwise it could erase the completion alert.
        manager.turn_completed(7, "codex", "turns");
        manager.turn_completed(7, "codex", "turns"); // Duplicate Stop hook.
        publish(Thinking); // A new prompt re-arms completion.
        publish(Idle);
        manager.turn_completed(7, "codex", "turns");
        for expected in ["waiting", "resolved", "done", "done"] {
            let note = tokio::time::timeout(Duration::from_secs(2), received.recv()).await.unwrap().unwrap();
            assert_eq!(note["kind"], expected);
            assert_eq!(note["session"], 7);
            assert_eq!(note["agent_id"], "codex");
        }
        assert!(tokio::time::timeout(Duration::from_millis(100), received.recv()).await.is_err());
        server.abort();
    }

    #[test]
    fn clamp_expiry_cases() {
        let now = 1_700_000_000i64;
        // 很远的将来:截到 now + 3600 - 60
        assert_eq!(clamp_expiry(now + 100_000, now), Some(now + 3540));
        // 留了安全余量以内但还够 60 秒:原样返回
        assert_eq!(clamp_expiry(now + 590, now), Some(now + 590));
        // 太接近 60 秒门槛、恰好等于 now、已经过去:统统宁可不带这个字段
        assert_eq!(clamp_expiry(now + 30, now), None);
        assert_eq!(clamp_expiry(now, now), None);
        assert_eq!(clamp_expiry(now - 10, now), None);
    }

    #[test]
    fn decide_transitions() {
        assert_eq!(decide(&track(None, None, false), Waiting, &None), Some("waiting"));
        assert_eq!(decide(&track(Some(Waiting), None, true), Waiting, &Some("p".into())), Some("permission"));
        assert_eq!(decide(&track(Some(Waiting), None, true), Working, &None), Some("resolved"));
        assert_eq!(decide(&track(Some(Waiting), None, true), Done, &None), Some("resolved"));
        // 从没真的发出过通知(比如当时没登录)就不撤回
        assert_eq!(decide(&track(Some(Waiting), None, false), Working, &None), None);
        assert_eq!(decide(&track(Some(Working), None, false), Idle, &None), None);
    }
}
