pub mod api;
pub mod mcp;
pub mod ws;

use std::collections::{HashMap, HashSet};

use gateterm_protocol::AgentState;

pub use api::{
    AgentIntegrationStatus, AgentSessionInfo, AgentSessionsResponse, ChatBlock, ChatMessage,
    ChatRole, ChatWindow, CloudStatus, GateHttpClient, HistoryEntry, HistoryOpenResponse,
    HistoryPage, LauncherItem, PendingRequestInfo,
};
pub use mcp::{
    McpPresets, McpServerStatus, build_mcp_presets, derive_mcp_urls, mask_token,
    should_apply_status_response,
};
pub use ws::{WsCommand, WsConnectionState, WsEvent, spawn_ws_worker};

pub const MAX_AUTO_OLDER_PAGES: usize = 16;

const HARNESS_USER_PREFIXES: [&str; 8] = [
    "# AGENTS.md instructions",
    "<environment_context>",
    "<permissions instructions>",
    "<developer_instructions>",
    "<system-reminder>",
    "<local-command-caveat>",
    "<command-name>",
    "This session is being continued from a previous conversation",
];

const CODEX_REQUEST_MARKER: &str = "## My request for Codex:";

/// Returns `true` if a `role = user` text block is empty or harness-injected context
/// (aligned with `crates/server/src/agent/history.rs::prompt_body`).
pub fn is_harness_user_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    if HARNESS_USER_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }
    if let Some((_, after)) = trimmed.split_once(CODEX_REQUEST_MARKER) {
        if after.trim().is_empty() {
            return true;
        }
    }
    false
}

/// Returns `true` if the message contains at least one genuine human `ChatBlock::Text` prompt.
pub fn is_user_prompt(msg: &ChatMessage) -> bool {
    if msg.role != ChatRole::User {
        return false;
    }
    msg.blocks.iter().any(|b| match b {
        ChatBlock::Text { text } => !is_harness_user_text(text),
        _ => false,
    })
}

/// Returns `true` if any message in the slice is a genuine human user prompt.
pub fn has_user_prompt(messages: &[ChatMessage]) -> bool {
    messages.iter().any(is_user_prompt)
}

