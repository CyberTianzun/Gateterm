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

const AGENT_CLAUDE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M 4.709 15.955 L 9.429 13.308 L 9.509 13.078 L 9.429 12.95 L 9.2 12.95 L 8.41 12.902 L 5.712 12.829 L 3.373 12.732 L 1.107 12.61 L 0.536 12.489 L 0 11.784 L 0.055 11.432 L 0.535 11.111 L 1.221 11.171 L 2.741 11.274 L 5.019 11.432 L 6.671 11.529 L 9.12 11.784 L 9.509 11.784 L 9.564 11.627 L 9.43 11.529 L 9.327 11.432 L 6.969 9.836 L 4.417 8.148 L 3.081 7.176 L 2.357 6.685 L 1.993 6.223 L 1.835 5.215 L 2.491 4.493 L 3.372 4.553 L 3.597 4.614 L 4.49 5.3 L 6.398 6.776 L 8.889 8.609 L 9.254 8.913 L 9.399 8.81 L 9.418 8.737 L 9.254 8.463 L 7.899 6.017 L 6.453 3.527 L 5.809 2.495 L 5.639 1.876 C 5.574 1.638 5.539 1.393 5.535 1.147 L 6.283 0.134 L 6.696 0 L 7.692 0.134 L 8.112 0.498 L 8.732 1.912 L 9.734 4.141 L 11.289 7.171 L 11.745 8.069 L 11.988 8.901 L 12.079 9.156 L 12.237 9.156 L 12.237 9.01 L 12.365 7.304 L 12.602 5.209 L 12.832 2.514 L 12.912 1.754 L 13.288 0.844 L 14.035 0.352 L 14.619 0.632 L 15.099 1.317 L 15.032 1.761 L 14.746 3.612 L 14.187 6.515 L 13.823 8.457 L 14.035 8.457 L 14.278 8.215 L 15.263 6.909 L 16.915 4.845 L 17.645 4.025 L 18.495 3.121 L 19.042 2.69 L 20.075 2.69 L 20.835 3.819 L 20.495 4.985 L 19.431 6.332 L 18.55 7.474 L 17.286 9.174 L 16.496 10.534 L 16.569 10.644 L 16.757 10.624 L 19.613 10.018 L 21.156 9.738 L 22.997 9.423 L 23.83 9.811 L 23.921 10.206 L 23.593 11.013 L 21.624 11.499 L 19.315 11.961 L 15.876 12.774 L 15.834 12.804 L 15.883 12.865 L 17.432 13.011 L 18.094 13.047 L 19.716 13.047 L 22.736 13.272 L 23.526 13.794 L 24 14.432 L 23.921 14.917 L 22.706 15.537 L 21.066 15.148 L 17.237 14.238 L 15.925 13.909 L 15.743 13.909 L 15.743 14.019 L 16.836 15.087 L 18.842 16.897 L 21.351 19.227 L 21.478 19.805 L 21.156 20.26 L 20.816 20.211 L 18.611 18.554 L 17.76 17.807 L 15.834 16.187 L 15.706 16.187 L 15.706 16.357 L 16.15 17.006 L 18.495 20.527 L 18.617 21.607 L 18.447 21.96 L 17.839 22.173 L 17.171 22.051 L 15.797 20.126 L 14.382 17.959 L 13.239 16.016 L 13.099 16.096 L 12.425 23.35 L 12.109 23.72 L 11.38 24 L 10.773 23.539 L 10.451 22.792 L 10.773 21.316 L 11.162 19.392 L 11.477 17.862 L 11.763 15.962 L 11.933 15.33 L 11.921 15.288 L 11.781 15.306 L 10.347 17.273 L 8.167 20.218 L 6.441 22.063 L 6.027 22.227 L 5.31 21.857 L 5.377 21.195 L 5.778 20.606 L 8.166 17.57 L 9.606 15.688 L 10.536 14.602 L 10.53 14.444 L 10.475 14.444 L 4.132 18.56 L 3.002 18.706 L 2.515 18.25 L 2.576 17.504 L 2.807 17.261 L 4.715 15.949 L 4.709 15.955 Z"/></svg>"#;

