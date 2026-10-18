//! Host-owned history discovery. Clients send agent/session identities, never paths or commands.
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::detect::{self, DetectEnv};
use super::transcript::{Block, Role};
use crate::app::{AppState, bearer_ok};

pub const MAX_SESSIONS: usize = 1000;
const MAX_VISITS: usize = 20_000;
// Agent bootstrap instructions can exceed 32 KiB before the first real user prompt.
// Sampling remains bounded per file and runs only in the background indexer.
const HEAD_BYTES: u64 = 512 * 1024;
const TAIL_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub agent_id: String,
    pub agent_name: String,
    pub agent_session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub updated_at: u64,
    pub terminal_session: Option<u64>,
    pub can_resume: bool,
    pub unavailable_reason: Option<String>,
    #[serde(skip)]
    path: PathBuf,
}

#[derive(Default)]
pub struct Catalog {
    refreshed: Option<Instant>,
    indexing: bool,
    failed: bool,
    entries: Vec<Entry>,
    truncated: bool,
    skipped: usize,
    launched: HashMap<(String, String), u64>,
    agents: Vec<Value>,
}

struct Candidate {
    agent: &'static str,
    path: PathBuf,
    modified: u64,
}

fn roots(home: &Path) -> Vec<(&'static str, PathBuf, usize)> {
    vec![
        ("claude-code", home.join(".claude/projects"), 1),
        ("codex", home.join(".codex/sessions"), 3),
        ("codex", home.join(".codex/archived_sessions"), 0),
        ("pi", home.join(".pi/agent/sessions"), 1),
        ("grok", home.join(".grok/sessions"), 2),
        ("antigravity", home.join(".gemini/antigravity-cli/brain"), 4),
    ]
}

fn collect(
    agent: &'static str,
    dir: &Path,
    depth: usize,
    visits: &mut usize,
    files: &mut Vec<Candidate>,
    truncated: &mut bool,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for item in entries.flatten() {
        if *visits >= MAX_VISITS {
            *truncated = true;
            return;
        }
        *visits += 1;
        let Ok(kind) = item.file_type() else { continue };
        let path = item.path();
        // Never follow directory or file symlinks out of the known agent stores.
        if kind.is_dir() && depth > 0 {
            collect(agent, &path, depth - 1, visits, files, truncated);
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            let name = item.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("agent-")
                || (agent == "grok" && name != "updates.jsonl")
                || (agent == "antigravity" && name != "transcript_full.jsonl")
            {
                continue;
            }
            let modified = item
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            files.push(Candidate {
                agent,
                path,
                modified,
            });
        }
    }
}

