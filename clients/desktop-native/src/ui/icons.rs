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

const AGENT_CLAUDE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"/></svg>"#;

const AGENT_CODEX_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"/></svg>"#;

const AGENT_GEMINI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M20.616 10.835a14.147 14.147 0 01-4.45-3.001 14.111 14.111 0 01-3.678-6.452.503.503 0 00-.975 0 14.134 14.134 0 01-3.679 6.452 14.155 14.155 0 01-4.45 3.001c-.65.28-1.318.505-2.002.678a.502.502 0 000 .975c.684.172 1.35.397 2.002.677a14.147 14.147 0 014.45 3.001 14.112 14.112 0 013.679 6.453.502.502 0 00.975 0c.172-.685.397-1.351.677-2.003a14.145 14.145 0 013.001-4.45 14.113 14.113 0 016.453-3.678.503.503 0 000-.975 13.245 13.245 0 01-2.003-.678z"/></svg>"#;

const AGENT_OPENCODE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M16 6H8v12h8V6zm4 16H4V2h16v20z"/></svg>"#;

const AGENT_PI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 6.75h15M9.375 6.75v10.875M15.375 6.75v8.625a2.25 2.25 0 0 0 2.25 2.25"/></svg>"#;

const AGENT_GROK_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815"/></svg>"#;

const AGENT_AGY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z"/></svg>"#;

const AGENT_GENERIC_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4.5" y="6.75" width="15" height="12" rx="3"/><path d="M12 3v3.75M9 12.375h.015M15 12.375h.015M9 15.75h6"/></svg>"#;

/// Returns the canonical brand display label and primary brand color for a given `agent_id`.
pub fn agent_brand_style(agent_id: &str, cx: &gpui::App) -> (&'static str, gpui::Hsla) {
    let lower = agent_id.to_ascii_lowercase();
    if lower.contains("claude") {
        ("Claude", gpui::rgb(0xD97757).into())
    } else if lower.contains("codex") {
        ("Codex", gpui::rgb(0x7A9DFF).into())
    } else if lower.contains("gemini") {
        ("Gemini", gpui::rgb(0x3186FF).into())
    } else if lower.contains("opencode") {
        ("OpenCode", gpui::rgb(0x38BDF8).into())
    } else if lower == "pi" || lower.starts_with("pi-") {
        ("pi", gpui::rgb(0xF472B6).into())
    } else if lower.contains("grok") {
        ("Grok", gpui_component::ActiveTheme::theme(cx).foreground)
    } else if lower.contains("agy") || lower.contains("antigravity") {
        ("AGY", gpui::rgb(0x3186FF).into())
    } else {
        ("Agent", gpui_component::ActiveTheme::theme(cx).primary)
    }
}

/// Shared, reusable Agent brand icon component used across the Sidebar and Agent Session History.
pub fn render_agent_brand_icon(agent_id: &str, cx: &gpui::App) -> gpui::AnyElement {
    use gpui::{IntoElement as _, ParentElement as _, Styled as _, div, px};
    use gpui_component::{Icon, Sizable as _};

    let (_, brand_color) = agent_brand_style(agent_id, cx);
    div()
        .w(px(18.0))
        .h(px(18.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(GatetermIcon::for_agent(agent_id))
                .small()
                .text_color(brand_color),
        )
        .into_any_element()
}

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
