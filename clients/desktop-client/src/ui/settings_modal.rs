//! Appearance, Terminal, Local Embedded Server, AI Agent Hooks, and Cloud Settings Modal.

use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::ScrollableElement as _,
    v_flex,
};

use super::app::GatetermWorkbench;
use crate::client::api::{AgentIntegrationStatus, CloudStatus};
use crate::i18n::LanguagePref;
use crate::theme::TerminalPaletteId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Appearance,
    LocalServer,
    AgentHooks,
    Cloud,
}

#[derive(Default)]
pub struct SettingsModalState {
    pub open: bool,
    pub section: SettingsSection,
    pub agent_hooks: Vec<AgentIntegrationStatus>,
    pub hooks_loading: bool,
    pub hooks_message: Option<String>,
    pub cloud_status: Option<CloudStatus>,
}

pub fn render_settings_modal(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> Option<AnyElement> {
    if !workbench.settings_modal.open {
        return None;
    }

    let locale = workbench.locale;
    let active_sec = workbench.settings_modal.section;

    let nav = v_flex()
        .w(px(190.0))
        .h_full()
        .p_2p5()
        .gap_1()
        .bg(cx.theme().sidebar)
        .border_r_1()
        .border_color(cx.theme().border)
        .child(
            Button::new("set-nav-appearance")
                .small()
                .with_variant(if active_sec == SettingsSection::Appearance {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .icon(IconName::Palette)
                .label(locale.t("外观与终端", "Appearance & Font"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_modal.section = SettingsSection::Appearance;
                    cx.notify();
                })),
        )
        .child(
            Button::new("set-nav-local")
                .small()
                .with_variant(if active_sec == SettingsSection::LocalServer {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .icon(IconName::HardDrive)
                .label(locale.t("内置本地服务", "Embedded Server"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_modal.section = SettingsSection::LocalServer;
                    cx.notify();
                })),
        )
        .child(
            Button::new("set-nav-hooks")
                .small()
                .with_variant(if active_sec == SettingsSection::AgentHooks {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .icon(IconName::Bot)
                .label(locale.t("AI Agent 钩子", "AI Agent Hooks"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_modal.section = SettingsSection::AgentHooks;
                    this.refresh_settings_hooks_and_cloud(cx);
                    cx.notify();
                })),
        )
        .child(
            Button::new("set-nav-cloud")
                .small()
                .with_variant(if active_sec == SettingsSection::Cloud {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .icon(IconName::Globe)
                .label(locale.t("Cloud 互联网中继", "Gateterm Cloud"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_modal.section = SettingsSection::Cloud;
                    this.refresh_settings_hooks_and_cloud(cx);
                    cx.notify();
                })),
        );

    let content: AnyElement = match active_sec {
        SettingsSection::Appearance => render_appearance_section(workbench, cx),
        SettingsSection::LocalServer => render_local_server_section(workbench, cx),
        SettingsSection::AgentHooks => render_agent_hooks_section(workbench, cx),
        SettingsSection::Cloud => render_cloud_section(workbench, cx),
    };

    let dialog = v_flex()
        .id("settings-modal-card")
        .occlude()
        .w(px(780.0))
        .h(px(540.0))
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
                            Icon::new(IconName::Settings)
                                .small()
                                .text_color(cx.theme().primary),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("Gateterm 桌面端设置", "Gateterm Desktop Settings")),
                        ),
                )
                .child(
                    Button::new("settings-close")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Close)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings_modal.open = false;
                            cx.notify();
                        })),
                ),
        )
        .child(
            h_flex()
                .flex_1()
                .size_full()
                .overflow_hidden()
                .child(nav)
                .child(
                    div()
                        .id("settings-content-scroll")
                        .flex_1()
                        .h_full()
                        .overflow_y_scrollbar()
                        .child(content),
                ),
        );

    Some(
        div()
            .id("settings-modal-backdrop")
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
                    this.settings_modal.open = false;
                    cx.notify();
                }),
            )
            .child(dialog)
            .into_any_element(),
    )
}

