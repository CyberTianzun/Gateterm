//! Shared custom SVG icons for `gateterm-desktop-native` aligned 1:1 with
//! `clients/web/src/index.html`, `clients/web/src/main.ts`, and
//! `clients/android/app/src/main/res/drawable/ic_*.xml`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatetermIcon {
    /// Agent session history icon (`clients/web/src/index.html#open-history` & `ic_history.xml`)
    History,
    /// Structured Agent Chat transcript icon (`clients/web/src/main.ts` & `ic_chat.xml`)
    Chat,
    /// Terminal view icon (`clients/web/src/main.ts` & `ic_terminal.xml`)
    Terminal,
    /// Show/expand sidebar icon (`clients/web/src/index.html#sidebar-show`)
    SidebarShow,
    /// Hide/collapse sidebar icon (`clients/web/src/main.ts#sidebar-position`)
    SidebarHide,
    /// Filter icon (`clients/android/app/src/main/res/drawable/ic_filter.xml`)
    Filter,
    /// Rename session pencil icon
    Pencil,
    /// QR code pairing icon (`clients/android/app/src/main/res/drawable/ic_qr_code_scanner.xml`)
    QrCode,
    /// Claude / Claude Code agent icon
    AgentClaude,
    /// OpenAI Codex agent icon
    AgentCodex,
    /// Google Gemini CLI agent icon
    AgentGemini,
    /// OpenCode agent icon
    AgentOpenCode,
    /// pi coding agent icon
    AgentPi,
    /// xAI Grok agent icon
    AgentGrok,
    /// Google Antigravity (AGY) agent icon
    AgentAgy,
    /// Fallback generic coding agent icon
    AgentGeneric,
}

impl GatetermIcon {
    pub fn for_agent(agent_id: &str) -> Self {
        let lower = agent_id.to_ascii_lowercase();
        if lower.contains("claude") {
            Self::AgentClaude
        } else if lower.contains("codex") {
            Self::AgentCodex
        } else if lower.contains("gemini") {
            Self::AgentGemini
        } else if lower.contains("opencode") {
            Self::AgentOpenCode
        } else if lower == "pi" || lower.starts_with("pi-") {
            Self::AgentPi
        } else if lower.contains("grok") {
            Self::AgentGrok
        } else if lower.contains("agy") || lower.contains("antigravity") {
            Self::AgentAgy
        } else {
            Self::AgentGeneric
        }
    }
}

impl IconNamed for GatetermIcon {
    fn path(self) -> SharedString {
        match self {
            Self::History => "gateterm/icons/history.svg".into(),
            Self::Chat => "gateterm/icons/chat.svg".into(),
            Self::Terminal => "gateterm/icons/terminal.svg".into(),
            Self::SidebarShow => "gateterm/icons/sidebar-show.svg".into(),
            Self::SidebarHide => "gateterm/icons/sidebar-hide.svg".into(),
            Self::Filter => "gateterm/icons/filter.svg".into(),
            Self::Pencil => "gateterm/icons/pencil.svg".into(),
            Self::QrCode => "gateterm/icons/qr-code.svg".into(),
            Self::AgentClaude => "gateterm/icons/agent-claude.svg".into(),
            Self::AgentCodex => "gateterm/icons/agent-codex.svg".into(),
            Self::AgentGemini => "gateterm/icons/agent-gemini.svg".into(),
            Self::AgentOpenCode => "gateterm/icons/agent-opencode.svg".into(),
            Self::AgentPi => "gateterm/icons/agent-pi.svg".into(),
            Self::AgentGrok => "gateterm/icons/agent-grok.svg".into(),
            Self::AgentAgy => "gateterm/icons/agent-agy.svg".into(),
            Self::AgentGeneric => "gateterm/icons/agent-generic.svg".into(),
        }
    }
}

const HISTORY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5M12 7v5l4 2"/></svg>"#;

const CHAT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 14a3 3 0 0 1-3 3H8l-5 4V6a3 3 0 0 1 3-3h12a3 3 0 0 1 3 3z"/><path d="M7 8h10M7 12h7"/></svg>"#;

const TERMINAL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3m6 0h4"/></svg>"#;

