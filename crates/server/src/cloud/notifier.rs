//! 把「agent 在等你」转成 Cloud 推送。
//!
//! 订阅的是和 WS 推送同一条 `AgentEvent` 广播,所以不需要改 agent 链路的任何一处。
//! 推送是尽力而为:Cloud 不可达只记 debug 日志,绝不反压到 hook 或 WS。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use octoterm_cloud_client::credentials::{self, Credentials};
use octoterm_cloud_client::{CloudClient, CloudError, Notify};
use octoterm_protocol::{AgentState, ServerMsg};
use tokio::sync::broadcast::error::RecvError;

use crate::agent::store::AgentSessionStore;
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

/// 按 (mtime, 文件长度) 缓存的 cloud.json:desktop 登录/退出后不用重启 server 就能生效。
/// 单看 mtime 在粗粒度文件系统上不够:同一个 tick 里内容换了,mtime 可能没变。
struct CredCache {
    path: PathBuf,
    cached: Option<((SystemTime, u64), Credentials)>,
}

impl CredCache {
    fn get(&mut self) -> Option<Credentials> {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            self.cached = None;
            return None;
        };
        let Ok(mtime) = meta.modified() else {
            self.cached = None;
            return None;
        };
        let key = (mtime, meta.len());
        if let Some((k, c)) = &self.cached
            && *k == key
        {
            return Some(c.clone());
        }
        match credentials::load(&self.path) {
            Ok(Some(c)) => {
                self.cached = Some((key, c.clone()));
                Some(c)
            }
            Ok(None) => {
                self.cached = None;
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "cloud.json 读取失败,本次不推送");
                None
            }
        }
    }
}

pub fn spawn(manager: Arc<SessionManager>, store: Arc<AgentSessionStore>, cred_path: Option<PathBuf>) {
    let Some(path) = cred_path else { return };
    let mut rx = manager.events();
    tokio::spawn(async move {
        let mut cache = CredCache { path: path.clone(), cached: None };
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
            let Some(creds) = cache.get() else { continue };
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

    /// 粗粒度文件系统(常见于 HFS+/一些容器挂载)上 mtime 的精度可能只有 1 秒;
    /// 同一秒内两次写入内容不同但 mtime 相同的 cloud.json,缓存键只看 mtime 会读到旧内容。
    #[test]
    fn cred_cache_busts_on_length_change_even_when_mtime_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        let creds1 = Credentials { url: "https://cloud.example.com".into(), email: "a@example.com".into(), token: "t1".into(), server_id: None };
        credentials::save(&path, &creds1).unwrap();
        let mtime1 = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut cache = CredCache { path: path.clone(), cached: None };
        assert_eq!(cache.get(), Some(creds1));

        let creds2 = Credentials {
            url: "https://cloud.example.com".into(),
            email: "someone-else@example.com".into(),
            token: "a-much-longer-replacement-token".into(),
            server_id: Some("srv-2".into()),
        };
        credentials::save(&path, &creds2).unwrap();
        // 把新内容的 mtime 强行拨回旧值,模拟低精度文件系统下两次写撞在同一个 tick 里。
        std::fs::File::open(&path).unwrap().set_modified(mtime1).unwrap();

        assert_eq!(cache.get(), Some(creds2), "内容变了但 mtime 没变时,缓存也该失效");
    }

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