fn render_appearance_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let current_palette = workbench.config.palette;
    let current_lang = workbench.config.language;

    let mut palette_row = h_flex().gap_2().flex_wrap();
    for pid in TerminalPaletteId::ALL {
        let active = pid == current_palette;
        palette_row = palette_row.child(
            Button::new(SharedString::from(format!("pal-{:?}", pid)))
                .small()
                .with_variant(if active {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                })
                .label(pid.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_palette(pid, cx);
                })),
        );
    }

    let mut lang_row = h_flex().gap_2();
    for lang in LanguagePref::ALL {
        let active = lang == current_lang;
        lang_row = lang_row.child(
            Button::new(SharedString::from(format!("lang-{:?}", lang)))
                .small()
                .with_variant(if active {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                })
                .label(lang.label(locale))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_language(lang, cx);
                })),
        );
    }

    let font_size_str: SharedString = format!("{:.0} px", workbench.config.font_size).into();
    let line_height_str: SharedString = format!("{:.2}x", workbench.config.line_height).into();

    v_flex()
        .w_full()
        .p_5()
        .gap_4()
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t("终端与界面配色主题", "Color Theme & Terminal Palette")),
                )
                .child(palette_row),
        )
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t("界面语言", "Interface Language")),
                )
                .child(lang_row),
        )
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t("终端等宽字体 (Font Family)", "Terminal Monospace Font Family")),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&workbench.font_family_input).small()),
                        )
                        .child(
                            Button::new("apply-font-family")
                                .small()
                                .primary()
                                .label(locale.t("应用字体", "Apply Font"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.apply_font_family_from_input(cx);
                                })),
                        ),
                ),
        )
        .child(
            h_flex()
                .gap_6()
                .items_center()
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("字号大小", "Font Size")),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("font-size-dec")
                                        .small()
                                        .secondary()
                                        .label("-")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.adjust_font_size(-1.0, cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .w(px(56.0))
                                        .text_center()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(font_size_str),
                                )
                                .child(
                                    Button::new("font-size-inc")
                                        .small()
                                        .secondary()
                                        .label("+")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.adjust_font_size(1.0, cx);
                                        })),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("行高倍率", "Line Height")),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("line-height-dec")
                                        .small()
                                        .secondary()
                                        .label("-")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.adjust_line_height(-0.05, cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .w(px(56.0))
                                        .text_center()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(line_height_str),
                                )
                                .child(
                                    Button::new("line-height-inc")
                                        .small()
                                        .secondary()
                                        .label("+")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.adjust_line_height(0.05, cx);
                                        })),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

fn render_local_server_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let enabled = workbench.config.start_embedded_server;
    let running_url = workbench
        .local_supervisor
        .as_ref()
        .map(|s| s.access_url());

    v_flex()
        .w_full()
        .p_5()
        .gap_4()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t(
                                    "随桌面端启动内置 gateterm-server",
                                    "Start Embedded Local gateterm-server Automatically",
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(locale.t(
                                    "无需单独运行命令行服务端，即可直接打开本机 PTY 终端与本地 Agent 会话。",
                                    "Spawn local PTY shells and monitor local AI agents without running an external daemon.",
                                )),
                        ),
                )
                .child(
                    Button::new("toggle-embedded-srv")
                        .small()
                        .with_variant(if enabled {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .label(if enabled {
                            locale.t("已启用", "Enabled")
                        } else {
                            locale.t("已禁用", "Disabled")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_embedded_server(cx);
                        })),
                ),
        )
        .child(
            v_flex()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t(
                            "监听地址 (默认 127.0.0.1:7681，端口被占用时自动回退)",
                            "Listen Address (default 127.0.0.1:7681, falls back automatically if busy)",
                        )),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&workbench.embedded_listen_input).small()),
                        )
                        .child(
                            Button::new("save-embedded-listen")
                                .small()
                                .primary()
                                .label(locale.t("保存并重启内置服务", "Save & Restart"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.save_and_restart_embedded_server(cx);
                                })),
                        ),
                ),
        )
        .children(running_url.map(|url| {
            let url_copy = url.clone();
            v_flex()
                .p_3()
                .gap_2()
                .rounded(px(6.0))
                .bg(cx.theme().secondary.opacity(0.6))
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().success)
                        .child(locale.t(
                            "● 内置服务正在运行（可在浏览器或移动端通过以下链接访问）",
                            "● Embedded Server Running (accessible via Web/Android using this URL)",
                        )),
                )
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
                                .text_color(cx.theme().foreground)
                                .child(SharedString::from(url)),
                        )
                        .child(
                            Button::new("copy-embedded-url")
                                .xsmall()
                                .secondary()
                                .icon(IconName::Copy)
                                .label(locale.t("复制链接", "Copy URL"))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        url_copy.clone(),
                                    ));
                                })),
                        ),
                )
        }))
        .into_any_element()
}

