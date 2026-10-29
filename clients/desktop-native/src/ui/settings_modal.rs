//! Full Desktop Settings Modal:
//! - Appearance, Font, Autostart, Close-to-Tray, `config.toml` & Logs (`SettingsSection::Appearance`)
//! - Embedded Server Host/Port/Token, Save & Apply, and Android QR Code Pairing (`SettingsSection::LocalServer`)
//! - AI Coding Agent Hooks (`SettingsSection::AgentHooks`)
//! - Gateterm Cloud Account Sign-In (Browser PKCE OAuth), Sign-Out & Account Deletion (`SettingsSection::Cloud`)

use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::ScrollableElement as _,
    v_flex,
};

use super::app::GatetermWorkbench;
use crate::client::api::{AgentIntegrationStatus, CloudStatus};
use crate::i18n::LanguagePref;
use crate::system::{
    cloud::{CloudAccountStatus, CloudAction, RelayRegion},
    configfile::ServerConfigSummary,
    pairing::{PairingState, QrMatrix},
};
use crate::theme::TerminalPaletteId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Appearance,
    LocalServer,
    AgentHooks,
    Cloud,
}

pub struct SettingsModalState {
    pub open: bool,
    pub section: SettingsSection,
    pub autostart_enabled: bool,
    pub autostart_message: Option<(bool, String)>,
    pub server_summary: ServerConfigSummary,
    pub local_server_message: Option<(bool, String)>,
    pub pairing: PairingState,
    pub agent_hooks: Vec<AgentIntegrationStatus>,
    pub hooks_loading: bool,
    pub hooks_message: Option<String>,
    pub cloud_status: Option<CloudStatus>,
    pub local_cloud_account: CloudAccountStatus,
    pub cloud_region: RelayRegion,
    pub cloud_busy: bool,
    pub cloud_delete_expanded: bool,
    pub cloud_confirm_delete: bool,
    pub cloud_message: Option<(bool, String)>,
}

impl Default for SettingsModalState {
    fn default() -> Self {
        Self {
            open: false,
            section: SettingsSection::Appearance,
            autostart_enabled: false,
            autostart_message: None,
            server_summary: ServerConfigSummary::default(),
            local_server_message: None,
            pairing: PairingState::default(),
            agent_hooks: Vec::new(),
            hooks_loading: false,
            hooks_message: None,
            cloud_status: None,
            local_cloud_account: CloudAccountStatus::LoggedOut,
            cloud_region: RelayRegion::China,
            cloud_busy: false,
            cloud_delete_expanded: false,
            cloud_confirm_delete: false,
            cloud_message: None,
        }
    }
}

#[inline(never)]
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
        .w(px(200.0))
        .flex_shrink_0()
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
                .label(locale.t("外观与系统", "Appearance & System"))
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
                .label(locale.t("内置服务与扫码", "Server & QR Pairing"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_modal.section = SettingsSection::LocalServer;
                    this.refresh_pairing_addresses(cx);
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
                .label(locale.t("Cloud 账号与中继", "Gateterm Cloud"))
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
        .w(px(840.0))
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
                .w_full()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_hidden()
                .child(nav)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                        .overflow_hidden()
                        .child(
                            div()
                                .id("settings-content-scroll")
                                .size_full()
                                .overflow_y_scrollbar()
                                .child(
                                    div()
                                        .w_full()
                                        .min_w(px(0.0))
                                        .overflow_x_hidden()
                                        .child(content),
                                ),
                        ),
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

#[inline(never)]
fn render_appearance_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let part_theme = render_appearance_theme_and_font(workbench, cx);
    let part_system = render_appearance_system_tray(workbench, cx);
    let part_config = render_appearance_config_summary(workbench, cx);

    v_flex()
        .w_full()
        .p_5()
        .gap_4()
        .child(part_theme)
        .child(div().w_full().h(px(1.0)).bg(cx.theme().border))
        .child(part_system)
        .child(div().w_full().h(px(1.0)).bg(cx.theme().border))
        .child(part_config)
        .into_any_element()
}

#[inline(never)]
fn render_appearance_theme_and_font(
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.update_language_with_window(lang, window, cx);
                })),
        );
    }

    let font_size_str: SharedString = format!("{:.0} px", workbench.config.font_size).into();
    let line_height_str: SharedString = format!("{:.2}x", workbench.config.line_height).into();

    v_flex()
        .w_full()
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
                        .child(locale.t(
                            "终端等宽字体 (Font Family)",
                            "Terminal Monospace Font Family",
                        )),
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