#[derive(Debug, Clone, Default)]
pub struct ChatTranscriptState {
    pub messages: Vec<ChatMessage>,
    pub cursor: String,
    pub older_cursor: Option<String>,
    pub history_supported: bool,
    pub older_loading: bool,
    pub older_failed: bool,
    pub timeline: u64,
    pub agent_key: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OlderPageStep {
    Continue { next_cursor: String },
    StopFoundUserPrompt,
    StopBeginning,
    StopPageLimit,
    StopReset,
    StopUnsupportedOldServer,
    StopFailed,
}

pub fn apply_chat_window(state: &mut ChatTranscriptState, win: ChatWindow, prepend: bool) {
    let incoming_older = win.older_cursor().map(str::to_string);
    let incoming_supported = win.history_supported();
    match &win {
        ChatWindow::Transcript { cursor, reset, .. } => {
            let is_fresh = !prepend && (state.cursor.is_empty() || *reset);
            state.fallback_reason = None;
            state.messages = merge_chat_window(&state.messages, &win, prepend);
            if !prepend || state.cursor.is_empty() || *reset {
                state.cursor = cursor.clone();
            }
            if prepend || is_fresh {
                state.older_cursor = incoming_older;
                state.history_supported = incoming_supported;
                if is_fresh || *reset {
                    state.older_loading = false;
                    state.older_failed = false;
                }
            }
            if *reset {
                state.timeline = state.timeline.wrapping_add(1);
            }
        }
        ChatWindow::Terminal { reason } => {
            state.fallback_reason = Some(reason.clone());
            state.older_loading = false;
            state.timeline = state.timeline.wrapping_add(1);
        }
    }
}

/// Applies one backward pagination response to `state` and returns the next loop step.
pub fn apply_older_page(
    state: &mut ChatTranscriptState,
    requested_before: &str,
    win: ChatWindow,
    pages_fetched_after: usize,
    max_pages: usize,
    is_history_preview: bool,
) -> OlderPageStep {
    let next_older = win.older_cursor().map(str::to_string);
    let next_supported = win.history_supported();
    let ChatWindow::Transcript {
        messages, reset, ..
    } = &win
    else {
        state.older_loading = false;
        state.older_failed = true;
        return OlderPageStep::StopFailed;
    };

    if *reset {
        if next_older.as_deref() == Some(requested_before) {
            state.older_loading = false;
            if is_history_preview {
                state.older_cursor = None;
                state.history_supported = false;
                state.older_failed = false;
                return OlderPageStep::StopUnsupportedOldServer;
            }
            state.older_failed = true;
            return OlderPageStep::StopFailed;
        }
        apply_chat_window(state, win, false);
        return OlderPageStep::StopReset;
    }

    if next_older.as_deref() == Some(requested_before) {
        state.older_loading = false;
        state.older_failed = true;
        return OlderPageStep::StopFailed;
    }

    let incoming = unique_incoming(&state.messages, messages);
    let found_prompt = has_user_prompt(&incoming);
    let mut combined = incoming;
    combined.append(&mut state.messages);
    state.messages = combined;
    state.older_cursor = next_older.clone();
    state.history_supported = next_supported;
    state.older_failed = false;
    state.fallback_reason = None;

    if found_prompt {
        state.older_loading = false;
        return OlderPageStep::StopFoundUserPrompt;
    }
    let Some(next_cursor) = next_older else {
        state.older_loading = false;
        return OlderPageStep::StopBeginning;
    };
    if pages_fetched_after >= max_pages {
        state.older_loading = false;
        return OlderPageStep::StopPageLimit;
    }
    state.older_loading = true;
    OlderPageStep::Continue { next_cursor }
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

fn unique_incoming(base: &[ChatMessage], messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut seen: HashSet<String> = base.iter().map(|m| m.id.clone()).collect();
    messages
        .iter()
        .filter(|m| seen.insert(m.id.clone()))
        .cloned()
        .collect()
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
    let incoming = unique_incoming(&base, messages);
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

    fn msg(id: &str, role: ChatRole, blocks: Vec<ChatBlock>) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role,
            ts: None,
            blocks,
        }
    }

    fn text_msg(id: &str, role: ChatRole, text: &str) -> ChatMessage {
        msg(
            id,
            role,
            vec![ChatBlock::Text {
                text: text.into(),
            }],
        )
    }

    fn transcript_win_raw(
        messages: Vec<ChatMessage>,
        cursor: &str,
        reset: bool,
        older_cursor: Option<Option<&str>>,
    ) -> ChatWindow {
        let mut obj = serde_json::Map::new();
        obj.insert("source".into(), serde_json::json!("transcript"));
        obj.insert("messages".into(), serde_json::to_value(messages).unwrap());
        obj.insert("cursor".into(), serde_json::json!(cursor));
        obj.insert("reset".into(), serde_json::json!(reset));
        obj.insert("more".into(), serde_json::json!(false));
        if let Some(inner) = older_cursor {
            obj.insert(
                "older_cursor".into(),
                match inner {
                    None => serde_json::Value::Null,
                    Some(s) => serde_json::Value::String(s.to_string()),
                },
            );
        }
        serde_json::from_value(serde_json::Value::Object(obj)).unwrap()
    }