fn safe_id(id: &str) -> bool {
    id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id.len() <= 160
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

fn short(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn prompt(value: &str) -> Option<String> {
    let value = value.trim();
    // Harness context is sometimes stored with role=user. It is not a session title.
    if value.starts_with("# AGENTS.md instructions")
        || value.starts_with("<environment_context>")
        || value.starts_with("<permissions instructions>")
        || value.starts_with("<developer_instructions>")
        || value.starts_with("<system-reminder>")
        || value.starts_with("<local-command-caveat>")
        || value.starts_with("<command-name>")
        || value.starts_with("This session is being continued from a previous conversation")
    {
        return None;
    }
    let value = value
        .split_once("## My request for Codex:")
        .map_or(value, |(_, p)| p);
    let value = short(value);
    (!value.is_empty()).then_some(value)
}

fn model_name(value: &Value) -> Option<String> {
    text(value, "model")
        .or_else(|| text(value, "modelId"))
        .or_else(|| value.get("model").and_then(|m| text(m, "id")))
        .map(|s| short(&s))
}

fn sample(path: &Path) -> std::io::Result<String> {
    sample_with_limits(path, HEAD_BYTES, TAIL_BYTES)
}

fn sample_with_limits(path: &Path, head_bytes: u64, tail_bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut head = Vec::new();
    (&mut file).take(head_bytes).read_to_end(&mut head)?;
    if len > head_bytes {
        // Discard the partial head/tail lines instead of parsing fragments as records.
        if let Some(end) = head.iter().rposition(|&b| b == b'\n') {
            head.truncate(end + 1);
        }
        let start = head_bytes.max(len.saturating_sub(tail_bytes));
        file.seek(SeekFrom::Start(start))?;
        let mut tail = Vec::new();
        file.take(tail_bytes).read_to_end(&mut tail)?;
        if start == 0 {
            head.extend_from_slice(&tail);
        } else if let Some(begin) = tail.iter().position(|&b| b == b'\n') {
            head.extend_from_slice(&tail[begin + 1..]);
        }
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

fn parse(candidate: Candidate, env: &DetectEnv) -> Option<Entry> {
    let adapter = super::find(candidate.agent)?;
    let source = sample(&candidate.path).ok()?;
    let mut id = None;
    let mut cwd = None;
    let mut title = None;
    let mut first_prompt = None;
    let mut description = None;
    let mut model = None;
    let mut sidechain = false;
    let mut valid_json = false;
    for line in source.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        valid_json = true;
        sidechain |= row.get("isSidechain").and_then(Value::as_bool) == Some(true);
        cwd = cwd.or_else(|| text(&row, "cwd"));
        match candidate.agent {
            "claude-code" => {
                id = id
                    .or_else(|| text(&row, "sessionId"))
                    .or_else(|| text(&row, "session_id"));
                if row.get("type").and_then(Value::as_str) == Some("custom-title") {
                    title = text(&row, "customTitle").or(title);
                }
                model = model_name(&row["message"]).or(model);
            }
            "codex" => {
                if id.is_none() && row.get("type").and_then(Value::as_str) == Some("session_meta") {
                    let meta = &row["payload"];
                    id = text(meta, "id").or_else(|| text(meta, "session_id")).or(id);
                    cwd = text(meta, "cwd").or(cwd);
                    title = text(meta, "title")
                        .or_else(|| text(meta, "thread_name"))
                        .or(title);
                }
                if row.get("type").and_then(Value::as_str) == Some("turn_context") {
                    model = model_name(&row["payload"]).or(model);
                }
            }
            "pi" => match row.get("type").and_then(Value::as_str) {
                Some("session") => id = text(&row, "id").or(id),
                Some("session_info") => title = text(&row, "name").or(title),
                Some("model_change") => model = model_name(&row).or(model),
                Some("message") => model = model_name(&row["message"]).or(model),
                _ => {}
            },
            "grok" => {
                id = id.or_else(|| {
                    row.pointer("/params/sessionId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            }
            _ => {}
        }
        // Parse each sampled record once into the shared message shape. Retain only
        // bounded summaries, never transcript bodies in the catalog.
        for message in adapter.parse_transcript(line, 0) {
            let joined = message
                .blocks
                .into_iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(value) = prompt(&joined) {
                if message.role == Role::User && first_prompt.is_none() {
                    first_prompt = Some(value.clone());
                }
                if message.role != Role::System {
                    description = Some(value);
                }
            }
        }
        // Older Codex event-only records have no response_item counterpart.
        if candidate.agent == "codex" && row["type"] == "event_msg" {
            let payload = &row["payload"];
            if matches!(
                payload["type"].as_str(),
                Some("user_message" | "agent_message")
            ) && let Some(value) = text(payload, "message").and_then(|s| prompt(&s))
            {
                if payload["type"] == "user_message" && first_prompt.is_none() {
                    first_prompt = Some(value.clone());
                }
                description = Some(value);
            }
        }
    }
    if sidechain || !valid_json {
        return None;
    }
    if candidate.agent == "grok" {
        // Grok keeps cwd/title alongside the ACP update log.
        if let Some(dir) = candidate.path.parent() {
            for name in ["session.json", "metadata.json", "meta.json"] {
                if let Some(v) = sample(&dir.join(name))
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                {
                    cwd = text(&v, "cwd").or(cwd);
                    title = text(&v, "title").or(title);
                    model = model_name(&v).or(model);
                }
            }
            id = id.or_else(|| dir.file_name().map(|s| s.to_string_lossy().into_owned()));
            // Older Grok stores have no metadata file; the project directory is URL encoded.
            if cwd.is_none() {
                cwd = dir
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|name| decode_project(&name.to_string_lossy()));
            }
        }
    }
    if candidate.agent == "antigravity" {
        id = candidate
            .path
            .ancestors()
            .nth(3)
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned());
    }
    let id = id.or_else(|| {
        (candidate.agent == "claude-code").then(|| {
            candidate
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
    })?;
    if !safe_id(&id) {
        return None;
    }
    let title = title
        .or(first_prompt)
        .map(|s| short(&s))
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let mut entry = Entry {
        agent_id: adapter.id().into(),
        agent_name: adapter.name().into(),
        agent_session_id: id,
        title,
        description,
        model,
        cwd,
        updated_at: candidate.modified,
        terminal_session: None,
        can_resume: false,
        unavailable_reason: None,
        path: candidate.path,
    };
    entry.unavailable_reason = resume_command(&entry, env).err().map(str::to_string);
    entry.can_resume = entry.unavailable_reason.is_none();
    Some(entry)
}

fn decode_project(name: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut index = 0;
    while index < name.len() {
        if name.as_bytes()[index] == b'%' {
            let value = u8::from_str_radix(name.get(index + 1..index + 3)?, 16).ok()?;
            bytes.push(value);
            index += 3;
        } else {
            bytes.push(name.as_bytes()[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    Path::new(&path).is_absolute().then_some(path)
}

pub fn scan(env: &DetectEnv) -> Catalog {
    let mut files = Vec::new();
    let mut visits = 0;
    let mut truncated = false;
    for (agent, root, depth) in roots(&env.home) {
        collect(agent, &root, depth, &mut visits, &mut files, &mut truncated);
    }
    files.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut seen = HashSet::new();
    let agents = super::registry()
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id(), "name": a.name(), "installed": a.detect(env).installed,
            })
        })
        .collect();
    let mut catalog = Catalog {
        refreshed: Some(Instant::now()),
        truncated,
        agents,
        ..Default::default()
    };
    for file in files {
        match parse(file, env) {
            Some(entry)
                if seen.insert((entry.agent_id.clone(), entry.agent_session_id.clone())) =>
            {
                if catalog.entries.len() == MAX_SESSIONS {
                    catalog.truncated = true;
                    break;
                }
                catalog.entries.push(entry)
            }
            Some(_) => {}
            None => catalog.skipped += 1,
        }
    }
    // Codex /rename is persisted in its index, independently of the rollout transcript.
    if let Ok(index) = sample_with_limits(
        &env.home.join(".codex/session_index.jsonl"),
        1024 * 1024,
        1024 * 1024,
    ) {
        let titles: HashMap<_, _> = index
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|v| Some((text(&v, "id")?, text(&v, "thread_name")?)))
            .collect();
        for entry in catalog.entries.iter_mut().filter(|e| e.agent_id == "codex") {
            if let Some(title) = titles.get(&entry.agent_session_id) {
                entry.title = short(title);
            }
        }
    }
    // CLI prompt history can recover an opening request when a very large
    // bootstrap record lies beyond the transcript sampling budget.
    if let Ok(index) =
        sample_with_limits(&env.home.join(".codex/history.jsonl"), 0, 2 * 1024 * 1024)
    {
        let mut prompts = HashMap::new();
        for row in index
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            if let (Some(id), Some(value)) = (
                text(&row, "session_id"),
                text(&row, "text").and_then(|s| prompt(&s)),
            ) {
                prompts.entry(id).or_insert(value);
            }
        }
        for entry in catalog
            .entries
            .iter_mut()
            .filter(|e| e.agent_id == "codex" && e.title.is_empty())
        {
            if let Some(value) = prompts.get(&entry.agent_session_id) {
                entry.title = value.clone();
                if entry.description.is_none() {
                    entry.description = Some(value.clone());
                }
            }
        }
    }
    catalog
}

fn resume_command(entry: &Entry, env: &DetectEnv) -> Result<Vec<String>, &'static str> {
    if !safe_id(&entry.agent_session_id) {
        return Err("not-found");
    }
    let (cli, flag) = match entry.agent_id.as_str() {
        "claude-code" => ("claude", "--resume"),
        "codex" => ("codex", "resume"),
        "pi" => ("pi", "--session"),
        "grok" => ("grok", "--resume"),
        // The desktop Antigravity vault is not an agy CLI resume store.
        "antigravity" => ("agy", "--conversation"),
        _ => return Err("unsupported-agent"),
    };
    let cwd = entry
        .cwd
        .as_deref()
        .filter(|p| Path::new(p).is_dir())
        .ok_or("missing-directory")?;
    if cwd.is_empty() {
        return Err("missing-directory");
    }
    let executable = detect::on_path(env, cli).ok_or("missing-cli")?;
    let target = if entry.agent_id == "pi" {
        entry.path.to_string_lossy().into_owned()
    } else {
        entry.agent_session_id.clone()
    };
    let command = vec![
        executable.to_string_lossy().into_owned(),
        flag.into(),
        target,
    ];
    // npm's Windows .cmd launchers need an interpreter. Quote each argument as a literal.
    #[cfg(windows)]
    if executable
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
    {
        let quoted = command
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(vec![
            "powershell.exe".into(),
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!("& {quoted}"),
        ]);
    }
    Ok(command)
}

/// Start one background scan; reads keep using the previous immutable index until it is ready.
pub fn start_index(state: &AppState, refresh: bool) {
    if !state.agents.transcript_enabled {
        return;
    }
    start_index_with_env(state, refresh, DetectEnv::current());
}

fn start_index_with_env(state: &AppState, refresh: bool, env: DetectEnv) {
    if !state.agents.transcript_enabled {
        return;
    }
    let store = state.agent_sessions.clone();
    {
        let mut cache = store.history.lock().unwrap();
        if cache.indexing || (!refresh && (cache.refreshed.is_some() || cache.failed)) {
            return;
        }
        cache.indexing = true;
        cache.failed = false;
    }
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || scan(&env)).await;
        let mut cache = store.history.lock().unwrap();
        match result {
            Ok(mut next) => {
                next.launched = std::mem::take(&mut cache.launched);
                *cache = next;
            }
            Err(error) => {
                tracing::error!(%error, "history indexing failed");
                cache.indexing = false;
                cache.failed = true;
            }
        }
    });
}

fn live_session(state: &AppState, entry: &Entry) -> Option<u64> {
    let live = |id| state.manager.get(id).is_some_and(|s| !s.has_exited());
    let current = state
        .agent_sessions
        .snapshot(&entry.agent_id, &entry.agent_session_id)
        .and_then(|s| s.session)
        .filter(|&id| live(id));
    if current.is_some() {
        return current;
    }
    let remembered = state
        .agent_sessions
        .history
        .lock()
        .unwrap()
        .launched
        .get(&(entry.agent_id.clone(), entry.agent_session_id.clone()))
        .copied();
    remembered.filter(|&id| {
        live(id)
            && !state.agent_sessions.list().iter().any(|s| {
                s.session == Some(id)
                    && s.agent_id == entry.agent_id
                    && s.agent_session_id != entry.agent_session_id
            })
    })
}

fn error(code: StatusCode, reason: &str) -> Response {
    (code, Json(serde_json::json!({ "error": reason }))).into_response()
}

fn guard(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if !bearer_ok(headers, &state.token) {
        return Some(error(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    if !state.agents.transcript_enabled {
        return Some(error(StatusCode::FORBIDDEN, "disabled"));
    }
    None
}

#[derive(Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub refresh: bool,
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Response {
    if let Some(response) = guard(&state, &headers) {
        return response;
    }
    start_index(&state, q.refresh);
    let query = q.q.trim().to_lowercase();
    let matches = |e: &&Entry| {
        (q.agent_id.is_empty() || e.agent_id == q.agent_id)
            && (q.workspace.is_empty() || e.cwd.as_deref() == Some(q.workspace.as_str()))
            && (query.is_empty()
                || format!(
                    "{} {} {} {} {} {}",
                    e.agent_name,
                    e.title,
                    e.cwd.as_deref().unwrap_or_default(),
                    e.agent_session_id,
                    e.description.as_deref().unwrap_or_default(),
                    e.model.as_deref().unwrap_or_default()
                )
                .to_lowercase()
                .contains(&query))
    };
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    // Clone only the requested page, not all indexed sessions or conversation bodies.
    let (mut page, total, agents, workspaces, truncated, skipped, indexing) = {
        let cache = state.agent_sessions.history.lock().unwrap();
        if cache.failed && cache.refreshed.is_none() {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "unreadable");
        }
        let total = cache.entries.iter().filter(matches).count();
        let mut workspaces: Vec<_> = cache
            .entries
            .iter()
            .filter(|e| q.agent_id.is_empty() || e.agent_id == q.agent_id)
            .filter_map(|e| e.cwd.clone())
            .collect();
        workspaces.sort();
        workspaces.dedup();
        let page: Vec<_> = cache
            .entries
            .iter()
            .filter(matches)
            .skip(q.offset)
            .take(limit)
            .cloned()
            .collect();
        (
            page,
            total,
            cache.agents.clone(),
            workspaces,
            cache.truncated,
            cache.skipped,
            cache.indexing,
        )
    };
    for entry in &mut page {
        entry.terminal_session = live_session(&state, entry);
    }
    Json(serde_json::json!({ "sessions": page, "total": total, "agents": agents, "workspaces": workspaces,
        "next_offset": (q.offset.saturating_add(limit) < total).then_some(q.offset.saturating_add(limit)),
        "truncated": truncated, "skipped": skipped, "indexing": indexing, "max_sessions": MAX_SESSIONS })).into_response()
}

#[derive(Clone, Deserialize)]
pub struct Key {
    pub agent_id: String,
    pub agent_session_id: String,
}

fn find_entry(state: &AppState, key: &Key) -> Option<Entry> {
    state
        .agent_sessions
        .history
        .lock()
        .unwrap()
        .entries
        .iter()
        .find(|e| e.agent_id == key.agent_id && e.agent_session_id == key.agent_session_id)
        .cloned()
}

pub async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(key): Query<Key>,
) -> Response {
    if let Some(response) = guard(&state, &headers) {
        return response;
    }
    let Some(entry) = find_entry(&state, &key) else {
        return error(StatusCode::NOT_FOUND, "not-found");
    };
    let Some(adapter) = super::find(&entry.agent_id) else {
        return error(StatusCode::NOT_FOUND, "not-found");
    };
    match tokio::task::spawn_blocking(move || {
        super::transcript::read_window(&entry.path, None, &|t, b| adapter.parse_transcript(t, b))
    })
    .await
    {
        Ok(Ok(window)) => {
            let mut body = serde_json::to_value(window).unwrap_or_default();
            body["source"] = Value::String("transcript".into());
            Json(body).into_response()
        }
        _ => error(StatusCode::CONFLICT, "unreadable"),
    }
}

pub async fn open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(key): Json<Key>,
) -> Response {
    open_with_env(state, headers, key, DetectEnv::current()).await
}

async fn open_with_env(state: AppState, headers: HeaderMap, key: Key, env: DetectEnv) -> Response {
    if let Some(response) = guard(&state, &headers) {
        return response;
    }
    // Serialize the live check and spawn across clients, including the time before hooks arrive.
    let _opening = state.agent_sessions.history_open.lock().await;
    // A live terminal can still be opened after its transcript has been removed.
    if let Some(session) = state
        .agent_sessions
        .snapshot(&key.agent_id, &key.agent_session_id)
        .and_then(|s| s.session)
        .and_then(|id| state.manager.get(id))
        .filter(|s| !s.has_exited())
    {
        return Json(serde_json::json!({ "session": session.info(), "reused": true }))
            .into_response();
    }
    let Some(entry) = find_entry(&state, &key) else {
        return error(StatusCode::NOT_FOUND, "not-found");
    };
    if let Some(session) = live_session(&state, &entry).and_then(|id| state.manager.get(id)) {
        return Json(serde_json::json!({ "session": session.info(), "reused": true }))
            .into_response();
    }
    let command = match resume_command(&entry, &env) {
        Ok(command) => command,
        Err(reason) => return error(StatusCode::CONFLICT, reason),
    };
    if !entry.path.is_file() {
        return error(StatusCode::NOT_FOUND, "not-found");
    }
    let manager = state.manager.clone();
    let (name, cwd) = (
        format!("{} · {}", entry.agent_name, entry.title),
        entry.cwd.clone(),
    );
    let result =
        tokio::task::spawn_blocking(move || manager.create(Some(name), Some(command), cwd)).await;
    match result {
        Ok(Ok(session)) => {
            state
                .agent_sessions
                .history
                .lock()
                .unwrap()
                .launched
                .insert(
                    (entry.agent_id.clone(), entry.agent_session_id.clone()),
                    session.id,
                );
            let snapshot = state.agent_sessions.apply(
                &entry.agent_id,
                &entry.agent_session_id,
                Some(session.id),
                super::store::Update {
                    cwd: entry.cwd,
                    title: Some(entry.title),
                    transcript: Some(entry.path.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            );
            state.manager.publish(snapshot.to_msg());
            Json(serde_json::json!({ "session": session.info(), "reused": false })).into_response()
        }
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "spawn-failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn fixture(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn metadata_uses_human_prompts_after_large_bootstrap_and_latest_conversation_text() {
        let home = tempfile::tempdir().unwrap();
        let rows = [
            serde_json::json!({"type":"session_meta","payload":{"id":"codex-meta","cwd":"/work/one"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("# AGENTS.md instructions\n{}", "x".repeat(40_000))}]}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>host</environment_context>"}]}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Fix session history titles"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-test"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Titles now use the first human request."}]}}),
        ];
        let source = rows.iter().map(|v| format!("{v}\n")).collect::<String>();
        fixture(
            home.path(),
            ".codex/sessions/2026/10/18/rollout-meta.jsonl",
            &source,
        );
        let env = DetectEnv {
            home: home.path().into(),
            path: None,
        };
        let entry = scan(&env).entries.remove(0);
        assert_eq!(entry.title, "Fix session history titles");
        assert_eq!(entry.model.as_deref(), Some("gpt-test"));
        assert_eq!(
            entry.description.as_deref(),
            Some("Titles now use the first human request.")
        );
        fixture(
            home.path(),
            ".codex/session_index.jsonl",
            "{\"id\":\"codex-meta\",\"thread_name\":\"My custom title\"}\n",
        );
        assert_eq!(scan(&env).entries[0].title, "My custom title");
    }

    #[test]
    fn prompt_history_recovers_titles_beyond_the_sampling_budget() {
        let home = tempfile::tempdir().unwrap();
        let padding = "x".repeat(HEAD_BYTES as usize + TAIL_BYTES as usize);
        fixture(
            home.path(),
            ".codex/sessions/2026/10/18/rollout-large.jsonl",
            &format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"large\"}}}}\n{{\"type\":\"instructions\",\"text\":\"{padding}\"}}\n"
            ),
        );
        fixture(
            home.path(),
            ".codex/history.jsonl",
            "{\"session_id\":\"large\",\"text\":\"Recovered human request\"}\n",
        );
        let env = DetectEnv {
            home: home.path().into(),
            path: None,
        };
        let entry = scan(&env).entries.remove(0);
        assert_eq!(entry.title, "Recovered human request");
        assert_eq!(
            entry.description.as_deref(),
            Some("Recovered human request")
        );
    }

    #[test]
    fn claude_and_pi_keep_titles_models_and_text_separate_from_tool_output() {
        let home = tempfile::tempdir().unwrap();
        fixture(
            home.path(),
            ".claude/projects/p/claude.jsonl",
            concat!(
                "{\"sessionId\":\"claude\",\"type\":\"user\",\"message\":{\"content\":\"First Claude prompt\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-test\",\"content\":[{\"type\":\"text\",\"text\":\"Latest Claude answer\"}]}}\n",
                "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"Not a description\"}]}}\n"
            ),
        );
        fixture(
            home.path(),
            ".pi/agent/sessions/p/pi.jsonl",
            concat!(
                "{\"type\":\"session\",\"id\":\"pi-meta\"}\n",
                "{\"type\":\"model_change\",\"modelId\":\"pi-test\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"First Pi prompt\"}}\n"
            ),
        );
        let catalog = scan(&DetectEnv {
            home: home.path().into(),
            path: None,
        });
        let claude = catalog
            .entries
            .iter()
            .find(|e| e.agent_id == "claude-code")
            .unwrap();
        assert_eq!(claude.title, "First Claude prompt");
        assert_eq!(claude.model.as_deref(), Some("claude-test"));
        assert_eq!(claude.description.as_deref(), Some("Latest Claude answer"));
        let pi = catalog.entries.iter().find(|e| e.agent_id == "pi").unwrap();
        assert_eq!(pi.title, "First Pi prompt");
        assert_eq!(pi.model.as_deref(), Some("pi-test"));
    }

    #[tokio::test]
    async fn workspace_filter_is_exact_and_applied_before_pagination() {
        let home = tempfile::tempdir().unwrap();
        for (id, cwd) in [
            ("one", "/work/one"),
            ("two", "/work/two"),
            ("child", "/work/one/child"),
        ] {
            fixture(
                home.path(),
                &format!(".codex/sessions/2026/10/18/{id}.jsonl"),
                &format!(
                    "{}\n",
                    serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":cwd}})
                ),
            );
        }
        let state = state();
        *state.agent_sessions.history.lock().unwrap() = scan(&DetectEnv {
            home: home.path().into(),
            path: None,
        });
        let response = list(
            State(state),
            headers(),
            Query(ListQuery {
                workspace: "/work/one".into(),
                limit: Some(1),
                ..Default::default()
            }),
        )
        .await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["total"], 1);
        assert_eq!(body["sessions"][0]["agent_session_id"], "one");
        assert_eq!(body["workspaces"].as_array().unwrap().len(), 3);
        assert!(body["next_offset"].is_null());
    }

    #[test]
    fn discovers_stores_deduplicates_and_skips_subagents_and_corruption() {
        let home = tempfile::tempdir().unwrap();
        fixture(
            home.path(),
            ".claude/projects/project/claude-1.jsonl",
            "{\"sessionId\":\"claude-1\",\"cwd\":\"/repo\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"First prompt\"}}\n{\"type\":\"custom-title\",\"customTitle\":\"Named Claude\"}\n",
        );
        fixture(
            home.path(),
            ".claude/projects/project/agent-child.jsonl",
            "{}\n",
        );
        fixture(
            home.path(),
            ".claude/projects/project/broken.jsonl",
            "broken\n",
        );
        let codex =
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-1\",\"cwd\":\"/repo\"}}\n";
        fixture(
            home.path(),
            ".codex/sessions/2026/10/16/rollout-codex-1.jsonl",
            codex,
        );
        fixture(
            home.path(),
            ".codex/archived_sessions/rollout-codex-1.jsonl",
            codex,
        );
        fixture(
            home.path(),
            ".codex/session_index.jsonl",
            "{\"id\":\"codex-1\",\"thread_name\":\"Renamed Codex\"}\n",
        );
        fixture(
            home.path(),
            ".pi/agent/sessions/project/stamp_pi-1.jsonl",
            "{\"type\":\"session\",\"id\":\"pi-1\",\"cwd\":\"/repo\"}\n{\"type\":\"session_info\",\"name\":\"Named Pi\"}\n",
        );
        fixture(
            home.path(),
            ".grok/sessions/project/grok-1/updates.jsonl",
            "{\"params\":{\"sessionId\":\"grok-1\"}}\n",
        );
        fixture(
            home.path(),
            ".grok/sessions/project/grok-1/metadata.json",
            "{\"title\":\"Named Grok\",\"cwd\":\"/repo\"}",
        );
        fixture(
            home.path(),
            ".gemini/antigravity-cli/brain/agy-1/.system_generated/logs/transcript_full.jsonl",
            "{}\n",
        );
        let env = DetectEnv {
            home: home.path().into(),
            path: None,
        };
        let result = scan(&env);
        assert_eq!(result.entries.len(), 5);
        assert_eq!(result.skipped, 1);
        assert!(!result.truncated);
        let claude = result
            .entries
            .iter()
            .find(|e| e.agent_id == "claude-code")
            .unwrap();
        assert_eq!(claude.title, "Named Claude");
        assert_eq!(claude.cwd.as_deref(), Some("/repo"));
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.agent_id == "grok")
                .unwrap()
                .title,
            "Named Grok"
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.agent_id == "codex")
                .unwrap()
                .title,
            "Renamed Codex"
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.agent_id == "antigravity")
                .unwrap()
                .agent_session_id,
            "agy-1"
        );
        assert!(result.entries.iter().all(|e| !e.can_resume));
    }

    #[test]
    fn resume_requires_an_existing_cwd_and_cli_and_pi_uses_its_file() {
        let home = tempfile::tempdir().unwrap();
        fixture(
            home.path(),
            ".pi/agent/sessions/project/stamp_pi-1.jsonl",
            &format!(
                "{{\"type\":\"session\",\"id\":\"pi-1\",\"cwd\":{}}}\n",
                serde_json::to_string(&home.path()).unwrap()
            ),
        );
        let mut env = DetectEnv {
            home: home.path().into(),
            path: None,
        };
        let entry = scan(&env).entries.remove(0);
        assert_eq!(resume_command(&entry, &env), Err("missing-cli"));
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(
            bin.join(if cfg!(windows) { "pi.cmd" } else { "pi" }),
            "unused fixture",
        )
        .unwrap();
        env.path = Some(std::env::join_paths([bin]).unwrap());
        let command = resume_command(&entry, &env).unwrap().join(" ");
        assert!(command.contains("--session"));
        assert!(command.contains("stamp_pi-1.jsonl"));
        let mut invalid = entry.clone();
        invalid.agent_session_id = "--eval=bad".into();
        assert_eq!(resume_command(&invalid, &env), Err("not-found"));
        invalid.agent_session_id = "--help".into();
        assert_eq!(resume_command(&invalid, &env), Err("not-found"));
        invalid = entry;
        invalid.cwd = Some(home.path().join("gone").to_string_lossy().into_owned());
        assert_eq!(resume_command(&invalid, &env), Err("missing-directory"));
    }

    fn state() -> AppState {
        AppState {
            manager: crate::session::manager::SessionManager::new(1 << 20, Default::default()),
            token: "test".into(),
            launchers: Arc::new(Vec::new()),
            listen_port: 0,
            agents: Default::default(),
            agent_sessions: Arc::new(super::super::store::AgentSessionStore::new()),
        }
    }

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", "Bearer test".parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn history_is_authenticated_and_obeys_transcript_gate() {
        let mut state = state();
        assert_eq!(
            list(
                State(state.clone()),
                HeaderMap::new(),
                Query(ListQuery::default())
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        state.agents.transcript_enabled = false;
        assert_eq!(
            list(State(state.clone()), headers(), Query(ListQuery::default()))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert!(
            state
                .agent_sessions
                .history
                .lock()
                .unwrap()
                .refreshed
                .is_none()
        );
        let key = Key {
            agent_id: "codex".into(),
            agent_session_id: "missing".into(),
        };
        assert_eq!(
            open(State(state), headers(), Json(key)).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn existing_terminal_wins_even_without_a_transcript_or_cli() {
        let state = state();
        let session = state.manager.create(None, None, None).unwrap();
        state
            .agent_sessions
            .apply("codex", "known", Some(session.id), Default::default());
        let response = open(
            State(state.clone()),
            headers(),
            Json(Key {
                agent_id: "codex".into(),
                agent_session_id: "known".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["session"]["id"], session.id);
        assert_eq!(body["reused"], true);
        assert_eq!(state.manager.list().len(), 1);
        assert!(
            state
                .agent_sessions
                .history
                .lock()
                .unwrap()
                .refreshed
                .is_none()
        );
        state.manager.kill(session.id);
    }

    #[tokio::test]
    async fn concurrent_resume_creates_one_real_pty_with_literal_arguments() {
        let home = tempfile::Builder::new()
            .prefix("history space's ")
            .tempdir()
            .unwrap();
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let record = home.path().join("arguments.txt");
        #[cfg(windows)]
        fixture(
            &bin,
            "claude.cmd",
            &format!(
                "@echo off\r\necho %~1 %~2 > \"{0}\"\r\ncd >> \"{0}\"\r\nset /p waiting=\r\n",
                record.display()
            ),
        );
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            fixture(
                &bin,
                "claude",
                &format!(
                    "#!/bin/sh\nprintf '%s %s\\n' \"$1\" \"$2\" > '{0}'\npwd >> '{0}'\nread waiting\n",
                    record.to_string_lossy().replace('\'', "'\\''")
                ),
            );
            fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fixture(
            home.path(),
            ".claude/projects/project/session-1.jsonl",
            &format!(
                "{{\"sessionId\":\"session-1\",\"cwd\":{},\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"Resume test\"}}}}\n",
                serde_json::to_string(home.path()).unwrap()
            ),
        );
        let env = || DetectEnv {
            home: home.path().into(),
            path: Some(std::env::join_paths([&bin]).unwrap()),
        };
        let state = state();
        *state.agent_sessions.history.lock().unwrap() = scan(&env());
        let key = Key {
            agent_id: "claude-code".into(),
            agent_session_id: "session-1".into(),
        };
        let (a, b) = tokio::join!(
            open_with_env(state.clone(), headers(), key.clone(), env()),
            open_with_env(state.clone(), headers(), key, env()),
        );
        let codes = (a.status(), b.status());
        let a: Value =
            serde_json::from_slice(&axum::body::to_bytes(a.into_body(), 4096).await.unwrap())
                .unwrap();
        let b: Value =
            serde_json::from_slice(&axum::body::to_bytes(b.into_body(), 4096).await.unwrap())
                .unwrap();
        for _ in 0..80 {
            if record.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let arguments = fs::read_to_string(record).unwrap_or_default();
        let sessions = state.manager.list();
        for session in &sessions {
            state.manager.kill(session.id);
        }
        assert_eq!(codes, (StatusCode::OK, StatusCode::OK), "{a} / {b}");
        assert_eq!(sessions.len(), 1);
        assert_eq!(a["session"]["id"], b["session"]["id"]);
        assert_eq!(a["reused"], false);
        assert_eq!(b["reused"], true);
        assert!(arguments.contains("--resume"), "{arguments:?}");
        assert!(arguments.contains("session-1"), "{arguments:?}");
        let actual_cwd = arguments.lines().nth(1).unwrap_or_default().trim();
        assert_eq!(
            fs::canonicalize(actual_cwd).unwrap(),
            fs::canonicalize(home.path()).unwrap()
        );
    }

    #[tokio::test]
    async fn search_pagination_and_live_links_are_resolved_on_the_server() {
        let home = tempfile::tempdir().unwrap();
        for index in 0..3 {
            fixture(
                home.path(),
                &format!(".codex/sessions/2026/10/16/rollout-{index}.jsonl"),
                &format!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"codex-{index}\",\"cwd\":\"/repo\"}}}}\n"
                ),
            );
        }
        let state = state();
        *state.agent_sessions.history.lock().unwrap() = scan(&DetectEnv {
            home: home.path().into(),
            path: None,
        });
        let response = list(
            State(state),
            headers(),
            Query(ListQuery {
                q: "REPO".into(),
                agent_id: "codex".into(),
                limit: Some(1),
                offset: 1,
                ..Default::default()
            }),
        )
        .await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["total"], 3);
        assert_eq!(body["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(body["next_offset"], 2);
        assert!(body["sessions"][0].get("path").is_none());
    }

    #[tokio::test]
    async fn switching_agent_sessions_does_not_reuse_a_stale_terminal_link() {
        let state = state();
        let session = state.manager.create(None, None, None).unwrap();
        let entry = Entry {
            agent_id: "codex".into(),
            agent_name: "Codex".into(),
            agent_session_id: "old".into(),
            title: "Old conversation".into(),
            description: None,
            model: None,
            cwd: None,
            updated_at: 0,
            terminal_session: None,
            can_resume: false,
            unavailable_reason: None,
            path: PathBuf::new(),
        };
        state
            .agent_sessions
            .history
            .lock()
            .unwrap()
            .launched
            .insert(("codex".into(), "old".into()), session.id);
        state
            .agent_sessions
            .apply("codex", "old", Some(session.id), Default::default());
        let before = live_session(&state, &entry);
        state.agent_sessions.apply_superseding(
            "codex",
            "new",
            Some(session.id),
            Default::default(),
        );
        let after = live_session(&state, &entry);
        state.manager.kill(session.id);
        assert_eq!(before, Some(session.id));
        assert_eq!(after, None);
    }

    #[test]
    fn index_limit_counts_distinct_sessions_instead_of_candidate_files() {
        let home = tempfile::tempdir().unwrap();
        for index in 0..1002 {
            let body = format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"codex-{index}\",\"cwd\":\"/repo\"}}}}\n"
            );
            fixture(
                home.path(),
                &format!(".codex/sessions/2026/10/16/rollout-{index}.jsonl"),
                &body,
            );
            fixture(
                home.path(),
                &format!(".codex/archived_sessions/rollout-{index}.jsonl"),
                &body,
            );
        }
        let catalog = scan(&DetectEnv {
            home: home.path().into(),
            path: None,
        });
        assert_eq!(catalog.entries.len(), MAX_SESSIONS);
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|e| &e.agent_session_id)
                .collect::<HashSet<_>>()
                .len(),
            MAX_SESSIONS
        );
        assert!(catalog.truncated);
    }

    #[tokio::test]
    async fn background_indexing_does_not_wait_for_a_client_or_rescan_each_page() {
        let home = tempfile::tempdir().unwrap();
        fixture(
            home.path(),
            ".codex/sessions/2026/10/16/rollout-one.jsonl",
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"one\",\"cwd\":\"/repo\"}}\n",
        );
        let state = state();
        start_index_with_env(
            &state,
            false,
            DetectEnv {
                home: home.path().into(),
                path: None,
            },
        );
        assert!(state.agent_sessions.history.lock().unwrap().indexing);
        let response = list(State(state.clone()), headers(), Query(ListQuery::default())).await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["indexing"], true);
        for _ in 0..200 {
            if !state.agent_sessions.history.lock().unwrap().indexing {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let indexed_at = state.agent_sessions.history.lock().unwrap().refreshed;
        assert!(indexed_at.is_some());
        // Reading pages uses this completed snapshot even if files have changed on disk.
        fixture(
            home.path(),
            ".codex/sessions/2026/10/16/rollout-two.jsonl",
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"two\",\"cwd\":\"/repo\"}}\n",
        );
        let response = list(State(state.clone()), headers(), Query(ListQuery::default())).await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["total"], 1);
        assert_eq!(body["max_sessions"], 1000);
        assert_eq!(body["indexing"], false);
        assert_eq!(
            state.agent_sessions.history.lock().unwrap().refreshed,
            indexed_at
        );
        // Explicit refresh runs asynchronously too, and swaps the completed index atomically.
        start_index_with_env(
            &state,
            true,
            DetectEnv {
                home: home.path().into(),
                path: None,
            },
        );
        assert_eq!(
            state.agent_sessions.history.lock().unwrap().entries.len(),
            1
        );
        for _ in 0..200 {
            if !state.agent_sessions.history.lock().unwrap().indexing {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            state.agent_sessions.history.lock().unwrap().entries.len(),
            2
        );
    }
}
