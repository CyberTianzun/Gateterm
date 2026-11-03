# `server::agent` Adapter Architecture & Protocol Family Kit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `crates/server/src/agent` into a modular adapter architecture with a shared protocol-family kit (`kit.rs`), a zero-allocation static registry, and isolated `adapters/<agent>/` submodules so core modules contain zero hardcoded agent ID branches.

**Architecture:** Extend `AgentAdapter` to cover all 7 integration surfaces (identity, detection/install/activation, command wrapping, live hooks/blocking decisions, daemon process exit watching, transcript parsing, and history discovery/resume). Extract shared 3-tier detection, JSON `hooks.<Event>` reconciliation, and Claude-style permission decision rendering into `kit.rs`, and move all 5 concrete agents into `crates/server/src/agent/adapters/<agent>/` while preserving backwards-compatible `pub use` re-exports.

**Tech Stack:** Rust (Edition 2024), `axum`, `serde`, `serde_json`, `sysinfo`, `tokio`.

**Spec:** `docs/superpowers/specs/2026-11-03-agent-adapter-refactor-design.md`

## Global Constraints

- Discovery (`detect`, `scan`, `history`, `messages`) must remain strictly read-only; disk writes occur only via `apply::apply` under explicit user actions.
- Wire contracts (`/api/agents/*`, `/hook/*`, `ServerMsg::AgentEvent`) must not change.
- Public module paths used by `crates/server/tests/agent_*.rs` and `clients/desktop-web/tests/hook_cli.rs` (`claude_transcript`, `codex_transcript`, `grok_transcript`, `pi_transcript`, `antigravity_transcript`, `codex_trust`, `claude_code`, `codex`, `pi`, `grok`, `antigravity`, and `find(id).unwrap().as_ref()`) must remain source-compatible via re-exports and `impl AsRef<dyn AgentAdapter> for dyn AgentAdapter`.
- Core framework files (`mod.rs`, `routes.rs`, `store.rs`, `edit.rs`, `apply.rs`, `transcript.rs`, `history.rs`, `process.rs`, `hook_cli.rs`, `detect.rs`) must not branch on specific agent IDs (`"claude-code"`, `"codex"`, `"pi"`, `"grok"`, `"antigravity"`).

## Review Focus

- `find(id)` returning `Option<&'static dyn AgentAdapter>` must still allow callers to write both `plan_purge(a, &ctx)` and `plan_purge(a.as_ref(), &ctx)` without breaking existing integration tests.
- `startup_warnings` for `NeedsActivation` must use each adapter's own `activation_hint()` rather than a partial hardcoded match table in `mod.rs`.
- `hook_cli::command_for_executable` must fall back to `CommandWrap::StandardQuoted` when called with an unknown agent ID so unit tests with synthetic IDs never panic.
- `history::scan` must preserve exact deduplication order (newest `modified` first, then lexicographical `path`) when collecting candidates across all registered adapters' `history_roots`.
- `JsonHooksSpec` conflict detection in `kit.rs` must preserve the distinction between Claude Code (only flags foreign `type == "http"` hooks on blocking events) and Codex (flags any foreign hook on blocking events).

---

### Task 1: Static Registry & Shared Protocol Family Kit (`kit.rs`)

**Files:**
- Create: `crates/server/src/agent/kit.rs`
- Modify: `crates/server/src/agent/mod.rs`
- Modify: `crates/server/src/agent/routes.rs`

**Interfaces:**
- Produces in `crates/server/src/agent/kit.rs`:
  - `pub enum ConfigEvidence<'a> { FileExists(&'a Path), JsonForeign { path: &'a Path, has_foreign: &'a dyn Fn(&Value) -> bool } }`
  - `pub struct DetectSpec<'a> { pub agent_name: &'static str, pub config_path: PathBuf, pub evidence: Option<ConfigEvidence<'a>>, pub cli: &'static str, pub dir: &'a Path, pub ignore_in_dir: &'a [&'a str] }`
  - `pub fn detect_by_spec(env: &DetectEnv, spec: &DetectSpec<'_>) -> Detected`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ConflictPolicy { None, ForeignHttpWithUrl, AnyForeign }`
  - `pub fn hooks_of<'a>(doc: &'a Value, event: &str) -> impl Iterator<Item = &'a Value>`
  - `pub fn json_hooks_integration<'a>(doc: Option<&Value>, port: u16, is_our_shape: &dyn Fn(&Value) -> bool, events: impl IntoIterator<Item = (&'a str, bool)>, conflict_policy: ConflictPolicy) -> (Integration, Vec<String>)`
  - `pub fn render_permission_decision(decision: &Decision) -> Value`
  - `pub fn str_field(body: &Value, key: &str) -> Option<String>`
