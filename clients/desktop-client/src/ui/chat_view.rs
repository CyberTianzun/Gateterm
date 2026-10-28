//! Structured AI Agent Chat Transcript View (`GET /api/agents/messages`).

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::ScrollableElement as _,
    v_flex,
};

use super::app::GatetermWorkbench;
use super::pane::TabViewMode;
use crate::client::api::{ChatBlock, ChatMessage, ChatRole};
use crate::i18n::Locale;

pub fn render_chat_view(
    workbench: &GatetermWorkbench,
    server_id: &str,
    session_id: u64,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let state_opt = workbench
        .servers
        .get(server_id)
        .and_then(|s| s.chat_states.get(&session_id));

    let agent_info = workbench
        .servers
        .get(server_id)
        .and_then(|s| crate::client::chat_agent_for_session(&s.agent_sessions, session_id));

    let header_title: SharedString = if let Some(agent) = &agent_info {
        format!(
            "{} · {} ({:?})",
            agent.agent_id, agent.agent_session_id, agent.state
        )
        .into()
    } else {
        locale
            .t("结构化对话视图", "Structured Agent Chat View")
            .into()
    };

    let srv_refresh = server_id.to_string();
    let srv_older = server_id.to_string();
    let srv_send = server_id.to_string();

    let older_cursor = state_opt.and_then(|st| st.older_cursor.clone());
    let fallback_reason = state_opt.and_then(|st| st.fallback_reason.clone());
    let messages: Vec<ChatMessage> = state_opt
        .map(|st| st.messages.clone())
        .unwrap_or_default();

    let header = h_flex()
        .w_full()
        .h(px(36.0))
        .px_3()
        .items_center()
        .justify_between()
        .bg(cx.theme().tab_bar)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(Icon::new(IconName::Bot).small().text_color(cx.theme().primary))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(cx.theme().foreground)
                        .child(header_title),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap_1p5()
                .children(older_cursor.map(|_| {
                    Button::new("chat-load-older")
                        .xsmall()
                        .ghost()
                        .label(locale.t("加载更早消息", "Load Older"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.fetch_chat_messages(srv_older.clone(), session_id, true, cx);
                        }))
                }))
                .child(
                    Button::new("chat-refresh")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Redo)
                        .label(locale.t("刷新", "Refresh"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.fetch_chat_messages(srv_refresh.clone(), session_id, false, cx);
                        })),
                )
                .child(
                    Button::new("chat-back-terminal")
                        .xsmall()
                        .secondary()
                        .icon(IconName::SquareTerminal)
                        .label(locale.t("返回终端", "Terminal View"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_active_tab_view_mode(TabViewMode::Terminal, window, cx);
                        })),
                ),
        );

    let body: AnyElement = if let Some(reason) = fallback_reason {
        let reason_desc = explain_fallback_reason(&reason, locale);
        v_flex()
            .flex_1()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .child(
                Icon::new(IconName::Info)
                    .large()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(SharedString::from(reason_desc)),
            )
            .child(
                Button::new("fallback-switch-term")
                    .small()
                    .primary()
                    .icon(IconName::SquareTerminal)
                    .label(locale.t("切换到终端视图", "Switch to Terminal View"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_active_tab_view_mode(TabViewMode::Terminal, window, cx);
                    })),
            )
            .into_any_element()
    } else if messages.is_empty() {
        v_flex()
            .flex_1()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .p_6()
            .child(
                Icon::new(IconName::Bot)
                    .large()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(locale.t(
                        "暂无结构化对话记录。若此会话正在运行 Claude / Codex / Gemini / OpenCode，请点击刷新。",
                        "No structured transcript messages yet. Click Refresh if an AI agent is running in this session.",
                    )),
            )
            .into_any_element()
    } else {
        render_message_list(&messages, locale, cx)
    };

    let input_bar = h_flex()
        .w_full()
        .p_2p5()
        .gap_2()
        .items_center()
        .bg(cx.theme().tab_bar)
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex_1()
                .child(Input::new(&workbench.chat_prompt_input).small()),
        )
        .child(
            Button::new("chat-send-prompt")
                .small()
                .primary()
                .label(locale.t("发送到终端", "Send to Terminal"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.send_chat_prompt_to_session(&srv_send, session_id, window, cx);
                })),
        );

    v_flex()
        .size_full()
        .overflow_hidden()
        .bg(cx.theme().background)
        .child(header)
        .child(div().flex_1().size_full().overflow_hidden().child(body))
        .child(input_bar)
        .into_any_element()
}

