//! 具体 Agent 适配器实现集合。
//!
//! 新增 Agent 时:在本目录下新建 `<agent>/mod.rs` 实现 [`super::AgentAdapter`],
//! 并将其静态实例加入 [`REGISTRY`]。核心框架层无需修改任何分支。

pub mod antigravity;
pub mod claude_code;
pub mod codex;
pub mod copilot;
pub mod grok;
pub mod opencode;
pub mod pi;

pub static REGISTRY: &[&dyn super::AgentAdapter] = &[
    &claude_code::ClaudeCode,
    &codex::Codex,
    &pi::Pi,
    &grok::Grok,
    &antigravity::Antigravity,
    &opencode::OpenCode,
    &copilot::Copilot,
];