- Produces in `crates/server/src/agent/mod.rs`:
  - `AgentAdapter::activation_hint(&self) -> Option<&'static str>`
  - `impl AsRef<dyn AgentAdapter> for dyn AgentAdapter`
  - `pub fn registry() -> &'static [&'static dyn AgentAdapter]`
  - `pub fn find(id: &str) -> Option<&'static dyn AgentAdapter>`

- [ ] **Step 1: Write unit tests for `kit.rs` and static `registry()` / `find()`**

Add unit tests at the bottom of `crates/server/src/agent/kit.rs` and `crates/server/src/agent/mod.rs`:

```rust
// In crates/server/src/agent/kit.rs
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_by_spec_prioritizes_config_then_cli_then_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let dir = home.join(".demo");
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        let hooks = dir.join("hooks.json");
        let env = DetectEnv { home: home.to_path_buf(), path: None };

        let spec = DetectSpec {
            agent_name: "Demo",
            config_path: hooks.clone(),
            evidence: Some(ConfigEvidence::FileExists(&cfg)),
            cli: "demo-cli",
            dir: &dir,
            ignore_in_dir: &["hooks.json"],
        };
        assert!(!detect_by_spec(&env, &spec).installed);

        std::fs::write(dir.join("session.jsonl"), "{}").unwrap();
        let d = detect_by_spec(&env, &spec);
        assert_eq!((d.installed, d.confidence, d.reason), (true, Confidence::Medium, "parent-dir"));

        std::fs::write(&cfg, "").unwrap();
        let d = detect_by_spec(&env, &spec);
        assert_eq!((d.installed, d.confidence, d.reason), (true, Confidence::High, "config-file"));
    }

    #[test]
    fn render_permission_decision_matches_claude_and_codex_envelope() {
        assert_eq!(render_permission_decision(&Decision::NoDecision), json!({}));
        let allow = render_permission_decision(&Decision::Allow {
            message: Some("ok".into()),
            updated_input: Some(json!({"a": 1})),
        });
        assert_eq!(
            allow,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow", "message": "ok", "updatedInput": {"a": 1} }
                }
            })
        );
    }
}
```

```rust
// In crates/server/src/agent/mod.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_registry_and_as_ref_are_zero_alloc_compatible() {
        let all = registry();
        assert_eq!(all.len(), 5);
        let claude = find("claude-code").unwrap();
        let via_as_ref: &dyn AgentAdapter = claude.as_ref();
        assert_eq!(via_as_ref.id(), "claude-code");
    }
}
```

- [ ] **Step 2: Run `cargo test -p gateterm-server --lib agent::` to verify failure**

Run: `cargo test -p gateterm-server --lib agent::`
Expected: FAIL to compile (`kit` module and new signatures not yet defined).

- [ ] **Step 3: Implement `crates/server/src/agent/kit.rs` and update `mod.rs` & `routes.rs`**

Create `crates/server/src/agent/kit.rs` implementing `DetectSpec`, `ConfigEvidence`, `detect_by_spec`, `ConflictPolicy`, `hooks_of`, `json_hooks_integration`, `render_permission_decision`, and `str_field`.
In `crates/server/src/agent/mod.rs`:
- Declare `pub mod kit;`.
- Add `fn activation_hint(&self) -> Option<&'static str> { None }` to `AgentAdapter`.
- Add `impl AsRef<dyn AgentAdapter> for dyn AgentAdapter { fn as_ref(&self) -> &dyn AgentAdapter { self } }`.
- Define `static REGISTRY: &[&dyn AgentAdapter] = &[&claude_code::ClaudeCode, &codex::Codex, &pi::Pi, &grok::Grok, &antigravity::Antigravity];` and update `registry()` and `find(id)` to return `&'static [&'static dyn AgentAdapter]` and `Option<&'static dyn AgentAdapter>`.
- Update `startup_warnings` in `mod.rs` to use `a_adapter.activation_hint().unwrap_or("按设置页「AI 集成」里的提示完成最后一步")` and implement `activation_hint(&self)` on `Codex` and `Pi`.
- Update `ctx_for` in `crates/server/src/agent/routes.rs` to return `Option<(&'static dyn crate::agent::AgentAdapter, InstallCtx)>`.

