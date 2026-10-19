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
use crate::session::manager::SessionManager;

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

pub fn spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, creds: SharedCreds) {
    // 没有可用配置目录(等同「永远未登录」)时不启动:与旧行为「无路径不启动」一致。
    let Some(path) = creds.path().map(Path::to_path_buf) else { return };
    let mut rx = manager.events();
    tokio::spawn(async move {
        // 同一个 Cloud url 复用一个 reqwest::Client(连接池),而不是每次推送都新建一个。
        let mut clients: HashMap<String, Arc<CloudClient>> = HashMap::new();
        let mut last: HashMap<(String, String), (AgentState, Option<String>)> = HashMap::new();
        loop {
            let msg = match rx.recv().await {
                Ok(m) => m,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            let ServerMsg::AgentEvent { agent_id, agent_session_id, session, state, pending, .. } = msg else {
                continue;
            };
            let key = (agent_id.clone(), agent_session_id);
            let kind = classify(last.get(&key), state, &pending);
            if state == AgentState::Done {
                last.remove(&key);
            } else {
                last.insert(key, (state, pending.clone()));
            }
            let (Some(kind), Some(session)) = (kind, session) else { continue };
            let Some(creds) = creds.get() else { continue };
            let tool_name = pending
                .as_ref()
                .and_then(|id| store.list_pending().into_iter().find(|p| &p.id == id))
                .and_then(|p| p.tool_name);
            let note = Notify { session, agent_id, kind: kind.into(), tool_name, expires_at: None };
            let client = clients.entry(creds.url.clone()).or_insert_with(|| Arc::new(CloudClient::new(&creds.url))).clone();
            tokio::spawn(send(path.clone(), client, creds, note));
        }
    });
}

async fn send(path: PathBuf, client: Arc<CloudClient>, creds: Credentials, note: Notify) {
    match client.notify(&creds.token, &note).await {
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
}
