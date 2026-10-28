//! `clients/web`-style New Session popup menu (`src/ui/new_session_modal.rs`):
//! Renders a clean modal/popover with a filter input at the top and launcher items
//! grouped by `provider` (`built-in`, `config`, `iterm2`, `windows-terminal`),
//! showing each launcher's `name` and `detail · cwd`.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
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
use crate::client::api::LauncherItem;
use crate::i18n::Locale;

#[derive(Default)]
pub struct NewSessionModalState {
    pub open: bool,
    pub selected_index: usize,
}

fn provider_title(provider: &str, locale: Locale) -> &'static str {
    match provider {
        "built-in" => locale.t("内置", "Built-in"),
        "config" => locale.t("自定义", "Custom"),
        "iterm2" => "iTerm2",
        "windows-terminal" => "Windows Terminal",
        _ => locale.t("其他", "Other"),
    }
}

pub fn filtered_launchers(launchers: &[LauncherItem], query: &str) -> Vec<LauncherItem> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return launchers.to_vec();
    }
    launchers
        .iter()
        .filter(|l| {
            let hay = format!(
                "{} {} {} {}",
                l.name,
                l.detail,
                l.provider,
                l.cwd.as_deref().unwrap_or("")
            )
            .to_lowercase();
            hay.contains(&q)
        })
        .cloned()
        .collect()
}

pub fn render_new_session_modal(
    app: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> Option<AnyElement> {
    if !app.new_session_modal.open {
        return None;
    }

    let locale = app.locale;
    let active_srv_id = app.active_server_id.clone();
    let conn = app.servers.get(&active_srv_id)?;
    let query = app.new_session_filter_input.read(cx).value().to_string();
    let items = filtered_launchers(&conn.launchers, &query);

    let mut groups: Vec<(String, Vec<(usize, LauncherItem)>)> = Vec::new();
    for (flat_idx, item) in items.iter().cloned().enumerate() {
        if let Some((_, list)) = groups.iter_mut().find(|(p, _)| p == &item.provider) {
            list.push((flat_idx, item));
        } else {
            groups.push((item.provider.clone(), vec![(flat_idx, item)]));
        }
    }

    let selected_idx = if items.is_empty() {
        0
    } else {
        app.new_session_modal.selected_index.min(items.len() - 1)
    };

    let mut list_col = v_flex().w_full().gap_1().p_1p5();

    if items.is_empty() {
        let srv_default = active_srv_id.clone();
        list_col = list_col.child(
            v_flex()
                .w_full()
                .py_4()
                .px_3()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(locale.t("无匹配项", "No matching launchers")),
                )
                .child(
                    Button::new("ns-fallback-default-shell")
                        .small()
                        .primary()
                        .icon(IconName::Plus)
                        .label(locale.t("启动默认终端", "Launch Default Shell"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.new_session_modal.open = false;
                            this.create_new_session(&srv_default, None, None, None, None, cx);
                        })),
                ),
        );
    } else {
        for (provider, group_items) in groups {
            list_col = list_col.child(
                div()
                    .px_2p5()
                    .pt_2()
                    .pb_0p5()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(provider_title(&provider, locale)),
            );

            for (flat_idx, item) in group_items {
                let is_selected = flat_idx == selected_idx;
                let srv_click = active_srv_id.clone();

                let name_opt = Some(item.name.clone());
                let cmd_opt = (!item.command.is_empty()).then(|| item.command.clone());
                let cwd_opt = item.cwd.clone();

                let meta_text = match (&item.detail.is_empty(), &item.cwd) {
                    (false, Some(cwd)) if !cwd.is_empty() => format!("{} · {}", item.detail, cwd),
                    (false, _) => item.detail.clone(),
                    (true, Some(cwd)) if !cwd.is_empty() => cwd.clone(),
                    _ => String::new(),
                };

                let row = h_flex()
                    .id(SharedString::from(format!("ns-item-{flat_idx}")))
                    .w_full()
                    .px_2p5()
                    .py_2()
                    .rounded(px(6.0))
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .cursor_pointer()
                    .bg(if is_selected {
                        cx.theme().primary.opacity(0.16)
                    } else {
                        cx.theme().transparent
                    })
                    .hover(|s| s.bg(cx.theme().secondary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.new_session_modal.open = false;
                        this.create_new_session(
                            &srv_click,
                            name_opt.clone(),
                            cmd_opt.clone(),
                            cwd_opt.clone(),
                            None,
                            cx,
                        );
                    }))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        Icon::new(if item.provider == "built-in" {
                                            IconName::SquareTerminal
                                        } else {
                                            IconName::Bot
                                        })
                                        .xsmall()
                                        .text_color(if is_selected {
                                            cx.theme().primary
                                        } else {
                                            cx.theme().muted_foreground
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(cx.theme().foreground)
                                            .truncate()
                                            .child(SharedString::from(item.name.clone())),
                                    ),
                            )
                            .children((!meta_text.is_empty()).then(|| {
                                div()
                                    .pl_4()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(SharedString::from(meta_text))
                            })),
                    );

                list_col = list_col.child(row);
            }
        }
    }

    Some(
        div()
            .id("new-session-modal-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.25))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.new_session_modal.open = false;
                cx.notify();
            }))
            .child(
                v_flex()
                    .id("new-session-popover-card")
                    .occlude()
                    .absolute()
                    .top(px(76.0))
                    .left(px(12.0))
                    .w(px(340.0))
                    .max_h(px(460.0))
                    .rounded(px(8.0))
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .w_full()
                            .p_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                Input::new(&app.new_session_filter_input)
                                    .small()
                                    .prefix(
                                        Icon::new(IconName::Search)
                                            .xsmall()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .cleanable(true),
                            ),
                    )
                    .child(
                        div()
                            .id("new-session-items-scroll")
                            .flex_1()
                            .min_h(px(0.0))
                            .w_full()
                            .overflow_y_scrollbar()
                            .child(list_col),
                    ),
            )
            .into_any_element(),
    )
}