pub fn render_message_list(
    messages: &[ChatMessage],
    locale: Locale,
    cx: &App,
) -> AnyElement {
    let mut list = v_flex().w_full().p_4().gap_3();

    for (idx, msg) in messages.iter().enumerate() {
        let (role_label, role_bg, role_border, role_fg) = match msg.role {
            ChatRole::User => (
                locale.t("用户", "User"),
                cx.theme().primary.opacity(0.12),
                cx.theme().primary.opacity(0.35),
                cx.theme().primary,
            ),
            ChatRole::Assistant => (
                locale.t("Agent", "Assistant"),
                cx.theme().secondary.opacity(0.65),
                cx.theme().border,
                cx.theme().foreground,
            ),
            ChatRole::System => (
                locale.t("系统", "System"),
                cx.theme().muted.opacity(0.5),
                cx.theme().border,
                cx.theme().muted_foreground,
            ),
        };

        let mut card = v_flex()
            .id(SharedString::from(format!("chat-msg-{idx}-{}", msg.id)))
            .w_full()
            .p_3()
            .gap_2()
            .rounded(px(6.0))
            .bg(role_bg)
            .border_1()
            .border_color(role_border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(role_fg)
                            .child(role_label),
                    )
                    .children(msg.ts.map(|ts| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(format!("#{ts}")))
                    })),
            );

        for block in &msg.blocks {
            card = card.child(render_chat_block(block, locale, cx));
        }

        list = list.child(card);
    }

    div()
        .id("chat-transcript-scroll")
        .size_full()
        .overflow_y_scrollbar()
        .child(list)
        .into_any_element()
}

fn render_chat_block(block: &ChatBlock, locale: Locale, cx: &App) -> AnyElement {
    match block {
        ChatBlock::Text { text } => div()
            .w_full()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(SharedString::from(text.clone()))
            .into_any_element(),
        ChatBlock::Thinking { text } => v_flex()
            .w_full()
            .p_2()
            .gap_1()
            .rounded(px(4.0))
            .bg(cx.theme().background.opacity(0.6))
            .border_l_2()
            .border_color(cx.theme().muted_foreground)
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(locale.t("思考过程", "Thinking")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(text.clone())),
            )
            .into_any_element(),
        ChatBlock::ToolUse { name, input } => {
            let title: SharedString = format!("🔧 {} · {name}", locale.t("调用工具", "Tool Use")).into();
            let short_input: String = if input.chars().count() > 600 {
                let mut s: String = input.chars().take(600).collect();
                s.push_str(" …");
                s
            } else {
                input.clone()
            };
            v_flex()
                .w_full()
                .p_2()
                .gap_1()
                .rounded(px(4.0))
                .bg(cx.theme().background.opacity(0.75))
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().primary)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(short_input)),
                )
                .into_any_element()
        }
        ChatBlock::ToolResult { ok, text } => {
            let status_color = if *ok {
                cx.theme().success
            } else {
                cx.theme().danger
            };
            let title: SharedString = if *ok {
                locale.t("✓ 工具输出", "✓ Tool Output").into()
            } else {
                locale.t("✗ 工具错误", "✗ Tool Error").into()
            };
            let short_text: String = if text.chars().count() > 800 {
                let mut s: String = text.chars().take(800).collect();
                s.push_str(" …");
                s
            } else {
                text.clone()
            };
            v_flex()
                .w_full()
                .p_2()
                .gap_1()
                .rounded(px(4.0))
                .bg(cx.theme().background.opacity(0.65))
                .border_1()
                .border_color(status_color.opacity(0.4))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(status_color)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(short_text)),
                )
                .into_any_element()
        }
    }
}

fn explain_fallback_reason(reason: &str, locale: Locale) -> String {
    match reason {
        "disabled" => locale
            .t(
                "服务端已禁用结构化对话读取 (GATETERM_AGENT_TRANSCRIPT=0)。",
                "Structured transcript reading is disabled on the server (GATETERM_AGENT_TRANSCRIPT=0).",
            )
            .to_string(),
        "no-transcript-path" => locale
            .t(
                "当前终端尚未关联到可读取的 Agent 会话日志文件路径。",
                "No agent transcript path is associated with this terminal session yet.",
            )
            .to_string(),
        "unsupported-agent" => locale
            .t(
                "当前 Agent 暂不支持结构化对话解析，请使用终端视图。",
                "This agent does not support structured transcript parsing; please use Terminal View.",
            )
            .to_string(),
        "unreadable" => locale
            .t(
                "无法读取 Agent 会话日志文件（文件尚未生成或已被清理）。",
                "Agent transcript file is unreadable or has not been created yet.",
            )
            .to_string(),
        "parse-failed" => locale
            .t(
                "解析 Agent 会话日志失败，建议切换到终端视图查看原始输出。",
                "Failed to parse agent transcript; switch to Terminal View for raw output.",
            )
            .to_string(),
        other => format!(
            "{}: {other}",
            locale.t("结构化对话不可用", "Structured chat unavailable")
        ),
    }
}