- [ ] **Step 4: Run tests to verify `kit` and static registry pass**

Run: `cargo test -p gateterm-server --lib agent:: && cargo test -p gateterm-server --test agent_purge`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/agent/kit.rs crates/server/src/agent/mod.rs crates/server/src/agent/routes.rs crates/server/src/agent/codex.rs crates/server/src/agent/pi.rs
git commit -m "refactor(agent): introduce protocol family kit and static adapter registry"
```

---

### Task 2: Decouple `hook_cli.rs` and `process.rs` via Adapter Strategies

**Files:**
- Modify: `crates/server/src/agent/mod.rs`
- Modify: `crates/server/src/agent/hook_cli.rs`
- Modify: `crates/server/src/agent/process.rs`
- Modify: `crates/server/src/agent/routes.rs`
- Modify: `crates/server/src/agent/claude_code.rs`
- Modify: `crates/server/src/agent/codex.rs`
- Modify: `crates/server/src/agent/antigravity.rs`

**Interfaces:**
- Produces in `crates/server/src/agent/hook_cli.rs`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CommandWrap { StandardQuoted, WindowsPowerShellEncoded, WindowsPowerShellCall }`
- Produces in `crates/server/src/agent/process.rs`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ProcessWatchSpec { pub exe_names: &'static [&'static str], pub exclude_subcommands: &'static [&'static str] }`
  - `pub(super) async fn watch_agent(state: &AppState, adapter: &dyn super::AgentAdapter, session: &AgentSession, event: &str)`
- Produces in `crates/server/src/agent/mod.rs` (`AgentAdapter`):
  - `fn command_wrap(&self) -> hook_cli::CommandWrap { hook_cli::CommandWrap::StandardQuoted }`
  - `fn process_watch(&self) -> Option<process::ProcessWatchSpec> { None }`

- [ ] **Step 1: Write unit tests for `CommandWrap` dispatch and `ProcessWatchSpec` matching**

In `crates/server/src/agent/hook_cli.rs` `mod tests`:
```rust
#[test]
fn command_wrap_follows_adapter_strategy_and_defaults_for_unknown_agent() {
    let exe = std::path::Path::new("/bin/gateterm-server");
    let unknown = command_for_executable("future-agent", 7683, "Stop", exe);
    assert_eq!(unknown, "\"/bin/gateterm-server\" hook http://127.0.0.1:7683/hook/future-agent/stop");
}
```
In `crates/server/src/agent/process.rs` `mod tests`:
```rust
const CODEX_SPEC: ProcessWatchSpec = ProcessWatchSpec {
    exe_names: &["codex", "codex.exe"],
    exclude_subcommands: &["app-server", "exec-server", "mcp-server"],
};
// Update shared_daemon_is_not_the_terminal_cli to call matches_cli(&CODEX_SPEC, ...)
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p gateterm-server --lib agent::hook_cli && cargo test -p gateterm-server --lib agent::process`
Expected: FAIL to compile (`CommandWrap`, `ProcessWatchSpec`, `matches_cli` not yet defined).

- [ ] **Step 3: Implement `CommandWrap` and `ProcessWatchSpec` across `hook_cli.rs`, `process.rs`, `routes.rs`, and adapters**

- In `hook_cli.rs`: define `CommandWrap`, look up `super::find(agent).map(|a| a.command_wrap()).unwrap_or(CommandWrap::StandardQuoted)` in `command_for_executable`, and remove hardcoded `"antigravity" | "claude-code"` and `"codex"` checks.
- In `claude_code.rs` and `antigravity.rs`: implement `fn command_wrap(&self) -> CommandWrap { CommandWrap::WindowsPowerShellEncoded }`.
- In `codex.rs`: implement `fn command_wrap(&self) -> CommandWrap { CommandWrap::WindowsPowerShellCall }` and `fn process_watch(&self) -> Option<ProcessWatchSpec>`.
- In `process.rs`: replace `is_codex_cli`, `find_codex`, and `watch_codex` with `matches_cli(spec: &ProcessWatchSpec, name: &str, args: &[ std::ffi::OsString]) -> bool`, `find_cli(system: &System, root: u32, spec: &ProcessWatchSpec) -> Option<Identity>`, and `pub(super) async fn watch_agent(state: &AppState, adapter: &dyn super::AgentAdapter, session: &AgentSession, event: &str)`.
- In `routes.rs`: replace `super::process::watch_codex(&state, &snapshot, &event).await` and `super::process::watch_codex(state, &s, "PermissionRequest").await` with `super::process::watch_agent(...)`.

- [ ] **Step 4: Run tests to verify `hook_cli` and `process` pass**

Run: `cargo test -p gateterm-server --lib agent:: && cargo test -p gateterm-desktop-web --test hook_cli`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/agent/mod.rs crates/server/src/agent/hook_cli.rs crates/server/src/agent/process.rs crates/server/src/agent/routes.rs crates/server/src/agent/claude_code.rs crates/server/src/agent/codex.rs crates/server/src/agent/antigravity.rs
git commit -m "refactor(agent): move command wrap and process watch policies into AgentAdapter"
```