const SIDEBAR_SHOW_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16m5-11 3 3-3 3"/></svg>"#;

const SIDEBAR_HIDE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16m7-11-3 3 3 3"/></svg>"#;

const FILTER_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 5h18l-7 8v6l-4 2v-8z"/></svg>"#;

const PENCIL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/><path d="m15 5 4 4"/></svg>"#;

const QR_CODE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect width="5" height="5" x="3" y="3" rx="1"/><rect width="5" height="5" x="16" y="3" rx="1"/><rect width="5" height="5" x="3" y="16" rx="1"/><path d="M21 16h-3a2 2 0 0 0-2 2v3"/><path d="M21 21v.01"/><path d="M12 7v3a2 2 0 0 1-2 2H7"/><path d="M3 12h.01"/><path d="M12 3h.01"/><path d="M12 16v.01"/><path d="M16 12h1"/><path d="M21 12v.01"/><path d="M12 21v-1"/></svg>"#;

const AGENT_CLAUDE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M8 2v12M2 8h12M3.76 3.76l8.48 8.48M12.24 3.76L3.76 12.24"/></svg>"#;

const AGENT_CODEX_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 1.75L13.5 4.9v6.2L8 14.25 2.5 11.1V4.9L8 1.75Z"/><path d="M5.75 6.25L7.75 8l-2 1.75M8.5 9.75h2"/></svg>"#;

const AGENT_GEMINI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M8 1.5C8 5.1 10.9 8 14.5 8 10.9 8 8 10.9 8 14.5 8 10.9 5.1 8 1.5 8 5.1 8 8 5.1 8 1.5Z"/></svg>"#;

const AGENT_OPENCODE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M5.25 4.75L2 8l3.25 3.25M10.75 4.75L14 8l-3.25 3.25M9.25 3.5l-2.5 9"/></svg>"#;

const AGENT_PI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 4.5h10M6.25 4.5v7.25M10.25 4.5v5.75a1.5 1.5 0 0 0 1.5 1.5"/></svg>"#;

const AGENT_GROK_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="8" cy="8" r="4.75"/><path d="M3 13L13 3"/></svg>"#;

const AGENT_AGY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2.25L13.5 13H10.6L8 7.75 5.4 13H2.5L8 2.25Z"/><path d="M5.5 10.5h5"/></svg>"#;

const AGENT_GENERIC_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4.5" width="10" height="8" rx="2"/><path d="M8 2v2.5M6 8.25h.01M10 8.25h.01M6 10.5h4"/></svg>"#;

const ALL_CUSTOM_ICON_PATHS: &[&str] = &[
    "gateterm/icons/history.svg",
    "gateterm/icons/chat.svg",
    "gateterm/icons/terminal.svg",
    "gateterm/icons/sidebar-show.svg",
    "gateterm/icons/sidebar-hide.svg",
    "gateterm/icons/filter.svg",
    "gateterm/icons/pencil.svg",
    "gateterm/icons/qr-code.svg",
    "gateterm/icons/agent-claude.svg",
    "gateterm/icons/agent-codex.svg",
    "gateterm/icons/agent-gemini.svg",
    "gateterm/icons/agent-opencode.svg",
    "gateterm/icons/agent-pi.svg",
    "gateterm/icons/agent-grok.svg",
    "gateterm/icons/agent-agy.svg",
    "gateterm/icons/agent-generic.svg",
];

/// Composite asset source serving Gateterm's cross-platform icons (`clients/web` & `clients/android`)
/// alongside `gpui_component_assets::Assets`.
pub struct GatetermAssets;