const AGENT_CODEX_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M 8.086 0.457 C 9.049 0.061 10.098 -0.082 11.132 0.042 C 12.465 0.195 13.653 0.762 14.696 1.742 C 14.725 1.769 14.765 1.78 14.803 1.771 C 16.211 1.425 17.565 1.547 18.864 2.137 L 18.927 2.167 L 19.081 2.243 C 20.438 2.946 21.411 4.013 21.999 5.441 C 22.277 6.12 22.417 6.829 22.42 7.567 C 22.44 8.117 22.379 8.666 22.24 9.198 C 22.226 9.253 22.241 9.312 22.28 9.353 C 23.066 10.15 23.613 11.152 23.858 12.244 C 24.243 14.145 23.848 15.859 22.675 17.384 L 22.493 17.604 C 21.716 18.494 20.696 19.137 19.559 19.455 C 19.509 19.47 19.468 19.508 19.451 19.557 C 19.196 20.293 18.94 20.921 18.464 21.549 C 17.265 23.131 15.502 24.011 13.516 24 C 11.933 23.992 10.53 23.413 9.306 22.264 C 9.268 22.229 9.215 22.217 9.166 22.232 C 8.648 22.399 8.126 22.423 7.562 22.417 C 6.661 22.41 5.773 22.197 4.967 21.795 C 4.123 21.376 3.388 20.766 2.821 20.014 C 2.618 19.745 2.417 19.492 2.27 19.193 C 2.067 18.781 1.902 18.352 1.775 17.91 C 1.509 16.907 1.503 15.852 1.758 14.846 C 1.766 14.822 1.769 14.797 1.766 14.772 C 1.761 14.747 1.748 14.725 1.729 14.708 C 1.113 14.084 0.641 13.333 0.349 12.506 C 0.155 11.997 0.043 11.461 0.016 10.917 C -0.032 10.201 0.031 9.482 0.204 8.785 C 0.654 7.301 1.513 6.137 2.781 5.292 C 3.063 5.104 3.331 4.958 3.583 4.854 C 3.869 4.734 4.156 4.634 4.444 4.55 C 4.486 4.538 4.519 4.505 4.531 4.463 C 4.749 3.678 5.125 2.945 5.635 2.31 C 6.315 1.464 7.132 0.846 8.086 0.457 Z M 7.282 8.307 C 7.049 7.9 6.531 7.759 6.125 7.992 C 5.718 8.224 5.576 8.742 5.809 9.149 L 7.503 12.114 L 5.815 14.962 C 5.6 15.363 5.739 15.862 6.131 16.094 C 6.522 16.326 7.027 16.207 7.275 15.826 L 9.215 12.554 C 9.371 12.291 9.373 11.965 9.222 11.7 L 7.282 8.307 Z M 12.728 14.547 C 12.279 14.574 11.929 14.945 11.929 15.395 C 11.929 15.844 12.279 16.215 12.728 16.242 L 17.576 16.242 C 18.028 16.22 18.384 15.847 18.384 15.394 C 18.384 14.941 18.028 14.568 17.576 14.546 L 12.728 14.546 Z"/></svg>"#;

const AGENT_GEMINI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M 20.616 10.835 C 18.955 10.128 17.444 9.109 16.166 7.834 C 14.386 6.05 13.116 3.822 12.488 1.382 C 12.431 1.159 12.231 1.003 12 1.003 C 11.77 1.003 11.57 1.159 11.513 1.382 C 10.884 3.822 9.614 6.05 7.834 7.834 C 6.556 9.109 5.045 10.128 3.384 10.835 C 2.734 11.115 2.066 11.34 1.382 11.513 C 1.158 11.568 1 11.769 1 12.001 C 1 12.232 1.158 12.433 1.382 12.488 C 2.066 12.66 2.732 12.885 3.384 13.165 C 5.045 13.872 6.556 14.891 7.834 16.166 C 9.615 17.95 10.885 20.178 11.513 22.619 C 11.568 22.843 11.769 23.001 12 23.001 C 12.232 23.001 12.433 22.843 12.488 22.619 C 12.66 21.934 12.885 21.268 13.165 20.616 C 13.872 18.955 14.891 17.444 16.166 16.166 C 17.95 14.386 20.178 13.116 22.619 12.488 C 22.842 12.431 22.998 12.231 22.998 12 C 22.998 11.77 22.842 11.57 22.619 11.513 C 21.934 11.341 21.265 11.114 20.616 10.835 Z"/></svg>"#;

const AGENT_OPENCODE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M 16 6 L 8 6 L 8 18 L 16 18 L 16 6 Z M 20 22 L 4 22 L 4 2 L 20 2 L 20 22 Z"/></svg>"#;

const AGENT_PI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M 4.5 6.75 L 19.5 6.75 M 9.375 6.75 L 9.375 17.625 M 15.375 6.75 L 15.375 15.375 C 15.375 16.618 16.382 17.625 17.625 17.625"/></svg>"#;

const AGENT_GROK_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M 9.27 15.29 L 17.248 9.393 C 17.639 9.103 18.198 9.216 18.385 9.665 C 19.365 12.034 18.927 14.88 16.975 16.834 C 15.024 18.788 12.308 19.216 9.826 18.24 L 7.115 19.497 C 11.004 22.158 15.726 21.5 18.677 18.544 C 21.018 16.2 21.743 13.005 21.065 10.124 L 21.071 10.131 C 20.088 5.899 21.313 4.207 23.821 0.748 C 23.881 0.666 23.941 0.584 24 0.5 L 20.699 3.805 L 20.699 3.795 L 9.267 15.292 Z M 7.623 16.723 C 4.831 14.053 5.313 9.922 7.694 7.539 C 9.455 5.776 12.341 5.056 14.86 6.114 L 17.565 4.864 C 17.003 4.45 16.388 4.114 15.736 3.864 C 12.386 2.493 8.541 3.268 5.984 5.83 C 3.451 8.366 2.654 12.266 4.022 15.594 C 5.044 18.081 3.369 19.84 1.682 21.616 C 1.083 22.246 0.483 22.875 0 23.541 L 7.62 16.726 Z"/></svg>"#;

const AGENT_AGY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path fill-rule="evenodd" d="M 21.751 22.607 C 23.091 23.612 25.101 22.942 23.259 21.099 C 17.73 15.74 18.904 1 12.037 1 C 5.17 1 6.342 15.74 0.815 21.1 C -1.195 23.109 0.982 23.611 2.322 22.606 C 7.514 19.089 7.179 12.892 12.037 12.892 C 16.894 12.892 16.559 19.089 21.751 22.607 Z"/></svg>"#;

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