#[inline(never)]
fn render_appearance_system_tray(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let autostart = workbench.settings_modal.autostart_enabled;
    let close_to_tray = workbench.config.close_to_tray;

    v_flex()
        .w_full()
        .gap_3()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(locale.t("系统集成与常驻行为", "System Integration & Tray Behavior")),
        )
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .child(locale.t(
                                    "关闭窗口时最小化到系统托盘",
                                    "Minimize to system tray when closing window",
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(locale.t(
                                    "点击右上角关闭按钮时隐藏主窗口，保持内置 gateterm-server 与终端会话在后台常驻运行。",
                                    "Keep embedded server and local PTY sessions running in the background when clicking Close (X).",
                                )),
                        ),
                )
                .child(
                    Button::new("toggle-close-to-tray")
                        .small()
                        .with_variant(if close_to_tray {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .label(if close_to_tray {
                            locale.t("已开启", "Enabled")
                        } else {
                            locale.t("直接退出", "Quit on Close")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_close_to_tray(cx);
                        })),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("开机自启 (Start at login)", "Start at login")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(locale.t(
                                    "登录系统时自动启动 Gateterm 桌面客户端与内置终端服务。",
                                    "Automatically launch Gateterm when you sign in to your computer.",
                                )),
                        ),
                )
                .child(
                    Button::new("toggle-autostart")
                        .small()
                        .with_variant(if autostart {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .label(if autostart {
                            locale.t("已启用", "Enabled")
                        } else {
                            locale.t("未启用", "Disabled")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_autostart(cx);
                        })),
                ),
        )
        .children(
            workbench
                .settings_modal
                .autostart_message
                .as_ref()
                .map(|(ok, msg)| {
                    div()
                        .text_xs()
                        .text_color(if *ok {
                            cx.theme().success
                        } else {
                            cx.theme().danger
                        })
                        .child(SharedString::from(msg.clone()))
                }),
        )
        .into_any_element()
}

#[inline(never)]
fn render_appearance_config_summary(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let summary = &workbench.settings_modal.server_summary;

    let mut launchers_box = v_flex()
        .w_full()
        .p_2p5()
        .gap_1()
        .rounded(px(6.0))
        .bg(cx.theme().secondary.opacity(0.45))
        .border_1()
        .border_color(cx.theme().border);

    if summary.launchers.is_empty() {
        launchers_box = launchers_box.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(locale.t(
                    "（当前未在 config.toml 中配置自定义启动项，使用系统内置终端与自动探测项）",
                    "(Built-in and auto-discovered launchers only; add [[launcher]] in config.toml)",
                )),
        );
    } else {
        for (name, cmd) in &summary.launchers {
            launchers_box = launchers_box.child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground)
                            .child(SharedString::from(name.clone())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(SharedString::from(cmd.clone())),
                    ),
            );
        }
    }

    v_flex()
        .w_full()
        .gap_2p5()
        .children(summary.config_error.as_ref().map(|err| {
            div()
                .p_2p5()
                .rounded(px(6.0))
                .bg(cx.theme().danger.opacity(0.15))
                .border_1()
                .border_color(cx.theme().danger)
                .text_xs()
                .text_color(cx.theme().danger)
                .child(SharedString::from(format!(
                    "{}: {err}",
                    locale.t(
                        "config.toml 解析失败（当前使用默认配置）",
                        "Invalid config.toml (using defaults)"
                    )
                )))
        }))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(locale.t("会话尺寸策略：", "Session sizing policy:")),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(4.0))
                                .bg(cx.theme().secondary)
                                .text_xs()
                                .text_color(cx.theme().foreground)
                                .child(SharedString::from(summary.window_size.clone())),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(locale.t("(在 config.toml 中修改)", "(edit in config.toml)")),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("open-config-toml")
                                .xsmall()
                                .secondary()
                                .icon(IconName::File)
                                .label(locale.t("打开 config.toml", "Open config.toml"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_server_config_file(cx);
                                })),
                        )
                        .child(
                            Button::new("open-log-file")
                                .xsmall()
                                .secondary()
                                .icon(IconName::ExternalLink)
                                .label(locale.t("查看日志", "View logs"))
                                .on_click(cx.listener(|this, _, _, _| {
                                    this.open_log_file();
                                })),
                        ),
                ),
        )
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(locale.t("配置文件启动项 ([[launcher]])", "Configured Launchers ([[launcher]])")),
        )
        .child(launchers_box)
        .into_any_element()
}