    #[test]
    fn is_user_prompt_filters_roles_tools_blanks_and_harness_prefixes() {
        assert!(!is_user_prompt(&text_msg("a1", ChatRole::Assistant, "hi")));
        assert!(!is_user_prompt(&text_msg("s1", ChatRole::System, "hi")));
        assert!(!is_user_prompt(&msg(
            "tr",
            ChatRole::User,
            vec![ChatBlock::ToolResult {
                ok: true,
                text: "ok".into(),
            }],
        )));
        assert!(!is_user_prompt(&msg(
            "pi",
            ChatRole::User,
            vec![
                ChatBlock::ToolUse {
                    name: "bash".into(),
                    input: "ls".into(),
                },
                ChatBlock::ToolResult {
                    ok: true,
                    text: "file".into(),
                },
            ],
        )));
        assert!(!is_user_prompt(&text_msg("blank", ChatRole::User, "  \n\t ")));

        let prefixes = [
            "# AGENTS.md instructions\nrule",
            "  <environment_context>\n<cwd>/tmp</cwd>",
            "<permissions instructions>\nallow",
            "<developer_instructions>\nbe concise",
            "<system-reminder>\nremember",
            "<local-command-caveat>note",
            "<command-name>/clear</command-name>",
            "This session is being continued from a previous conversation:\nsummary",
        ];
        for (i, prefix_text) in prefixes.iter().enumerate() {
            assert!(
                !is_user_prompt(&text_msg(&format!("p{i}"), ChatRole::User, prefix_text)),
                "prefix {i} should not count as human user prompt"
            );
        }

        assert!(!is_user_prompt(&text_msg(
            "codex-empty",
            ChatRole::User,
            "context header\n## My request for Codex:\n   \n",
        )));

        assert!(is_user_prompt(&msg(
            "mixed",
            ChatRole::User,
            vec![
                ChatBlock::ToolResult {
                    ok: true,
                    text: "out".into(),
                },
                ChatBlock::Text {
                    text: "<system-reminder>hint</system-reminder>".into(),
                },
                ChatBlock::Text {
                    text: "Fix bug".into(),
                },
            ],
        )));
        assert!(is_user_prompt(&text_msg(
            "codex-real",
            ChatRole::User,
            "context header\n## My request for Codex:\nFix the crash",
        )));

        assert!(!has_user_prompt(&[]));
        assert!(!has_user_prompt(&[text_msg("a", ChatRole::Assistant, "ok")]));
        assert!(has_user_prompt(&[
            text_msg("a", ChatRole::Assistant, "ok"),
            text_msg("u", ChatRole::User, "real question"),
        ]));
    }

    #[test]
    fn apply_chat_window_tracks_history_supported_and_preserves_older_on_forward_increment() {
        let mut state = ChatTranscriptState::default();
        state.older_loading = true;
        state.older_failed = true;
        let initial = transcript_win_raw(
            vec![text_msg("m2", ChatRole::Assistant, "working")],
            "c2",
            true,
            Some(Some("older-1")),
        );
        apply_chat_window(&mut state, initial, false);
        assert!(state.history_supported);
        assert_eq!(state.older_cursor.as_deref(), Some("older-1"));
        assert!(!state.older_loading);
        assert!(!state.older_failed);
        assert_eq!(state.timeline, 1);

        let fwd = transcript_win_raw(
            vec![text_msg("m3", ChatRole::Assistant, "done")],
            "c3",
            false,
            Some(None),
        );
        apply_chat_window(&mut state, fwd, false);
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.cursor, "c3");
        assert!(state.history_supported);
        assert_eq!(state.older_cursor.as_deref(), Some("older-1"));
        assert_eq!(state.timeline, 1);