---

### Task 3: Decouple `history.rs` via `AgentAdapter` History & Resume Hooks

**Files:**
- Modify: `crates/server/src/agent/mod.rs`
- Modify: `crates/server/src/agent/history.rs`
- Modify: `crates/server/src/agent/claude_code.rs`
- Modify: `crates/server/src/agent/codex.rs`
- Modify: `crates/server/src/agent/pi.rs`
- Modify: `crates/server/src/agent/grok.rs`
- Modify: `crates/server/src/agent/antigravity.rs`

**Interfaces:**
- Produces in `crates/server/src/agent/history.rs`:
  - `#[derive(Debug, Clone)] pub struct HistoryRoot { pub path: PathBuf, pub max_depth: usize }`
  - `#[derive(Debug, Default)] pub struct RowMeta { pub id: Option<String>, pub cwd: Option<String>, pub title: Option<String>, pub first_prompt: Option<String>, pub description: Option<String>, pub model: Option<String>, pub sidechain: bool }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ResumeTarget { SessionId, TranscriptPath }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ResumeSpec { pub cli: &'static str, pub flag: &'static str, pub target: ResumeTarget }`
  - `pub(crate) fn text(v: &Value, key: &str) -> Option<String>`
  - `pub(crate) fn short(value: &str) -> String`
  - `pub(crate) fn prompt(value: &str) -> Option<String>`
  - `pub(crate) fn model_name(value: &Value) -> Option<String>`
  - `pub(crate) fn sample_with_limits(path: &Path, head_bytes: u64, tail_bytes: u64) -> std::io::Result<String>`
- Produces in `crates/server/src/agent/mod.rs` (`AgentAdapter`):
  - `fn history_roots(&self, _home: &Path) -> Vec<history::HistoryRoot>`
  - `fn accept_history_file(&self, file_name: &str) -> bool`
  - `fn inspect_history_row(&self, _row: &Value, _meta: &mut history::RowMeta)`
  - `fn finalize_history_entry(&self, _path: &Path, _meta: &mut history::RowMeta)`
  - `fn enrich_history_catalog(&self, _home: &Path, _entries: &mut [history::Entry])`
  - `fn resume_spec(&self) -> Option<history::ResumeSpec>`

- [ ] **Step 1: Write unit test verifying `history.rs` delegates roots, filtering, row inspection, enrichment, and resume specs to adapters**

Add a test in `crates/server/src/agent/history.rs` `mod tests`:
```rust
#[test]
fn every_registered_adapter_declares_history_roots_and_resume_spec() {
    let home = Path::new("/tmp/home");
    for adapter in super::super::registry() {
        assert!(!adapter.history_roots(home).is_empty(), "{} missing history_roots", adapter.id());
        assert!(adapter.resume_spec().is_some(), "{} missing resume_spec", adapter.id());
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p gateterm-server --lib agent::history::tests::every_registered_adapter_declares_history_roots_and_resume_spec`
Expected: FAIL to compile (`history_roots` and `resume_spec` not yet defined on `AgentAdapter`).

- [ ] **Step 3: Move agent-specific history & resume logic from `history.rs` into the 5 adapters**

