//! agent 集成。扩展点只有一个:[`AgentAdapter`]。
//!
//! 形态照抄 `launcher` 模块 —— 多来源、单一 trait、**失败局部化**:某个 adapter
//! 抛错只让它自己的条目消失并留一条日志,不影响其他 adapter,更不影响终端本身。
//!
//! 与 `launcher` 的关键区别,也是这个模块唯一需要小心的地方:launcher **只读**
//! 扫描别人的配置,agent 集成在用户显式动作下**会写**别人的配置。这条例外的边界是
//! 「发现只读、集成需显式动作」,由三道关卡守住:`agents.install_enabled` 开关、
//! 写前备份、卸载能还原。设计依据见
//! `docs/superpowers/specs/2026-08-18-octoterm-agent-integration-design.md`。

use serde::Serialize;
use std::path::PathBuf;

pub mod apply;
pub mod claude_code;
pub mod claude_transcript;
pub mod codex;
pub mod codex_transcript;
pub mod codex_trust;
pub mod detect;
pub mod edit;
pub mod hook_cli;
pub mod pi;
pub mod pi_transcript;
pub mod routes;
pub mod store;
pub mod transcript;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// 「本机装没装这个 agent」的判定结果。
#[derive(Debug, Clone, Serialize)]
pub struct Detected {
    pub installed: bool,
    pub confidence: Confidence,
    /// 机器可读的判定依据。客户端可以据此分组,但不该硬编码文案。
    pub reason: &'static str,
    /// 给人看的一句话,UI 直接显示,**不解析**。
    pub detail: String,
    pub config_path: Option<PathBuf>,
}

/// 我方集成当前处于什么状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Integration {
    NotInstalled,
    Installed,
    /// 装过,但 URL 指向的端口不是当前监听端口 —— 「装了却不生效」。
    ///
    /// 这是个必须单独报出来的状态:按实测,连不上的 hook 对 Claude 是
    /// non-blocking,它照常能用,于是这种失效**完全没有外部症状**,只有远程
    /// 接管会静默失灵。不主动报,用户永远不会发现。
    StalePort,
    /// 当前端口上装着,但和「按当前服务装一遍」的结果对不上:缺了事件、混着别的
    /// 端口的残留、或 Codex 命令里的二进制路径变了。判定见 [`edit::is_current`]。
    ///
    /// 和 `StalePort` 一样没有外部症状 —— 部分事件静默失灵 —— 所以同样要单独报。
    Outdated,
    /// 装得和当前服务完全一致,但 agent 自己还没放行,hook 不会被调用。目前只有
    /// Codex:它按 `trusted_hash` 逐条门控,没在 `/hooks` 里审核过(或审核之后内容
    /// 又变了)的 hook 一律**静默跳过**。要做的是 `activation` 那一步,不是重装。
    NeedsActivation,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub detected: Detected,
    pub integration: Integration,
    /// 装完之后还需要用户做什么才生效。**机器可读的键**,不是文案 —— 文案由客户端
    /// 按自己的语言渲染(和 `detected.reason` 同一个路子)。
    ///
    /// Codex 和 pi 有。Codex:它用 `trusted_hash` 逐条门控 hook,必须由用户在它自己的
    /// TUI 里 review 过才生效。这一步没法自动化,也**不该**自动化 —— 那道闸防的正是
    /// 「第三方悄悄让 Codex 执行任意命令」,而我们恰好就是那个第三方。pi:扩展只在
    /// 启动时加载,已经在跑的要 `/reload`。
    pub activation: Option<&'static str>,
    /// 同一事件上发现的**别人的**阻塞式 hook,一行一条人类可读描述。
    ///
    /// 不是错误,也不会被我们删掉 —— 但两个阻塞 hook 抢同一个事件是真实的互操作
    /// 问题(例如本机同时装了 clawd-on-desk),必须让用户看得见。
    pub conflicts: Vec<String>,
}

pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;

    fn detect(&self, env: &detect::DetectEnv) -> Detected;

    /// 只产出「要对哪个文件做什么编辑」,**不写盘**。理由见 `edit` 模块的文档。
    fn plan_install(&self, ctx: &edit::InstallCtx) -> anyhow::Result<Vec<edit::ConfigEdit>>;
    fn plan_uninstall(&self, ctx: &edit::InstallCtx) -> anyhow::Result<Vec<edit::ConfigEdit>>;

    /// 只读地看一眼当前集成状态。读不到 / 读坏了一律当作没装。
    fn integration(&self, ctx: &edit::InstallCtx) -> (Integration, Vec<String>);

    /// 装完之后还需要用户做什么才生效。返回机器可读的键,`None` 表示装完即生效。
    fn activation(&self) -> Option<&'static str> {
        None
    }

    /// `activation` 那一步是不是已经做了。**只读**地看 agent 自己的记录,看不出来就当
    /// 做了 —— 这是给用户的提示,宁可漏报也不该把能用的集成报成不能用。
    fn activated(&self, env: &detect::DetectEnv, ctx: &edit::InstallCtx) -> bool {
        let _ = (env, ctx);
        true
    }

    /// 把 agent 方言的 hook payload 归一化成统一更新。
    ///
    /// **不认识的事件返回 `None`,当作忽略而不是错误** —— agent 升级会带来新事件,
    /// 不能因为多了一个事件名就 500,更不能因此把 Claude 卡住。
    fn parse(&self, event: &str, body: &serde_json::Value) -> Option<store::Update>;

    /// 这家的对话记录能不能读。`false` 时聊天视图回落到终端,并说明是「还不支持」
    /// 而不是「出错了」—— 两者对用户是两回事。
    fn supports_transcript(&self) -> bool {
        false
    }

    /// 记录文件在哪。有些 agent 在 hook payload 里直接给(Claude),有些得自己推
    /// (Codex 的文件名里含会话 id)。`None` = 推不出来。
    fn locate_transcript(&self, home: &std::path::Path, agent_session_id: &str) -> Option<PathBuf> {
        let _ = (home, agent_session_id);
        None
    }

    /// 把这家的对话记录解析成归一化消息。只在 [`Self::supports_transcript`] 为真时调用。
    fn parse_transcript(&self, text: &str, base_offset: u64) -> Vec<transcript::Message> {
        let _ = (text, base_offset);
        Vec::new()
    }

    /// 这个事件是不是「阻塞式」的 —— agent 会挂在 socket 上等我们写响应。
    fn is_blocking(&self, event: &str) -> bool;

    /// 把统一决策渲染回 agent 方言的响应体。
    fn render(&self, decision: &store::Decision) -> serde_json::Value;
}

pub fn registry() -> Vec<Box<dyn AgentAdapter>> {
    vec![Box::new(claude_code::ClaudeCode), Box::new(codex::Codex), Box::new(pi::Pi)]
}

pub fn find(id: &str) -> Option<Box<dyn AgentAdapter>> {
    registry().into_iter().find(|a| a.id() == id)
}

/// 清空计划:对这个 adapter 碰过的每个配置文件做一次 [`edit::EditOp::PurgeOurs`]。
///
/// 文件清单取自卸载计划 —— 「我们会写哪些文件」只有 adapter 知道,而卸载计划恰好
/// 覆盖了全部。不另开一个 trait 方法,免得两份清单日后走散。
pub fn plan_purge(adapter: &dyn AgentAdapter, ctx: &edit::InstallCtx) -> anyhow::Result<Vec<edit::ConfigEdit>> {
    let mut out: Vec<edit::ConfigEdit> = Vec::new();
    for e in adapter.plan_uninstall(ctx)? {
        if out.iter().any(|o| o.path == e.path) {
            continue;
        }
        // 整文件托管的,「清空」就是删掉那个文件(只删带标记的,见 apply_file)
        let op = if e.op.is_file_op() { edit::EditOp::RemoveOwnedFile } else { edit::EditOp::PurgeOurs };
        out.push(edit::ConfigEdit { path: e.path, op });
    }
    Ok(out)
}

