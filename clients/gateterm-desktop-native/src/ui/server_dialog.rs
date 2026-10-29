//! Add / Edit Remote Server Connection Modal.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex,
};

use super::app::GatetermWorkbench;

#[derive(Default)]
pub struct ServerDialogState {
    pub open: bool,
    pub editing_id: Option<String>,
    pub error: Option<String>,
}

pub fn render_server_dialog(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> Option<AnyElement> {
    if !workbench.server_dialog.open {
        return None;
    }

    let locale = workbench.locale;
    let is_edit = workbench.server_dialog.editing_id.is_some();
    let title = if is_edit {
        locale.t("编辑远程服务器", "Edit Remote Server")
    } else {
        locale.t("添加远程 Gateterm 服务器", "Add Remote Gateterm Server")
    };

    let dialog = v_flex()
        .id("server-dialog-card")
        .occlude()
        .w(px(500.0))
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
                        .child(
                            Icon::new(IconName::Network)
                                .small()
                                .text_color(cx.theme().primary),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(title),
                        ),
                )
                .child(
                    Button::new("srv-dialog-close")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Close)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.server_dialog.open = false;
                            cx.notify();
                        })),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p_4()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("名称 (可选)", "Display Name (optional)")),
                        )
                        .child(Input::new(&workbench.server_name_input).small()),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t(
                                    "服务器地址或完整访问链接 (支持 http://host:port/#token=...)",
                                    "Server URL or Access Link (supports http://host:port/#token=...)",
                                )),
                        )
                        .child(Input::new(&workbench.server_url_input).small()),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t(
                                    "访问 Token (若上方链接已含 #token=... 可留空)",
                                    "Access Token (optional if URL contains #token=...)",
                                )),
                        )
                        .child(Input::new(&workbench.server_token_input).small()),
                )
                .children(workbench.server_dialog.error.as_ref().map(|err| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(SharedString::from(err.clone()))
                }))
                .child(
                    h_flex()
                        .pt_2()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("srv-dialog-cancel")
                                .small()
                                .ghost()
                                .label(locale.t("取消", "Cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.server_dialog.open = false;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("srv-dialog-save")
                                .small()
                                .primary()
                                .label(locale.t("保存并连接", "Save & Connect"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_server_dialog(window, cx);
                                })),
                        ),
                ),
        );

    Some(
        div()
            .id("server-dialog-backdrop")
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
                    this.server_dialog.open = false;
                    cx.notify();
                }),
            )
            .child(dialog)
            .into_any_element(),
    )
}