1. Add `HistoryRoot`, `RowMeta`, `ResumeTarget`, `ResumeSpec` in `history.rs` and the 6 trait methods to `AgentAdapter` in `mod.rs`.
2. Implement the history & resume methods on each adapter:
   - `ClaudeCode`:
     - `history_roots`: `[HistoryRoot { path: home.join(".claude/projects"), max_depth: 1 }]`
     - `inspect_history_row`: extract `sessionId`/`session_id`, `custom-title` -> `customTitle`, `row["message"]` model name.
     - `finalize_history_entry`: fallback `meta.id` to `path.file_stem()`.
     - `resume_spec`: `Some(ResumeSpec { cli: "claude", flag: "--resume", target: ResumeTarget::SessionId })`.
   - `Codex`:
     - `history_roots`: `.codex/sessions` (depth 3) and `.codex/archived_sessions` (depth 0).
     - `inspect_history_row`: extract `session_meta` (`id`/`session_id`, `cwd`, `title`/`thread_name`), `turn_context` (`model`), and legacy `event_msg` (`user_message` / `agent_message`).
     - `enrich_history_catalog`: apply `.codex/session_index.jsonl` custom titles and `.codex/history.jsonl` fallback opening prompts.
     - `resume_spec`: `Some(ResumeSpec { cli: "codex", flag: "resume", target: ResumeTarget::SessionId })`.
   - `Pi`:
     - `history_roots`: `[HistoryRoot { path: home.join(".pi/agent/sessions"), max_depth: 1 }]`
     - `inspect_history_row`: `session` -> `id`, `session_info` -> `name`, `model_change` / `message` -> `model`.
     - `resume_spec`: `Some(ResumeSpec { cli: "pi", flag: "--session", target: ResumeTarget::TranscriptPath })`.
   - `Grok`:
     - `history_roots`: `[HistoryRoot { path: home.join(".grok/sessions"), max_depth: 2 }]`
     - `accept_history_file`: `file_name == "updates.jsonl"`
     - `inspect_history_row`: extract `/params/sessionId`.
     - `finalize_history_entry`: read sibling `session.json`/`metadata.json`/`meta.json`, fallback `id` to parent dir name, fallback `cwd` via URL-decoded grandparent dir name (`decode_project`).
     - `resume_spec`: `Some(ResumeSpec { cli: "grok", flag: "--resume", target: ResumeTarget::SessionId })`.
   - `Antigravity`:
     - `history_roots`: `[HistoryRoot { path: home.join(".gemini/antigravity-cli/brain"), max_depth: 4 }]`
     - `accept_history_file`: `file_name == "transcript_full.jsonl"`
     - `finalize_history_entry`: extract `id` from `path.ancestors().nth(3)`.
     - `resume_spec`: `Some(ResumeSpec { cli: "agy", flag: "--conversation", target: ResumeTarget::SessionId })`.
3. Simplify `history::collect`, `history::parse`, `history::scan`, and `history::resume_command` to call the `AgentAdapter` trait methods with zero hardcoded agent names.

- [ ] **Step 4: Run all `history` unit tests**

