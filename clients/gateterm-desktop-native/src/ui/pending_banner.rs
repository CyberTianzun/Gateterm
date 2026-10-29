//! Pending AI Agent Tool Approval Banner (`GET /api/agents/pending`, `POST /api/agents/answer`).

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use super::app::GatetermWorkbench;
use crate::client::api::PendingRequestInfo;

pub fn render_pending_banners(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> Option<AnyElement> {
    let locale = workbench.locale;
    let mut rows: Vec<(String, String, PendingRequestInfo)> = Vec::new();
    for (server_id, conn) in &workbench.servers {
        for req in &conn.pending_requests {
            if workbench.dismissed_pending.contains(&req.id) {
                continue;
            }
            let session_name = req
                .session
                .and_then(|sid| conn.sessions.iter().find(|s| s.id == sid))
                .map(|s| s.name.clone())
                .unwrap_or_else(|| {
                    req.session
                        .map(|sid| format!("#{sid}"))
                        .unwrap_or_else(|| req.agent_session_id.clone())
                });
            rows.push((server_id.clone(), session_name, req.clone()));
        }
    }

    if rows.is_empty() {
        return None;
    }

    let warning_color = cx.theme().warning;
    let bg_color = warning_color.opacity(0.12);
    let border_color = warning_color.opacity(0.45);

    let mut container = v_flex()
        .w_full()
        .gap_1()
        .px_3()
        .py_1p5()
        .bg(bg_color)
        .border_b_1()
        .border_color(border_color);

    for (server_id, session_name, req) in rows {
        let req_id_allow = req.id.clone();
        let req_id_deny = req.id.clone();
        let req_id_dismiss = req.id.clone();
        let srv_allow = server_id.clone();
        let srv_deny = server_id.clone();
        let srv_jump = server_id.clone();
        let target_session = req.session;

        let agent_badge: SharedString = format!("[{} · {}]", req.agent_id, session_name).into();
        let summary_text: SharedString = req.summary().into();
        let allow_label: SharedString = locale.t("允许", "Allow").into();
        let deny_label: SharedString = locale.t("拒绝", "Deny").into();
        let dismiss_label: SharedString = locale.t("忽略", "Dismiss").into();
        let title_label: SharedString = locale.t("等待工具审批", "Pending Approval").into();

        let row = h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .small()
                            .text_color(warning_color),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(warning_color)
                            .child(title_label),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("pending-jump-{}", req.id)))
                            .cursor_pointer()
                            .text_xs()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.0))
                            .bg(cx.theme().secondary)
                            .text_color(cx.theme().foreground)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(sid) = target_session {
                                    this.attach_or_focus_session(&srv_jump, sid, None, window, cx);
                                }
                            }))
                            .child(agent_badge),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child(summary_text),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        Button::new(SharedString::from(format!("allow-{}", req.id)))
                            .xsmall()
                            .primary()
                            .label(allow_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.answer_pending_request(
                                    srv_allow.clone(),
                                    req_id_allow.clone(),
                                    "allow",
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("deny-{}", req.id)))
                            .xsmall()
                            .danger()
                            .label(deny_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.answer_pending_request(
                                    srv_deny.clone(),
                                    req_id_deny.clone(),
                                    "deny",
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("dismiss-{}", req.id)))
                            .xsmall()
                            .ghost()
                            .label(dismiss_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dismissed_pending.insert(req_id_dismiss.clone());
                                cx.notify();
                            })),
                    ),
            );

        container = container.child(row);
    }

    Some(container.into_any_element())
}
