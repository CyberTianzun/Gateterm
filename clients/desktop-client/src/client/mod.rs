pub mod api;
pub mod ws;

use std::collections::{HashMap, HashSet};

use gateterm_protocol::AgentState;

pub use api::{
    AgentIntegrationStatus, AgentSessionInfo, AgentSessionsResponse, ChatBlock, ChatMessage,
    ChatRole, ChatWindow, CloudStatus, GateHttpClient, HistoryEntry, HistoryOpenResponse,
    HistoryPage, LauncherItem, PendingRequestInfo,
};
pub use ws::{WsCommand, WsConnectionState, WsEvent, spawn_ws_worker};

#[derive(Debug, Clone, Default)]
pub struct ChatTranscriptState {
    pub messages: Vec<ChatMessage>,
    pub cursor: String,
    pub older_cursor: Option<String>,
    pub fallback_reason: Option<String>,
}

pub fn apply_chat_window(state: &mut ChatTranscriptState, win: ChatWindow, prepend: bool) {
    match &win {
        ChatWindow::Transcript {
            cursor,
            older_cursor,
            reset,
            ..
        } => {
            state.fallback_reason = None;
            state.messages = merge_chat_window(&state.messages, &win, prepend);
            if !prepend || state.cursor.is_empty() || *reset {
                state.cursor = cursor.clone();
            }
            if prepend || state.older_cursor.is_none() || *reset {
                state.older_cursor = older_cursor.clone();
            }
        }
        ChatWindow::Terminal { reason } => {
            state.fallback_reason = Some(reason.clone());
        }
    }
}

pub type AgentMap = HashMap<String, AgentSessionInfo>;

fn state_priority(state: AgentState) -> u8 {
    match state {
        AgentState::Waiting => 5,
        AgentState::Error => 4,
        AgentState::Working => 3,
        AgentState::Thinking => 2,
        AgentState::Idle => 1,
        AgentState::Done => 0,
    }
}

pub fn apply_agent_event(map: &mut AgentMap, ev: AgentSessionInfo) {
    let key = ev.key();
    if ev.state == AgentState::Done {
        map.remove(&key);
    } else {
        let supports_transcript = map
            .get(&key)
            .map(|prev| prev.supports_transcript)
            .unwrap_or(ev.supports_transcript);
        map.insert(
            key,
            AgentSessionInfo {
                supports_transcript,
                ..ev
            },
        );
    }
}

pub fn replace_all_agents(map: &mut AgentMap, list: Vec<AgentSessionInfo>) {
    map.clear();
    for s in list {
        if s.state != AgentState::Done {
            map.insert(s.key(), s);
        }
    }
}

pub fn best_agent_for_session(map: &AgentMap, session_id: u64) -> Option<AgentSessionInfo> {
    map.values()
        .filter(|s| s.session == Some(session_id))
        .max_by_key(|s| state_priority(s.state))
        .cloned()
}

pub fn chat_agent_for_session(map: &AgentMap, session_id: u64) -> Option<AgentSessionInfo> {
    map.values()
        .filter(|s| s.session == Some(session_id) && s.supports_transcript)
        .max_by_key(|s| state_priority(s.state))
        .cloned()
        .or_else(|| best_agent_for_session(map, session_id))
}

pub fn merge_chat_window(
    prev: &[ChatMessage],
    win: &ChatWindow,
    prepend: bool,
) -> Vec<ChatMessage> {
    let ChatWindow::Transcript {
        messages, reset, ..
    } = win
    else {
        return prev.to_vec();
    };
    let base: Vec<ChatMessage> = if *reset { Vec::new() } else { prev.to_vec() };
    let mut seen: HashSet<String> = base.iter().map(|m| m.id.clone()).collect();
    let incoming: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| seen.insert(m.id.clone()))
        .cloned()
        .collect();
    if prepend {
        let mut combined = incoming;
        combined.extend(base);
        combined
    } else {
        let mut combined = base;
        combined.extend(incoming);
        combined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_priority_prefers_waiting_over_working() {
        let mut map = AgentMap::new();
        apply_agent_event(
            &mut map,
            AgentSessionInfo {
                agent_id: "claude-code".into(),
                agent_session_id: "s1".into(),
                session: Some(1),
                state: AgentState::Working,
                detail: Some("Bash".into()),
                pending: None,
                supports_transcript: true,
            },
        );
        apply_agent_event(
            &mut map,
            AgentSessionInfo {
                agent_id: "codex".into(),
                agent_session_id: "s2".into(),
                session: Some(1),
                state: AgentState::Waiting,
                detail: Some("Permission".into()),
                pending: Some("p1".into()),
                supports_transcript: true,
            },
        );
        let best = best_agent_for_session(&map, 1).unwrap();
        assert_eq!(best.agent_id, "codex");
        assert_eq!(best.state, AgentState::Waiting);
    }

    #[test]
    fn merge_chat_window_deduplicates_and_resets() {
        let m1 = ChatMessage {
            id: "1".into(),
            role: ChatRole::User,
            ts: None,
            blocks: vec![ChatBlock::Text { text: "hello".into() }],
        };
        let m2 = ChatMessage {
            id: "2".into(),
            role: ChatRole::Assistant,
            ts: None,
            blocks: vec![ChatBlock::Text { text: "world".into() }],
        };
        let win = ChatWindow::Transcript {
            messages: vec![m1.clone(), m2.clone()],
            cursor: "c2".into(),
            reset: false,
            more: false,
            older_cursor: None,
        };
        let merged = merge_chat_window(&[m1], &win, false);
        assert_eq!(merged.len(), 2);

        let reset_win = ChatWindow::Transcript {
            messages: vec![m2],
            cursor: "c3".into(),
            reset: true,
            more: false,
            older_cursor: None,
        };
        let after_reset = merge_chat_window(&merged, &reset_win, false);
        assert_eq!(after_reset.len(), 1);
        assert_eq!(after_reset[0].id, "2");
    }
}