Run: `cargo test -p gateterm-server --lib agent::history`
Expected: PASS (all 10 history tests pass).

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/agent/mod.rs crates/server/src/agent/history.rs crates/server/src/agent/claude_code.rs crates/server/src/agent/codex.rs crates/server/src/agent/pi.rs crates/server/src/agent/grok.rs crates/server/src/agent/antigravity.rs
git commit -m "refactor(agent): move history discovery, metadata extraction, and resume specs into AgentAdapter"
```

---

### Task 4: Reorganize Concrete Adapters into `adapters/<agent>/` and Deduplicate via `kit.rs`

**Files:**
- Create: `crates/server/src/agent/adapters/mod.rs`
- Move & Refactor:
  - `crates/server/src/agent/claude_code.rs` → `crates/server/src/agent/adapters/claude_code/mod.rs`
  - `crates/server/src/agent/claude_transcript.rs` → `crates/server/src/agent/adapters/claude_code/transcript.rs`
  - `crates/server/src/agent/codex.rs` → `crates/server/src/agent/adapters/codex/mod.rs`
  - `crates/server/src/agent/codex_transcript.rs` → `crates/server/src/agent/adapters/codex/transcript.rs`
  - `crates/server/src/agent/codex_trust.rs` → `crates/server/src/agent/adapters/codex/trust.rs`
  - `crates/server/src/agent/pi.rs` → `crates/server/src/agent/adapters/pi/mod.rs`
  - `crates/server/src/agent/pi_transcript.rs` → `crates/server/src/agent/adapters/pi/transcript.rs`
  - `crates/server/src/agent/pi_extension.ts` → `crates/server/src/agent/adapters/pi/extension.ts`
  - `crates/server/src/agent/grok.rs` → `crates/server/src/agent/adapters/grok/mod.rs`
  - `crates/server/src/agent/grok_transcript.rs` → `crates/server/src/agent/adapters/grok/transcript.rs`
  - `crates/server/src/agent/antigravity.rs` → `crates/server/src/agent/adapters/antigravity/mod.rs`
  - `crates/server/src/agent/antigravity_transcript.rs` → `crates/server/src/agent/adapters/antigravity/transcript.rs`
- Modify: `crates/server/src/agent/mod.rs`

**Interfaces:**
- Produces in `crates/server/src/agent/adapters/mod.rs`:
  - `pub mod antigravity; pub mod claude_code; pub mod codex; pub mod grok; pub mod pi;`
  - `pub static REGISTRY: &[&dyn super::AgentAdapter] = &[&claude_code::ClaudeCode, &codex::Codex, &pi::Pi, &grok::Grok, &antigravity::Antigravity];`
- Produces in `crates/server/src/agent/mod.rs` (compatibility re-exports):
  - `pub mod adapters;`
  - `pub use adapters::{antigravity, claude_code, codex, grok, pi};`
  - `pub use adapters::antigravity::transcript as antigravity_transcript;`
  - `pub use adapters::claude_code::transcript as claude_transcript;`
  - `pub use adapters::codex::{transcript as codex_transcript, trust as codex_trust};`
  - `pub use adapters::grok::transcript as grok_transcript;`
  - `pub use adapters::pi::transcript as pi_transcript;`

- [ ] **Step 1: Add architectural boundary test in `crates/server/src/agent/mod.rs`**

Add a test in `crates/server/src/agent/mod.rs` `mod tests`:
```rust
#[test]
fn core_agent_modules_contain_no_hardcoded_agent_id_branches() {
    let agent_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent");
    for file in ["mod.rs", "routes.rs", "store.rs", "edit.rs", "apply.rs", "detect.rs", "hook_cli.rs", "process.rs", "history.rs", "transcript.rs"] {
        let src = std::fs::read_to_string(agent_dir.join(file)).unwrap();
        let prod = src.split("#[cfg(test)]").next().unwrap_or(&src);
        for id in ["\"claude-code\"", "\"codex\"", "\"grok\"", "\"antigravity\""] {
            assert!(!prod.contains(id), "{file} production code still hardcodes agent id {id}");
        }
    }
}
```

- [ ] **Step 2: Run the boundary test to verify failure**

Run: `cargo test -p gateterm-server --lib agent::tests::core_agent_modules_contain_no_hardcoded_agent_id_branches`
Expected: FAIL if any core file still references concrete adapter modules or IDs.

- [ ] **Step 3: Move all 5 adapters into `crates/server/src/agent/adapters/<agent>/`, wire `kit.rs` helpers, and re-export compatibility aliases in `mod.rs`**

1. Create `crates/server/src/agent/adapters/mod.rs` and the 5 subdirectories (`claude_code`, `codex`, `pi`, `grok`, `antigravity`).
2. Move each agent's files into its subdirectory and update `super::` imports (`crate::agent::...`).
3. Replace duplicated `detect`, `integration`, `hooks_of`, `render`, and `str_field` implementations in `claude_code/mod.rs`, `codex/mod.rs`, `pi/mod.rs`, `grok/mod.rs`, and `antigravity/mod.rs` with calls to `crate::agent::kit::*`.
4. Remove old top-level agent files from `crates/server/src/agent/` and export compatibility module aliases from `crates/server/src/agent/mod.rs`.

- [ ] **Step 4: Run the full server & desktop-web test suites and clippy**

Run: `cargo test -p gateterm-server && cargo test -p gateterm-desktop-web && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS with 0 warnings.

- [ ] **Step 5: Commit**

```bash
git add -A crates/server/src/agent docs/superpowers/specs/2026-11-03-agent-adapter-refactor-design.md docs/superpowers/plans/2026-11-03-agent-adapter-refactor.md
git commit -m "refactor(agent): organize adapters into submodules and deduplicate via kit"
```