/// 已装的集成是否和当前服务一致。读不到 / 读坏了当作一致 —— `integration` 那边
/// 已经按「没装」处理了,这里不重复报。
fn install_is_current(adapter: &dyn AgentAdapter, ctx: &edit::InstallCtx) -> bool {
    let Ok(plan) = adapter.plan_install(ctx) else { return true };
    let mut paths: Vec<&std::path::Path> = Vec::new();
    for e in &plan {
        if !paths.contains(&e.path.as_path()) {
            paths.push(&e.path);
        }
    }
    paths.into_iter().all(|path| {
        // 整文件托管:一致 = 文本一模一样
        if let Some(edit::EditOp::WriteOwnedFile { content }) =
            plan.iter().filter(|e| e.path == path).map(|e| &e.op).find(|op| op.is_file_op())
        {
            return std::fs::read_to_string(path).is_ok_and(|t| &t == content);
        }
        let Some(doc) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            return true;
        };
        let ops: Vec<&edit::EditOp> = plan.iter().filter(|e| e.path == path).map(|e| &e.op).collect();
        edit::is_current(&doc, &ops, ctx.port)
    })
}

/// 扫描所有 adapter。单个 adapter 的失败只影响它自己那一条(局部失败原则)。
pub fn scan(env: &detect::DetectEnv, port: u16) -> Vec<AgentStatus> {
    let ctx = edit::InstallCtx { home: env.home.clone(), port, include_blocking: true };
    registry()
        .into_iter()
        .map(|a| {
            let (mut integration, conflicts) = a.integration(&ctx);
            if integration == Integration::Installed {
                // 和安装走同一个「带不带决策类」的取舍(见 routes::wants_blocking),
                // 否则有冲突时只装了遥测的用户会被永远报成不一致。
                let ctx = edit::InstallCtx { include_blocking: conflicts.is_empty(), ..ctx.clone() };
                if !install_is_current(a.as_ref(), &ctx) {
                    integration = Integration::Outdated;
                } else if !a.activated(env, &ctx) {
                    integration = Integration::NeedsActivation;
                }
            }
            AgentStatus {
                id: a.id(),
                name: a.name(),
                detected: a.detect(env),
                integration,
                activation: a.activation(),
                conflicts,
            }
        })
        .collect()
}

/// 启动自检:已经装了、但和这次启动的服务对不上的集成,一条一句警告。
///
/// **只报不修**。启动时重写别人的配置正是 `apply` 模块明确不做的事(和 agent 自己
/// 写同一个文件的竞态);修复仍然是用户的显式动作。没装的不报 —— 那是用户的选择。
pub fn startup_warnings(env: &detect::DetectEnv, port: u16) -> Vec<String> {
    scan(env, port)
        .into_iter()
        .filter_map(|a| {
            let path = a
                .detected
                .config_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let why = match a.integration {
                Integration::StalePort => format!("装的 hook 指向的不是当前端口 {port}"),
                Integration::Outdated => "装的 hook 和当前服务不一致(端口、事件或程序路径有变化)".to_string(),
                Integration::NeedsActivation => {
                    return Some(format!(
                        "{name}: hook 已装好,但还没在 {name} 里放行,不会被调用(会话识别、聊天视图都不生效)。\
                         {path}\n      {hint}",
                        name = a.name,
                        hint = activation_hint(a.activation),
                    ));
                }
                Integration::Installed | Integration::NotInstalled => return None,
            };
            Some(format!(
                "{name}: {why},远程接管 / 通知不会完全生效。{path}\n      \
                 在「设置 → AI 集成」里重新安装;若有残留,先用「清空」或 `octoterm-server purge-hooks`",
                name = a.name,
            ))
        })
        .collect()
}

/// 启动日志里「放行那一步怎么做」。客户端按 `activation` 键自己出文案,这里只服务日志。
fn activation_hint(key: Option<&str>) -> &'static str {
    match key {
        Some("codex-hooks-review") => "在 octoterm 里启动 codex,执行 /hooks 逐条审核通过,再重启 codex",
        Some("pi-reload") => "在 pi 里执行 /reload 或重启 pi",
        _ => "按设置页「AI 集成」里的提示完成最后一步",
    }
}
