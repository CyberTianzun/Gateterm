//! Agent Session History Browser & Resume Modal
//! (`GET /api/agents/history`, `GET /api/agents/history/messages`, `POST /api/agents/history/open`).

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
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
use super::chat_view::render_message_list;
use crate::client::api::{ChatMessage, HistoryAgentFilter, HistoryEntry};

#[derive(Default)]
pub struct HistoryModalState {
    pub open: bool,
    pub server_id: String,
    pub agent_filter: Option<String>,
    pub entries: Vec<HistoryEntry>,
    pub agents: Vec<HistoryAgentFilter>,
    pub next_offset: Option<usize>,
    pub loading: bool,
    pub error: Option<String>,
    pub selected_key: Option<(String, String)>,
    pub preview_messages: Vec<ChatMessage>,
    pub preview_loading: bool,
}

pub fn render_history_modal(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> Option<AnyElement> {
    if !workbench.history_modal.open {
        return None;
    }

    let locale = workbench.locale;
    let state = &workbench.history_modal;
    let current_filter = state.agent_filter.clone();

    let mut filter_bar = h_flex().items_center().gap_1p5().flex_wrap().child(
        Button::new("hist-filter-all")
            .xsmall()
            .with_variant(if current_filter.is_none() {
                gpui_component::button::ButtonVariant::Primary
            } else {
                gpui_component::button::ButtonVariant::Ghost
            })
            .label(locale.t("全部", "All"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.history_modal.agent_filter = None;
                this.refresh_history_list(false, cx);
            })),
    );

    for agent in &state.agents {
        let aid = agent.id.clone();
        let active = current_filter.as_deref() == Some(aid.as_str());
        let label: SharedString = agent.name.clone().into();
        filter_bar = filter_bar.child(
            Button::new(SharedString::from(format!("hist-filter-{}", agent.id)))
                .xsmall()
                .with_variant(if active {
                    gpui_component::button::ButtonVariant::Primary
                } else {
                    gpui_component::button::ButtonVariant::Ghost
                })
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.history_modal.agent_filter = Some(aid.clone());
                    this.refresh_history_list(false, cx);
                })),
        );
    }

    let mut entry_list = v_flex().w_full().p_2().gap_1p5();
    if state.entries.is_empty() && !state.loading {
        entry_list = entry_list.child(
            div()
                .p_4()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(locale.t("没有找到匹配的 Agent 历史会话", "No matching agent history sessions found")),
        );
    }

    for (idx, entry) in state.entries.iter().enumerate() {
        let is_selected = state
            .selected_key
            .as_ref()
            .is_some_and(|(a, s)| a == &entry.agent_id && s == &entry.agent_session_id);
        let aid_select = entry.agent_id.clone();
        let sid_select = entry.agent_session_id.clone();
        let aid_open = entry.agent_id.clone();
        let sid_open = entry.agent_session_id.clone();
        let can_resume = entry.can_resume || entry.terminal_session.is_some();

        let title: SharedString = if entry.title.trim().is_empty() {
            entry.agent_session_id.clone().into()
        } else {
            entry.title.clone().into()
        };
        let subtitle: SharedString = format!(
            "{} · {}",
            entry.agent_name,
            entry.cwd.as_deref().unwrap_or("-")
        )
        .into();

        let open_label: SharedString = if entry.terminal_session.is_some() {
            locale.t("已打开 · 跳转", "Active · Focus").into()
        } else {
            locale.t("恢复会话", "Resume").into()
        };

        let item = v_flex()
            .id(SharedString::from(format!(
                "hist-item-{idx}-{}",
                entry.agent_session_id
            )))
            .w_full()
            .p_2p5()
            .gap_1()
            .rounded(px(6.0))
            .cursor_pointer()
            .bg(if is_selected {
                cx.theme().primary.opacity(0.16)
            } else {
                cx.theme().secondary.opacity(0.5)
            })
            .border_1()
            .border_color(if is_selected {
                cx.theme().primary
            } else {
                cx.theme().border
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_history_entry(aid_select.clone(), sid_select.clone(), cx);
            }))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(title),
                    )
                    .children(can_resume.then(|| {
                        Button::new(SharedString::from(format!(
                            "hist-open-{idx}-{}",
                            entry.agent_session_id
                        )))
                        .xsmall()
                        .primary()
                        .label(open_label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_history_entry(aid_open.clone(), sid_open.clone(), window, cx);
                        }))
                    })),
            )
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            );

        entry_list = entry_list.child(item);
    }

    if state.next_offset.is_some() {
        entry_list = entry_list.child(
            div().pt_2().child(
                Button::new("hist-load-more")
                    .xsmall()
                    .ghost()
                    .label(locale.t("加载更多", "Load More"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_history_list(true, cx);
                    })),
            ),
        );
    }

    let preview_pane: AnyElement = if state.preview_loading {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(locale.t("正在加载对话预览…", "Loading transcript preview...")),
            )
            .into_any_element()
    } else if state.preview_messages.is_empty() {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(locale.t(
                        "选择左侧的 Agent 历史会话以预览对话内容或恢复到终端。",
                        "Select an agent session on the left to preview its transcript or resume it in a terminal.",
                    )),
            )
            .into_any_element()
    } else {
        render_message_list(&state.preview_messages, locale, cx)
    };

    let dialog = v_flex()
        .id("history-modal-card")
        .occlude()
        .w(px(920.0))
        .h(px(620.0))
        .rounded(px(8.0))
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            h_flex()
                .w_full()
                .h(px(44.0))
                .px_4()
                .items_center()
                .justify_between()
                .bg(cx.theme().tab_bar)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Inbox).small().text_color(cx.theme().primary))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("Agent 历史会话浏览与恢复", "Agent Session History & Resume")),
                        ),
                )
                .child(
                    Button::new("hist-close")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Close)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.history_modal.open = false;
                            cx.notify();
                        })),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p_3()
                .gap_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&workbench.history_search_input).small()),
                        )
                        .child(
                            Button::new("hist-search-btn")
                                .small()
                                .primary()
                                .icon(IconName::Search)
                                .label(locale.t("搜索", "Search"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_history_list(false, cx);
                                })),
                        ),
                )
                .child(filter_bar),
        )
        .child(
            h_flex()
                .flex_1()
                .size_full()
                .overflow_hidden()
                .child(
                    div()
                        .id("hist-entries-scroll")
                        .w(px(360.0))
                        .h_full()
                        .border_r_1()
                        .border_color(cx.theme().border)
                        .overflow_y_scrollbar()
                        .child(entry_list),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .overflow_hidden()
                        .child(preview_pane),
                ),
        );

    Some(
        div()
            .id("history-modal-backdrop")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(gpui::black().opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.history_modal.open = false;
                    cx.notify();
                }),
            )
            .child(dialog)
            .into_any_element(),
    )
}