fn render_agent_hooks_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let hooks = &workbench.settings_modal.agent_hooks;

    let mut list = v_flex().w_full().gap_2p5();
    if hooks.is_empty() {
        list = list.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(locale.t(
                    "正在从当前活动服务器获取 Agent 钩子状态…",
                    "Fetching AI Agent hook status from active server...",
                )),
        );
    }

    for agent in hooks {
        let aid_inst = agent.id.clone();
        let aid_uninst = agent.id.clone();
        let status_badge: SharedString = if agent.installed {
            locale.t("✓ 已安装钩子", "✓ Hook Installed").into()
        } else {
            locale.t("未安装钩子", "Not Installed").into()
        };
        let status_color = if agent.installed {
            cx.theme().success
        } else {
            cx.theme().muted_foreground
        };
        let detail_text: SharedString = if agent.integration.is_empty() {
            agent.id.clone().into()
        } else {
            format!("{} · {}", agent.id, agent.integration).into()
        };

        let card = h_flex()
            .w_full()
            .p_3()
            .items_center()
            .justify_between()
            .rounded(px(6.0))
            .bg(cx.theme().secondary.opacity(0.5))
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().foreground)
                                    .child(SharedString::from(agent.name.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(status_color)
                                    .child(status_badge),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail_text),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        Button::new(SharedString::from(format!("hook-inst-{}", agent.id)))
                            .xsmall()
                            .primary()
                            .label(locale.t("安装钩子", "Install"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.manage_agent_hook(aid_inst.clone(), "install", cx);
                            })),
                    )
                    .children(agent.installed.then(|| {
                        Button::new(SharedString::from(format!("hook-uninst-{}", agent.id)))
                            .xsmall()
                            .ghost()
                            .label(locale.t("卸载", "Uninstall"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.manage_agent_hook(aid_uninst.clone(), "uninstall", cx);
                            }))
                    })),
            );

        list = list.child(card);
    }

    v_flex()
        .w_full()
        .p_5()
        .gap_3()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t(
                            "AI Coding Agent 状态与权限审批钩子",
                            "AI Coding Agent Status & Approval Hooks",
                        )),
                )
                .child(
                    Button::new("hooks-refresh")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Redo)
                        .label(locale.t("刷新", "Refresh"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_settings_hooks_and_cloud(cx);
                        })),
                ),
        )
        .children(workbench.settings_modal.hooks_message.as_ref().map(|msg| {
            div()
                .text_xs()
                .text_color(cx.theme().primary)
                .child(SharedString::from(msg.clone()))
        }))
        .child(list)
        .into_any_element()
}

fn render_cloud_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let status = &workbench.settings_modal.cloud_status;

    v_flex()
        .w_full()
        .p_5()
        .gap_3()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(locale.t("Gateterm Cloud 中继连接状态", "Gateterm Cloud Relay Status")),
        )
        .child(if let Some(cloud) = status {
            let state_text: SharedString = format!(
                "{}: {}",
                locale.t("状态", "State"),
                if cloud.connected {
                    locale.t("已连接", "Connected")
                } else {
                    locale.t("未登录 / 未连接", "Not signed in / Disconnected")
                }
            )
            .into();
            v_flex()
                .p_3()
                .gap_2()
                .rounded(px(6.0))
                .bg(cx.theme().secondary.opacity(0.5))
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if cloud.connected {
                            cx.theme().success
                        } else {
                            cx.theme().foreground
                        })
                        .child(state_text),
                )
                .children(cloud.email.as_ref().map(|email| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(format!("Account: {email}")))
                }))
                .into_any_element()
        } else {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(locale.t(
                    "当前服务器未启用 Gateterm Cloud 或正在获取状态。",
                    "Gateterm Cloud is not enabled on the active server or status is loading.",
                ))
                .into_any_element()
        })
        .into_any_element()
}