#[inline(never)]
fn render_local_server_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let enabled = workbench.config.start_embedded_server;
    let running_url = workbench.local_supervisor.as_ref().map(|s| s.access_url());
    let listen_addr = workbench.local_supervisor.as_ref().and_then(|s| s.listen_addr());
    let pairing = &workbench.settings_modal.pairing;

    let mut addr_buttons = h_flex().gap_1p5().flex_wrap();
    for addr in &pairing.addresses {
        let ip = addr.ip;
        let is_selected = pairing.selected == Some(ip);
        addr_buttons = addr_buttons.child(
            Button::new(SharedString::from(format!("pair-ip-{ip}")))
                .xsmall()
                .with_variant(if is_selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                })
                .label(addr.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_pairing_ip(ip, cx);
                })),
        );
    }

    let pairing_card = v_flex()
        .w_full()
        .min_w(px(0.0))
        .p_3p5()
        .gap_3()
        .rounded(px(6.0))
        .bg(cx.theme().secondary.opacity(0.45))
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t(
                            "Android 扫码连接 (QR Code Pairing)",
                            "Connect Android with a QR Code",
                        )),
                )
                .child(
                    Button::new("refresh-pairing-addrs")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Redo)
                        .label(locale.t("刷新网卡地址", "Refresh addresses"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_pairing_addresses(cx);
                        })),
                ),
        )
        .child(if listen_addr.is_none() {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(locale.t(
                    "内置服务尚未启动，启用并保存后可生成手机扫码连接二维码。",
                    "Embedded server is not running. Enable and apply settings to generate a QR code.",
                ))
                .into_any_element()
        } else if pairing.addresses.is_empty() {
            let is_loopback = listen_addr.is_some_and(|a| a.ip().is_loopback());
            v_flex()
                .w_full()
                .min_w(px(0.0))
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(if is_loopback {
                            locale.t(
                                "当前仅监听本机 (127.0.0.1)。将监听地址切换为 0.0.0.0 并保存，局域网或 VPN 下的手机即可扫码连接。",
                                "Listening on localhost (127.0.0.1) only. Switch listen address to 0.0.0.0 and save to allow phone connections over LAN/VPN.",
                            )
                        } else {
                            locale.t(
                                "未找到可供手机连接的局域网或 VPN 网卡地址，请检查网络后点击刷新地址。",
                                "No reachable LAN or VPN network addresses found. Check your connection and refresh.",
                            )
                        }),
                )
                .children(is_loopback.then(|| {
                    Button::new("switch-lan-0000")
                        .xsmall()
                        .primary()
                        .label(locale.t(
                            "一键切换为 0.0.0.0 并应用",
                            "Switch to 0.0.0.0 & Apply for LAN Access",
                        ))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.switch_embedded_to_all_interfaces(window, cx);
                        }))
                }))
                .into_any_element()
        } else {
            let conn_url = pairing.selected_connection_url().unwrap_or_default();
            let host_origin = pairing.selected_host_origin().unwrap_or_default();
            let conn_url_copy = conn_url.clone();

            v_flex()
                .w_full()
                .min_w(px(0.0))
                .gap_3()
                .child(
                    v_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(locale.t("选择手机可访问的局域网 / VPN IP：", "Select a LAN or VPN IP reachable by your phone:")),
                        )
                        .child(addr_buttons),
                )
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .items_start()
                        .gap_4()
                        .children(
                            pairing
                                .cached_qr
                                .as_ref()
                                .map(|(_, matrix)| render_qr_code_element(matrix)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(SharedString::from(host_origin)),
                                )
                                .child(
                                    Button::new("copy-pairing-qr-link")
                                        .xsmall()
                                        .primary()
                                        .icon(IconName::Copy)
                                        .label(locale.t("复制连接链接", "Copy connection link"))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                conn_url_copy.clone(),
                                            ));
                                        })),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(locale.t(
                                            "1. 在 Android 客户端选择「添加服务器 → 扫码」，自动填入地址与 Token。\n2. 请确保手机与电脑处于同一局域网或 VPN（如 Tailscale）下。\n3. 修改监听地址、端口或 Token 后，请先点击上方「保存并应用」。",
                                            "1. On Android, tap Add Server -> Scan QR code to fill in address and token.\n2. Ensure your phone and PC share a LAN or VPN (e.g. Tailscale).\n3. After changing address, port, or token, click Save & Apply first.",
                                        )),
                                ),
                        ),
                )
                .into_any_element()
        });

    v_flex()
        .w_full()
        .min_w(px(0.0))
        .p_5()
        .gap_4()
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.0))
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
                                    "无需单独运行命令行服务端，即可直接打开本机 PTY 终端与供 Web / Android 远端接入。",
                                    "Spawn local PTY shells and serve Web / Android clients without an external daemon.",
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
                .w_full()
                .min_w(px(0.0))
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t(
                            "监听地址 (Host:Port，如 127.0.0.1:17683 或 0.0.0.0:17683)",
                            "Listen Address (Host:Port, e.g. 127.0.0.1:17683 or 0.0.0.0:17683)",
                        )),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(Input::new(&workbench.embedded_listen_input).small()),
                        )
                        .child(
                            Button::new("preset-loopback")
                                .xsmall()
                                .secondary()
                                .label("127.0.0.1")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_embedded_host_preset("127.0.0.1", window, cx);
                                })),
                        )
                        .child(
                            Button::new("preset-lan")
                                .xsmall()
                                .secondary()
                                .label("0.0.0.0 (LAN)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_embedded_host_preset("0.0.0.0", window, cx);
                                })),
                        ),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .min_w(px(0.0))
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t("访问 Token (Access Token)", "Access Token")),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(Input::new(&workbench.embedded_token_input).small()),
                        )
                        .child(
                            Button::new("regen-embedded-token")
                                .small()
                                .secondary()
                                .icon(IconName::Redo)
                                .label(locale.t("重新生成", "Regenerate"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.regenerate_embedded_token_input(window, cx);
                                })),
                        )
                        .child(
                            Button::new("save-embedded-listen")
                                .small()
                                .primary()
                                .label(locale.t("保存并应用", "Save & Apply"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_and_restart_embedded_server_with_window(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(locale.t(
                            "框内显示当前生效的 Token；修改监听地址或 Token 后点击「保存并应用」将无损热重载 HTTP 监听，已有本地终端会话不会中断。",
                            "Shows the active token. Clicking Save & Apply hot-rebinds the HTTP listener while preserving all active local PTY sessions.",
                        )),
                ),
        )
        .children(
            workbench
                .settings_modal
                .local_server_message
                .as_ref()
                .map(|(ok, msg)| {
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if *ok {
                            cx.theme().success
                        } else {
                            cx.theme().danger
                        })
                        .child(SharedString::from(msg.clone()))
                }),
        )
        .children(running_url.map(|url| {
            let url_copy = url.clone();
            let url_open = url.clone();
            v_flex()
                .w_full()
                .min_w(px(0.0))
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
                        .w_full()
                        .min_w(px(0.0))
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().foreground)
                                .child(SharedString::from(url)),
                        )
                        .child(
                            Button::new("open-embedded-web")
                                .xsmall()
                                .secondary()
                                .icon(IconName::ExternalLink)
                                .label(locale.t("打开 Web 端", "Open Web"))
                                .on_click(cx.listener(move |_, _, _, _| {
                                    let _ = open::that_detached(url_open.clone());
                                })),
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
        .child(pairing_card)
        .into_any_element()
}

#[inline(never)]
fn render_qr_code_element(matrix: &QrMatrix) -> AnyElement {
    let module_px: f32 = 4.5;
    let total_px = matrix.size as f32 * module_px;

    let mut container = div()
        .relative()
        .w(px(total_px))
        .h(px(total_px))
        .flex_shrink_0()
        .rounded(px(6.0))
        .bg(gpui::white());

    for row in 0..matrix.size {
        let mut col = 0;
        while col < matrix.size {
            if !matrix.is_dark(col, row) {
                col += 1;
                continue;
            }
            let start = col;
            col += 1;
            while col < matrix.size && matrix.is_dark(col, row) {
                col += 1;
            }
            let span = col - start;
            container = container.child(
                div()
                    .absolute()
                    .left(px(start as f32 * module_px))
                    .top(px(row as f32 * module_px))
                    .w(px(span as f32 * module_px))
                    .h(px(module_px))
                    .bg(gpui::black()),
            );
        }
    }

    container.into_any_element()
}

#[inline(never)]
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

#[inline(never)]
fn render_cloud_section(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> AnyElement {
    let locale = workbench.locale;
    let account = &workbench.settings_modal.local_cloud_account;
    let region = workbench.settings_modal.cloud_region;
    let busy = workbench.settings_modal.cloud_busy;
    let delete_expanded = workbench.settings_modal.cloud_delete_expanded;
    let confirm_delete = workbench.settings_modal.cloud_confirm_delete;

    let mut region_buttons = h_flex().gap_2().flex_wrap();
    for r in RelayRegion::ALL {
        let active = r == region;
        region_buttons = region_buttons.child(
            Button::new(SharedString::from(format!("cloud-reg-{:?}", r)))
                .small()
                .with_variant(if active {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                })
                .label(r.label(locale))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_modal.cloud_region = r;
                    cx.notify();
                })),
        );
    }

    let account_card: AnyElement = match account {
        CloudAccountStatus::LoggedIn { email, url } => v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .rounded(px(6.0))
            .bg(cx.theme().secondary.opacity(0.45))
            .border_1()
            .border_color(cx.theme().border)
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
                                    .text_color(cx.theme().success)
                                    .child(SharedString::from(format!(
                                        "● {}: {email}",
                                        locale.t("已登录 Cloud 账号", "Signed in to Cloud")
                                    ))),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SharedString::from(url.clone())),
                            ),
                    )
                    .child(
                        Button::new("cloud-logout-btn")
                            .small()
                            .secondary()
                            .disabled(busy)
                            .label(locale.t("退出登录", "Sign out"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_cloud_action(CloudAction::Logout, window, cx);
                            })),
                    ),
            )
            .child(div().w_full().h(px(1.0)).bg(cx.theme().border))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(locale.t("危险操作：永久注销并删除 Cloud 账号", "Danger Zone: Permanently delete Cloud account")),
                    )
                    .child(
                        Button::new("toggle-cloud-delete")
                            .xsmall()
                            .ghost()
                            .label(if delete_expanded {
                                locale.t("收起", "Hide")
                            } else {
                                locale.t("删除账号…", "Delete account…")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings_modal.cloud_delete_expanded =
                                    !this.settings_modal.cloud_delete_expanded;
                                cx.notify();
                            })),
                    ),
            )
            .children(delete_expanded.then(|| {
                v_flex()
                    .p_3()
                    .gap_2p5()
                    .rounded(px(6.0))
                    .bg(cx.theme().danger.opacity(0.08))
                    .border_1()
                    .border_color(cx.theme().danger.opacity(0.4))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(locale.t(
                                "永久删除 Cloud 账号及其全部数据，需要输入账号密码与发送到邮箱的 6 位确认码。",
                                "Permanently delete your Cloud account and all data. Requires your password and a 6-digit email confirmation code.",
                            )),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(80.0))
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(locale.t("账号密码", "Password")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&workbench.cloud_password_input).small()),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(80.0))
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(locale.t("邮箱确认码", "Email code")),
                            )
                            .child(
                                div()
                                    .w(px(140.0))
                                    .child(Input::new(&workbench.cloud_code_input).small()),
                            )
                            .child(
                                Button::new("cloud-send-del-code")
                                    .small()
                                    .secondary()
                                    .disabled(busy)
                                    .label(locale.t("发送确认码", "Send code"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_cloud_action(
                                            CloudAction::SendDeleteCode,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                Button::new("cloud-confirm-del-chk")
                                    .xsmall()
                                    .with_variant(if confirm_delete {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Secondary
                                    })
                                    .label(if confirm_delete {
                                        locale.t(
                                            "✓ 我明白删除后无法恢复",
                                            "✓ I understand this cannot be undone",
                                        )
                                    } else {
                                        locale.t(
                                            "□ 我明白删除后无法恢复",
                                            "□ I understand this cannot be undone",
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_modal.cloud_confirm_delete =
                                            !this.settings_modal.cloud_confirm_delete;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("cloud-delete-submit")
                                    .small()
                                    .with_variant(ButtonVariant::Danger)
                                    .disabled(busy)
                                    .label(locale.t("永久删除账号", "Permanently delete account"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_cloud_action(
                                            CloudAction::DeleteAccount,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    )
            }))
            .into_any_element(),

        CloudAccountStatus::LoggedOut => v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .rounded(px(6.0))
            .bg(cx.theme().secondary.opacity(0.45))
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(locale.t("中继地区 (Relay Region)", "Relay Region")),
                    )
                    .child(region_buttons)
                    .children((region != RelayRegion::China).then(|| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child(locale.t(
                                "该地区暂未部署，稍后开放。",
                                "This region is not available yet.",
                            ))
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(locale.t(
                        "在浏览器中登录或注册 Gateterm Cloud 账号，完成后将通过本机安全回调自动返回客户端。",
                        "Sign in or create an account in your browser, then return here automatically via loopback PKCE callback.",
                    )),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("cloud-browser-login")
                            .small()
                            .primary()
                            .disabled(busy)
                            .icon(IconName::Globe)
                            .label(locale.t("通过浏览器登录", "Sign in with browser"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_cloud_action(CloudAction::Login, window, cx);
                            })),
                    )
                    .children(busy.then(|| {
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().primary)
                                    .child(locale.t(
                                        "等待浏览器完成登录…",
                                        "Waiting for browser sign-in…",
                                    )),
                            )
                            .child(
                                Button::new("cloud-cancel-login")
                                    .xsmall()
                                    .secondary()
                                    .label(locale.t("取消登录", "Cancel sign-in"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_cloud_action(
                                            CloudAction::CancelLogin,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                    })),
            )
            .into_any_element(),
    };

    v_flex()
        .w_full()
        .p_5()
        .gap_3p5()
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(locale.t(
                            "Gateterm Cloud 账号与移动推送",
                            "Gateterm Cloud Account & Push Notifications",
                        )),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(locale.t(
                            "可选功能：登录后可将本机 AI Coding Agent 的等待审批提醒与执行完成通知实时推送到手机客户端。",
                            "Optional: Sign in to receive mobile push notifications when local AI coding agents need approval or finish a turn.",
                        )),
                ),
        )
        .child(account_card)
        .children(
            workbench
                .settings_modal
                .cloud_message
                .as_ref()
                .map(|(ok, msg)| {
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if *ok {
                            cx.theme().success
                        } else {
                            cx.theme().danger
                        })
                        .child(SharedString::from(msg.clone()))
                }),
        )
        .into_any_element()
}