        state.older_loading = true;
        state.older_failed = true;
        let reset_win = transcript_win_raw(
            vec![text_msg("m4", ChatRole::Assistant, "fresh")],
            "c4",
            true,
            Some(None),
        );
        apply_chat_window(&mut state, reset_win, false);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].id, "m4");
        assert_eq!(state.cursor, "c4");
        assert_eq!(state.older_cursor, None);
        assert!(state.history_supported);
        assert!(!state.older_loading);
        assert!(!state.older_failed);
        assert_eq!(state.timeline, 2);

        state.older_loading = true;
        apply_chat_window(
            &mut state,
            ChatWindow::Terminal {
                reason: "unreadable".into(),
            },
            false,
        );
        assert_eq!(state.fallback_reason.as_deref(), Some("unreadable"));
        assert!(!state.older_loading);
        assert_eq!(state.timeline, 3);
    }

    #[test]
    fn apply_older_page_covers_all_stop_and_continue_cases() {
        // 1. StopFoundUserPrompt
        let mut st = ChatTranscriptState {
            messages: vec![text_msg("m3", ChatRole::Assistant, "reply")],
            cursor: "c1".into(),
            older_cursor: Some("cur-1".into()),
            history_supported: true,
            older_loading: true,
            older_failed: false,
            timeline: 1,
            agent_key: Some("codex s1".into()),
            fallback_reason: None,
        };
        let step = apply_older_page(
            &mut st,
            "cur-1",
            transcript_win_raw(
                vec![text_msg("m1", ChatRole::User, "help")],
                "c1",
                false,
                Some(Some("cur-0")),
            ),
            1,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step, OlderPageStep::StopFoundUserPrompt);
        assert_eq!(st.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["m1", "m3"]);
        assert_eq!(st.older_cursor.as_deref(), Some("cur-0"));
        assert!(!st.older_loading);
        assert!(!st.older_failed);

        // 2. Continue on assistant/tool/harness, empty, and boundary-duplicate user prompt ID
        let mut st_cont = ChatTranscriptState {
            messages: vec![text_msg("dup-u", ChatRole::User, "already present")],
            cursor: "c1".into(),
            older_cursor: Some("cur-1".into()),
            history_supported: true,
            older_loading: true,
            older_failed: false,
            timeline: 1,
            agent_key: None,
            fallback_reason: None,
        };
        let step_a = apply_older_page(
            &mut st_cont,
            "cur-1",
            transcript_win_raw(
                vec![
                    text_msg("h1", ChatRole::User, "<system-reminder>hi</system-reminder>"),
                    text_msg("a1", ChatRole::Assistant, "thinking"),
                ],
                "c1",
                false,
                Some(Some("cur-2")),
            ),
            1,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(
            step_a,
            OlderPageStep::Continue {
                next_cursor: "cur-2".into()
            }
        );
        let step_empty = apply_older_page(
            &mut st_cont,
            "cur-2",
            transcript_win_raw(vec![], "c1", false, Some(Some("cur-3"))),
            2,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(
            step_empty,
            OlderPageStep::Continue {
                next_cursor: "cur-3".into()
            }
        );
        let step_dup = apply_older_page(
            &mut st_cont,
            "cur-3",
            transcript_win_raw(
                vec![text_msg("dup-u", ChatRole::User, "already present")],
                "c1",
                false,
                Some(Some("cur-4")),
            ),
            3,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(
            step_dup,
            OlderPageStep::Continue {
                next_cursor: "cur-4".into()
            }
        );

        // 3. StopBeginning
        let step_beg = apply_older_page(
            &mut st_cont,
            "cur-4",
            transcript_win_raw(
                vec![text_msg("a2", ChatRole::Assistant, "first")],
                "c1",
                false,
                Some(None),
            ),
            4,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_beg, OlderPageStep::StopBeginning);
        assert_eq!(st_cont.older_cursor, None);
        assert!(!st_cont.older_failed);
        assert!(!st_cont.older_loading);

        // 4. StopPageLimit
        let mut st_cap = ChatTranscriptState {
            older_cursor: Some("cur-15".into()),
            history_supported: true,
            older_loading: true,
            ..Default::default()
        };
        let step_cap = apply_older_page(
            &mut st_cap,
            "cur-15",
            transcript_win_raw(
                vec![text_msg("a16", ChatRole::Assistant, "step 16")],
                "c1",
                false,
                Some(Some("cur-16")),
            ),
            MAX_AUTO_OLDER_PAGES,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_cap, OlderPageStep::StopPageLimit);
        assert_eq!(st_cap.older_cursor.as_deref(), Some("cur-16"));
        assert!(!st_cap.older_failed);
        assert!(!st_cap.older_loading);

        // 4b. Finding a prompt on the 16th page still stops with StopFoundUserPrompt
        let mut st_cap_found = ChatTranscriptState {
            older_cursor: Some("cur-15".into()),
            history_supported: true,
            older_loading: true,
            ..Default::default()
        };
        let step_cap_found = apply_older_page(
            &mut st_cap_found,
            "cur-15",
            transcript_win_raw(
                vec![text_msg("u16", ChatRole::User, "prompt on 16th page")],
                "c1",
                false,
                Some(Some("cur-16")),
            ),
            MAX_AUTO_OLDER_PAGES,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_cap_found, OlderPageStep::StopFoundUserPrompt);
        assert!(!st_cap_found.older_failed);
        assert!(!st_cap_found.older_loading);

        // 5. StopReset when reset == true and older_cursor != requested_before
        let mut st_reset = ChatTranscriptState {
            messages: vec![text_msg("old", ChatRole::Assistant, "old")],
            cursor: "c1".into(),
            older_cursor: Some("cur-1".into()),
            history_supported: true,
            older_loading: true,
            timeline: 2,
            ..Default::default()
        };
        let step_reset = apply_older_page(
            &mut st_reset,
            "cur-1",
            transcript_win_raw(
                vec![text_msg("new-1", ChatRole::Assistant, "compacted")],
                "c9",
                true,
                Some(Some("cur-new")),
            ),
            1,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_reset, OlderPageStep::StopReset);
        assert_eq!(st_reset.messages.len(), 1);
        assert_eq!(st_reset.messages[0].id, "new-1");
        assert_eq!(st_reset.cursor, "c9");
        assert_eq!(st_reset.older_cursor.as_deref(), Some("cur-new"));
        assert_eq!(st_reset.timeline, 3);
        assert!(!st_reset.older_loading);
        assert!(!st_reset.older_failed);

        // 6. StopUnsupportedOldServer in history preview
        let mut st_old_hist = ChatTranscriptState {
            messages: vec![text_msg("keep", ChatRole::Assistant, "keep me")],
            cursor: "c1".into(),
            older_cursor: Some("cur-same".into()),
            history_supported: true,
            older_loading: true,
            ..Default::default()
        };
        let step_old_hist = apply_older_page(
            &mut st_old_hist,
            "cur-same",
            transcript_win_raw(
                vec![text_msg("keep", ChatRole::Assistant, "keep me")],
                "c1",
                true,
                Some(Some("cur-same")),
            ),
            1,
            MAX_AUTO_OLDER_PAGES,
            true,
        );
        assert_eq!(step_old_hist, OlderPageStep::StopUnsupportedOldServer);
        assert_eq!(st_old_hist.messages.len(), 1);
        assert_eq!(st_old_hist.messages[0].id, "keep");
        assert_eq!(st_old_hist.older_cursor, None);
        assert!(!st_old_hist.history_supported);
        assert!(!st_old_hist.older_failed);
        assert!(!st_old_hist.older_loading);

        // 7. StopFailed on Terminal & stalled cursor (and live reset with same cursor)
        let mut st_fail = ChatTranscriptState {
            messages: vec![text_msg("keep", ChatRole::Assistant, "keep")],
            cursor: "c1".into(),
            older_cursor: Some("cur-1".into()),
            history_supported: true,
            older_loading: true,
            ..Default::default()
        };
        let step_term = apply_older_page(
            &mut st_fail,
            "cur-1",
            ChatWindow::Terminal {
                reason: "unreadable".into(),
            },
            1,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_term, OlderPageStep::StopFailed);
        assert!(st_fail.older_failed);
        assert!(!st_fail.older_loading);
        assert_eq!(st_fail.fallback_reason, None);
        assert_eq!(st_fail.older_cursor.as_deref(), Some("cur-1"));
        assert_eq!(st_fail.messages.len(), 1);

        st_fail.older_loading = true;
        st_fail.older_failed = false;
        let step_stall = apply_older_page(
            &mut st_fail,
            "cur-1",
            transcript_win_raw(
                vec![text_msg("x", ChatRole::Assistant, "x")],
                "c1",
                false,
                Some(Some("cur-1")),
            ),
            1,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_stall, OlderPageStep::StopFailed);
        assert!(st_fail.older_failed);
        assert!(!st_fail.older_loading);

        st_fail.older_loading = true;
        st_fail.older_failed = false;
        let step_live_same = apply_older_page(
            &mut st_fail,
            "cur-1",
            transcript_win_raw(vec![], "c1", true, Some(Some("cur-1"))),
            1,
            MAX_AUTO_OLDER_PAGES,
            false,
        );
        assert_eq!(step_live_same, OlderPageStep::StopFailed);
        assert!(st_fail.older_failed);
        assert!(!st_fail.older_loading);
    }
}