impl AssetSource for GatetermAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            "gateterm/icons/history.svg" => Ok(Some(Cow::Borrowed(HISTORY_SVG))),
            "gateterm/icons/chat.svg" => Ok(Some(Cow::Borrowed(CHAT_SVG))),
            "gateterm/icons/terminal.svg" => Ok(Some(Cow::Borrowed(TERMINAL_SVG))),
            "gateterm/icons/sidebar-show.svg" => Ok(Some(Cow::Borrowed(SIDEBAR_SHOW_SVG))),
            "gateterm/icons/sidebar-hide.svg" => Ok(Some(Cow::Borrowed(SIDEBAR_HIDE_SVG))),
            "gateterm/icons/filter.svg" => Ok(Some(Cow::Borrowed(FILTER_SVG))),
            "gateterm/icons/pencil.svg" => Ok(Some(Cow::Borrowed(PENCIL_SVG))),
            "gateterm/icons/qr-code.svg" => Ok(Some(Cow::Borrowed(QR_CODE_SVG))),
            "gateterm/icons/agent-claude.svg" => Ok(Some(Cow::Borrowed(AGENT_CLAUDE_SVG))),
            "gateterm/icons/agent-codex.svg" => Ok(Some(Cow::Borrowed(AGENT_CODEX_SVG))),
            "gateterm/icons/agent-gemini.svg" => Ok(Some(Cow::Borrowed(AGENT_GEMINI_SVG))),
            "gateterm/icons/agent-opencode.svg" => Ok(Some(Cow::Borrowed(AGENT_OPENCODE_SVG))),
            "gateterm/icons/agent-pi.svg" => Ok(Some(Cow::Borrowed(AGENT_PI_SVG))),
            "gateterm/icons/agent-grok.svg" => Ok(Some(Cow::Borrowed(AGENT_GROK_SVG))),
            "gateterm/icons/agent-agy.svg" => Ok(Some(Cow::Borrowed(AGENT_AGY_SVG))),
            "gateterm/icons/agent-generic.svg" => Ok(Some(Cow::Borrowed(AGENT_GENERIC_SVG))),
            _ => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = gpui_component_assets::Assets.list(path)?;
        for &custom in ALL_CUSTOM_ICON_PATHS {
            if custom.starts_with(path) {
                items.push(custom.into());
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_gateterm_icons_load_valid_svg_bytes() {
        let icons = [
            GatetermIcon::History,
            GatetermIcon::Chat,
            GatetermIcon::Terminal,
            GatetermIcon::SidebarShow,
            GatetermIcon::SidebarHide,
            GatetermIcon::Filter,
            GatetermIcon::Pencil,
            GatetermIcon::QrCode,
            GatetermIcon::AgentClaude,
            GatetermIcon::AgentCodex,
            GatetermIcon::AgentGemini,
            GatetermIcon::AgentOpenCode,
            GatetermIcon::AgentPi,
            GatetermIcon::AgentGrok,
            GatetermIcon::AgentAgy,
            GatetermIcon::AgentGeneric,
        ];
        for icon in icons {
            let bytes = GatetermAssets
                .load(icon.path().as_ref())
                .expect("asset load should not error")
                .expect("asset should exist");
            let text = std::str::from_utf8(&bytes).expect("valid utf-8 svg");
            assert!(text.starts_with("<svg") && text.ends_with("</svg>"));
        }
        for builtin in [
            gpui_component::IconName::Plus,
            gpui_component::IconName::Search,
            gpui_component::IconName::Settings,
            gpui_component::IconName::WindowClose,
        ] {
            let bytes = GatetermAssets
                .load(builtin.path().as_ref())
                .expect("builtin asset load should not error")
                .expect("builtin asset should exist");
            assert!(!bytes.is_empty());
        }
    }

    #[test]
    fn maps_agent_ids_to_brand_icons() {
        assert_eq!(GatetermIcon::for_agent("claude-code"), GatetermIcon::AgentClaude);
        assert_eq!(GatetermIcon::for_agent("codex"), GatetermIcon::AgentCodex);
        assert_eq!(GatetermIcon::for_agent("gemini-cli"), GatetermIcon::AgentGemini);
        assert_eq!(GatetermIcon::for_agent("opencode"), GatetermIcon::AgentOpenCode);
        assert_eq!(GatetermIcon::for_agent("pi"), GatetermIcon::AgentPi);
        assert_eq!(GatetermIcon::for_agent("grok"), GatetermIcon::AgentGrok);
        assert_eq!(GatetermIcon::for_agent("agy"), GatetermIcon::AgentAgy);
        assert_eq!(GatetermIcon::for_agent("custom-bot"), GatetermIcon::AgentGeneric);
    }
}
