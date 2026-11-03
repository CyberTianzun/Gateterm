# `server::agent` 适配器架构与协议族底座重构设计

日期：2026-11-03  
状态：草案（待评审）

## 1. 背景与意图（Intent & Why）

[`crates/server/src/agent`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs) 最初只面向 1~2 个 Agent 的 Hook 安装与基础状态同步设计了 [`AgentAdapter`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L100-L168) Trait。随着功能演进，服务端现已支持 5 个 Agent（Claude Code、Codex、pi、Grok、Antigravity CLI），前端与文档已预置 7 个品牌，且后续计划扩展至 `zcode`、`cursor cli`、`kiro cli`、`opencode`、`kilo code`、`mimo code`、`hermes agent`、`github copilot`、`qwen code`、`devin`、`kimi code` 等十余种 CLI Agent。

在演进过程中，原有的适配器边界出现了三方面退化：

1. **目录结构扁平混杂**：框架基础设施（路由、会话表、配置编辑与落盘、窗口切片、历史索引）与各 Agent 的具体实现文件（`<agent>.rs`、`<agent>_transcript.rs`、`codex_trust.rs`、`pi_extension.ts`）共 22 个文件全部平铺在 `crates/server/src/agent/` 下。
2. **核心流程击穿 `AgentAdapter` 边界**：
   - [`history.rs`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs) 硬编码了 5 家 Agent 的扫描根目录 [`roots()`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs#L61-L70)、文件名过滤 [`collect()`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs#L97-L100)、JSONL 行元数据提取 `match candidate.agent`、Grok 旁路元数据与 Antigravity 路径推导、Codex 专属 `session_index.jsonl` / `history.jsonl` 后处理，以及 [`resume_command()`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs#L471-L500) 恢复命令表。
   - [`hook_cli.rs`](file:///D:/online/Octoterm/crates/server/src/agent/hook_cli.rs#L45-L55) 在 [`command_for_executable()`](file:///D:/online/Octoterm/crates/server/src/agent/hook_cli.rs#L45-L55) 中用 `matches!(agent, ...)` 硬编码 Windows 下的命令包装语法。
   - [`process.rs`](file:///D:/online/Octoterm/crates/server/src/agent/process.rs#L43-L113) 与 [`routes.rs`](file:///D:/online/Octoterm/crates/server/src/agent/routes.rs#L328) 将守护进程类 TUI 的退出监视写死为 [`watch_codex()`](file:///D:/online/Octoterm/crates/server/src/agent/process.rs#L81-L113) 和 [`is_codex_cli()`](file:///D:/online/Octoterm/crates/server/src/agent/process.rs#L43-L52)。
   - [`mod.rs`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L170-L182) 的 [`registry()`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L170-L178) 每次查询都重新在堆上分配 `Vec<Box<dyn AgentAdapter>>`；[`activation_hint()`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L294-L300) 硬编码激活提示且漏掉了后来新增的 `grok-reload` 与 `antigravity-restart`。
3. **缺少同族协议复用底座**：三元安装检测（配置文件 > PATH CLI > 目录痕迹）、JSON `hooks.<Event>` 数组安装/卸载与冲突对账、`PermissionRequest` 决策信封渲染在多个适配器中大量重复。

本次重构旨在将 [`server::agent`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs) 收敛为**边界完整的 `AgentAdapter` Trait + 可组合协议族底座（Kit） + 独立 `adapters/` 子模块**，做到**核心框架零 Agent ID 硬编码，新增 Agent 只需在 `adapters/` 下添加一个子模块并在静态注册表登记一行**。

---

## 2. 目标、非目标与反目标（Goals, Non-Goals, Anti-Goals）

### 目标（Goals）
1. **消除抽象泄漏**：[`history.rs`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs)、[`hook_cli.rs`](file:///D:/online/Octoterm/crates/server/src/agent/hook_cli.rs)、[`process.rs`](file:///D:/online/Octoterm/crates/server/src/agent/process.rs)、[`routes.rs`](file:///D:/online/Octoterm/crates/server/src/agent/routes.rs)、[`mod.rs`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs)、[`transcript.rs`](file:///D:/online/Octoterm/crates/server/src/agent/transcript.rs) 中不再出现任何具体 Agent ID（如 `"claude-code"`、`"codex"`、`"pi"`、`"grok"`、`"antigravity"`）的分支判断。
2. **目录物理分层**：核心基础设施保留在 `crates/server/src/agent/` 顶层，所有具体 Agent 迁移至 `crates/server/src/agent/adapters/<agent>/` 子目录。
3. **提炼协议族底座（`kit.rs`）**：将三元安装检测、`hooks.<Event>` JSON 组管理与状态对账、Claude/Codex 兼容的决策信封渲染抽成声明式可复用组件。
4. **静态零分配注册表**：[`registry()`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L170-L178) 返回 `&'static [&'static dyn AgentAdapter]`，[`find()`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L180-L182) 返回 `Option<&'static dyn AgentAdapter>`。
5. **100% 行为与测试兼容**：线上协议（HTTP `/api/agents/*`、`/hook/*`、WS `ServerMsg::AgentEvent`）及现有单元/集成测试全部无感通过。

### 非目标（Non-Goals）
- 本次重构**不新增**新的 Agent 实现（新 Agent 在重构合入后按新架构独立接入）。
- 不修改 Web、Desktop、Android 客户端代码或线上协议。

### 反目标（Anti-Goals）
- **不做僵化的纯配置表驱动**：不把 Codex `app-server` JSON-RPC 信任检查、Grok 旁路文件/URL 解码等异构逻辑强行塞进一个布满特判枚举的全局配置结构体。
- **不破坏现有外部调用路径**：集成测试（`crates/server/tests/agent_*.rs`、`clients/desktop-web/tests/hook_cli.rs`）与桌面端宿主引用的公开符号通过 `pub use` 重导出保持兼容。

---

## 3. 约束条件（Constraints）

1. **安全与写盘边界不变**：发现（`detect`、`scan`、`history`、`messages`）永远只读；写盘仅发生在用户显式 install/uninstall/purge 时，经 [`apply.rs`](file:///D:/online/Octoterm/crates/server/src/agent/apply.rs) 统一门控、备份与原子替换。
2. **局部失败原则不变**：单个 Adapter 的检测、配置解析或历史文件损坏只影响该条目，绝不波及其他 Adapter。
3. **测试与调用方源码兼容**：
   - `crates/server/src/agent/mod.rs` 保留 `pub use adapters::{antigravity, claude_code, codex, grok, pi};` 以及 `claude_transcript`、`codex_transcript`、`grok_transcript`、`pi_transcript`、`antigravity_transcript`、`codex_trust` 的模块别名重导出，确保现有 `crates/server/tests/agent_*.rs` 无需大规模重写导入路径。
   - 为 `dyn AgentAdapter` 实现 `AsRef<dyn AgentAdapter>`，使返回 `&'static dyn AgentAdapter` 的 [`find()`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L180-L182) 既能直接传参给 `plan_purge(adapter, ctx)`，也能兼容现有测试中的 `plan_purge(adapter.as_ref(), ctx)` 调用。

---

## 4. 备选方案与取舍（Approaches）

| 方案 | 做法 | 优点 | 缺点 / 取舍 | 结论 |
| :--- | :--- | :--- | :--- | :--- |
| **方案 A：单一胖 Trait + 子模块划分** | 仅把泄漏逻辑塞进 `AgentAdapter` Trait，按 `adapters/<agent>/` 拆目录，不抽公共底座 | 边界清晰，能消除核心模块的 `match agent_id` | 未解决同族 Agent（三元检测、`hooks` JSON 对账、审批渲染）的样板重复，扩充 10+ Agent 时仍有大量复制粘贴 | 不采用 |
| **方案 B：完整 `AgentAdapter` Trait + 可组合协议族底座（`kit`）+ `adapters/<agent>/`** | 用完整 Trait 定义所有接入面边界，提取 `kit.rs` 提供三元检测、JSON Hook 族对账、决策渲染等可组合部件，同时允许特殊 Agent 覆写任意 Trait 方法 | 严格守住“新增 Agent = 1 个子模块 + 1 行注册”边界；常规 Agent 只需极少声明代码，特殊 Agent 仍保有完全的多态自由度 | 需要设计清晰的策略结构（`DetectSpec`、`JsonHooksSpec`、`HistoryRoot`、`ResumeSpec`、`ProcessWatchSpec`）并迁移现有 5 个适配器 | **已选定** |
| **方案 C：纯声明式配置表驱动** | 取消各 Agent 的多态实现，所有行为用一张静态配置表 + 枚举标志描述 | 简单 Agent 代码最少 | 无法优雅表达 Codex JSON-RPC 信任查询、Grok URL 编码目录回退、Antigravity 标签剥离等异构逻辑，会让核心引擎退化回特判大泥球 | 不采用 |

---

## 5. 详细设计（Detailed Design）

### 5.1 目录结构

```text
crates/server/src/agent/
├── mod.rs              # AgentAdapter Trait、静态 REGISTRY、scan/find/plan_purge/startup_warnings、兼容重导出
├── kit.rs              # 协议族复用底座：三元检测器、JSON hooks.<Event> 计划与状态对账、Claude 式决策信封渲染
├── detect.rs           # DetectEnv、PATH 查找、目录非我方条目判定
├── edit.rs             # 纯函数配置编辑计划（ConfigEdit / EditOp）与所有权验证（is_ours）
├── apply.rs            # 写盘门控、备份修剪、原子写入
├── hook_cli.rs         # `hook <url>` CLI 转发器 + 基于 CommandWrap 策略的命令串生成
├── process.rs          # 基于 ProcessWatchSpec 策略的通用终端子进程退出监视器
├── transcript.rs       # 聊天视图归一化消息模型、窗口与双向游标、工具入参压平
├── history.rs          # 宿主机历史会话扫描、采样、分页检索与恢复（完全经由 AgentAdapter 驱动）
├── store.rs            # 内存态会话表、挂起审批管理、过期清理
├── routes.rs           # /api/agents/* 与 /hook/{agent}/{event} HTTP 路由
└── adapters/           # 所有具体 Agent 实现（物理隔离）
    ├── mod.rs          # 注册表汇总：pub static REGISTRY: &[&dyn AgentAdapter]
    ├── claude_code/
    │   ├── mod.rs
    │   └── transcript.rs
    ├── codex/
    │   ├── mod.rs
    │   ├── transcript.rs
    │   └── trust.rs
    ├── pi/
    │   ├── mod.rs
    │   ├── transcript.rs
    │   └── extension.ts
    ├── grok/
    │   ├── mod.rs
    │   └── transcript.rs
    └── antigravity/
        ├── mod.rs
        └── transcript.rs
```

### 5.2 完整的 `AgentAdapter` Trait 契约

[`AgentAdapter`](file:///D:/online/Octoterm/crates/server/src/agent/mod.rs#L100-L168) 覆盖一个 Agent 在 Gateterm 全生命周期中的 **7 个接入面**，除最核心的身份、检测、装卸与事件解析外，均提供安全的默认实现：

```rust
pub trait AgentAdapter: Send + Sync {
    // ── 1. 身份 ──────────────────────────────────────────────────────
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;

    // ── 2. 检测、安装与自检 ──────────────────────────────────────────
    fn detect(&self, env: &detect::DetectEnv) -> Detected;
    fn plan_install(&self, ctx: &edit::InstallCtx) -> anyhow::Result<Vec<edit::ConfigEdit>>;
    fn plan_uninstall(&self, ctx: &edit::InstallCtx) -> anyhow::Result<Vec<edit::ConfigEdit>>;
    fn integration(&self, ctx: &edit::InstallCtx) -> (Integration, Vec<String>);

    /// 装完后是否还需用户在 Agent 内执行激活操作（机器可读键，供客户端国际化渲染）。
    fn activation(&self) -> Option<&'static str> { None }

    /// 服务端启动自检日志中的激活操作提示（供 `startup_warnings` 使用）。
    fn activation_hint(&self) -> Option<&'static str> { None }

    /// 激活步骤是否已完成（如 Codex `/hooks` 信任审核）。
    fn activated(&self, _env: &detect::DetectEnv, _ctx: &edit::InstallCtx) -> bool { true }

    // ── 3. Hook 命令包装策略 ─────────────────────────────────────────
    /// Windows 下生成 command 型 hook 命令串时的 shell 包装风格。
    fn command_wrap(&self) -> hook_cli::CommandWrap {
        hook_cli::CommandWrap::StandardQuoted
    }

    // ── 4. 实时 Hook 事件与远程审批 ──────────────────────────────────
    fn agent_session_id(&self, body: &serde_json::Value) -> Option<String> {
        body.get("session_id").and_then(serde_json::Value::as_str).map(str::to_string)
    }
    fn parse(&self, event: &str, body: &serde_json::Value) -> Option<store::Update>;
    fn telemetry_reply(&self) -> Option<serde_json::Value> { None }
    fn is_blocking(&self, event: &str) -> bool {
        let _ = event;
        false
    }
    fn render(&self, _decision: &store::Decision) -> serde_json::Value {
        serde_json::json!({})
    }

    // ── 5. 守护进程类 CLI 退出监视 ───────────────────────────────────
    /// 当 Agent 采用共享后台守护进程、TUI 退出不一定立即触发 SessionEnd 时（如 Codex），
    /// 返回进程匹配规则，由 `process::watch_agent` 监视托管终端内的前台 CLI 退出。
    fn process_watch(&self) -> Option<process::ProcessWatchSpec> { None }

    // ── 6. 聊天视图（Transcript）────────────────────────────────────
    fn supports_transcript(&self) -> bool { false }
    fn locate_transcript(&self, _home: &Path, _agent_session_id: &str) -> Option<PathBuf> { None }
    fn parse_transcript(&self, _text: &str, _base_offset: u64) -> Vec<transcript::Message> { Vec::new() }

    // ── 7. 历史会话索引与一键恢复（History & Resume）────────────────
    /// 该 Agent 历史会话文件所在的根目录与最大目录递归深度。
    fn history_roots(&self, _home: &Path) -> Vec<history::HistoryRoot> { Vec::new() }

    /// 历史扫描时是否接受该 `.jsonl` 文件名（默认排除 `agent-*` 子代理记录）。
    fn accept_history_file(&self, file_name: &str) -> bool {
        !file_name.starts_with("agent-")
    }

    /// 扫描采样阶段：从单行 JSONL 记录中提取会话 ID、工作目录、标题、模型名、是否为子代理（sidechain）及补充提示词。
    fn inspect_history_row(&self, _row: &serde_json::Value, _meta: &mut history::RowMeta) {}

    /// 扫描采样收尾阶段：结合文件路径或同目录旁路文件（如 Grok 的 `metadata.json`、Antigravity 的路径祖先、Claude 的文件名 stem）补全元数据。
    fn finalize_history_entry(&self, _path: &Path, _meta: &mut history::RowMeta) {}

    /// 全局索引后处理：在所有候选文件解析完毕后，允许 Adapter 用外部索引文件（如 Codex 的 `session_index.jsonl` 与 `history.jsonl`）批量补全标题。
    fn enrich_history_catalog(&self, _home: &Path, _entries: &mut [history::Entry]) {}

    /// 一键恢复历史会话时的 CLI 启动规格（可执行文件名、参数标志、传会话 ID 还是文件路径）。
    fn resume_spec(&self) -> Option<history::ResumeSpec> { None }
}
```

### 5.3 核心模块解耦细节

1. **[`hook_cli.rs`](file:///D:/online/Octoterm/crates/server/src/agent/hook_cli.rs)**：
   - 定义 `pub enum CommandWrap { StandardQuoted, WindowsPowerShellEncoded, WindowsPowerShellCall }`。
   - `command_for_executable(agent: &str, port: u16, event: &str, exe: &Path)` 内部通过 `super::find(agent).map(|a| a.command_wrap()).unwrap_or(CommandWrap::StandardQuoted)` 选择包装方式，不再硬编码 `"antigravity" | "claude-code"` 或 `"codex"`。
2. **[`process.rs`](file:///D:/online/Octoterm/crates/server/src/agent/process.rs)**：
   - 定义 `pub struct ProcessWatchSpec { pub exe_names: &'static [&'static str], pub exclude_subcommands: &'static [&'static str] }`。
   - 将原 `watch_codex` 泛化为 `pub(super) async fn watch_agent(state: &AppState, adapter: &dyn AgentAdapter, session: &AgentSession, event: &str)`：若 `adapter.process_watch()` 返回 `Some(spec)`，则在托管终端进程树下查找匹配 `spec` 的最外层 CLI 进程并监视其退出。
3. **[`history.rs`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs)**：
   - 定义：
     - `pub struct HistoryRoot { pub path: PathBuf, pub max_depth: usize }`
     - `pub struct RowMeta { pub id: Option<String>, pub cwd: Option<String>, pub title: Option<String>, pub first_prompt: Option<String>, pub description: Option<String>, pub model: Option<String>, pub sidechain: bool }`
     - `pub enum ResumeTarget { SessionId, TranscriptPath }`
     - `pub struct ResumeSpec { pub cli: &'static str, pub flag: &'static str, pub target: ResumeTarget }`
   - `scan()` 遍历 `super::registry()` 中每个 Adapter 的 `history_roots(&env.home)`，调用 `adapter.accept_history_file(name)` 过滤，采样后逐行调 `adapter.inspect_history_row(&row, &mut meta)` + 通用 `adapter.parse_transcript(line, 0)`，收尾调 `adapter.finalize_history_entry(&candidate.path, &mut meta)`，最后对每个 Adapter 调一次 `adapter.enrich_history_catalog(&env.home, &mut catalog.entries)`。
   - `resume_command()` 查询 `adapter.resume_spec()` 构造恢复命令，彻底移除 `match entry.agent_id.as_str()`。

### 5.4 协议族复用底座（`kit.rs`）

为了大幅降低当前 5 个 Agent 以及未来 10+ 新 Agent 的样板代码，`kit.rs` 提供三组正交的声明式/函数式构件：

1. **标准三元检测器（`DetectSpec`）**：
   - 支持配置：① 配置文件硬证据判定（文件存在，或 JSON 文件含非我方内容 `has_foreign_content`）、② `PATH` 上的 CLI 可执行文件名、③ 父目录及排除列表（`ignore_entries`）。
   - 一行调用即可生成标准的 `Detected { installed, confidence, reason, detail, config_path }`。
2. **`hooks.<Event>` JSON 数组族配置助手（`JsonHooksSpec`）**：
   - 适用于配置文件形如 `{"hooks": {"<Event>": [{"matcher"?: "...", "hooks": [...]}]}}` 的所有 Agent（Claude Code、Codex、Grok，以及未来的 Cursor CLI、Kiro CLI、Qwen Code 等）。
   - 提供通用的 ` hooks_of(doc, event)`、`is_our_hook_shape(hook, agent_id)`、`plan_json_hooks_install`、`plan_json_hooks_uninstall`、以及 `json_hooks_integration`（自动统计当前端口命中数、旧端口残留数、以及阻塞事件上的第三方冲突描述）。
3. **Claude/Codex 风格 `PermissionRequest` 决策渲染器（`render_permission_decision`）**：
   - 统一渲染 `{"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"|"deny", "message"?, "updatedInput"?}}}`，消除 Claude Code 与 Codex（以及未来兼容 Claude Hook 协议的 Agent）之间的重复实现。

---

## 6. 留给实现者的细节（What's Left to the Builder）

- `kit.rs` 内部辅助函数的具体参数签名与模块内可见性（`pub` vs `pub(crate)`）。
- [`history.rs`](file:///D:/online/Octoterm/crates/server/src/agent/history.rs) 中通用字符串工具函数（`text`、`short`、`prompt`、`model_name`）提升到 `history` 或 `kit` 的具体摆放位置，以方便各 Adapter 的 `inspect_history_row` 调用。
- 在保持全部现有单元测试与集成测试不变的前提下，为重构后的 `kit` 与多态分发补充必要的边界单测。
