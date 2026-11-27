//! 设置窗口的绘制。只画界面、只返回用户意图,不碰文件、不碰 supervisor ——
//! 副作用全部由 app.rs 执行(保存流程本身在 [`crate::settings::save`] 里),
//! UI 层不执行 IO;布局和语言切换可用无窗口的 egui 渲染测试验证。
//!
//! **[`draw`] 同一帧里可能被调用多次**:调用方 `EguiWindow::redraw_ui` 收的是
//! `FnMut`,而界面里用到的 `egui::Grid` 每次开窗第一帧必然 `request_discard`、
//! 让整帧重跑一趟。所以这里只允许改 [`View`] 里的纯界面状态和返回 [`Outcome`],
//! 一次性的动作(保存、写文件、重启 HTTP 层、Agent 扫描/安装/卸载/清理)必须由
//! 调用方在闭包外做一次。

use crate::i18n::{LanguagePref, Locale};
use crate::settings::agents::{draw_agents, AgentUiAction, AgentsView};
use crate::settings::cloud::{CloudAction, CloudForm, CloudStatus, RelayRegion};
use crate::settings::state::Form;
use crate::tr;

pub use crate::settings::state::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Agents,
    Cloud,
}

/// 用户在这一帧里表达的意图。同一帧最多认一个:后点的覆盖先点的,
/// 而实际上一帧内不可能同时点中两个按钮。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Save,
    Cancel,
    OpenConfigFile,
    RegenerateToken,
    RefreshWebAddrs,
    CheckForUpdates,
    Agents(AgentUiAction),
    Cloud(CloudAction),
}

/// 设置窗口的全部可见状态。窗口开着的时候由 app.rs 持有,关窗时一起丢掉。
pub struct View {
    pub tab: SettingsTab,
    pub form: Form,
    pub web_addr: crate::settings::web_addr::WebAddrPicker,
    /// 只读展示,v1 不提供修改。
    pub window_size: String,
    /// (名称, 命令) —— 只读列表。
    pub launchers: Vec<(String, String)>,
    /// 启动时的致命状况(配置解析失败 / 没监听上),常驻在所有选项卡最顶上,
    /// 直到状况被修好为止。和 `message` 不是一回事:后者只描述最近一次保存的结果。
    pub banner: Option<String>,
    pub message: Option<Message>,
    pub agents: AgentsView,
    pub cloud: CloudForm,
    pub cloud_status: CloudStatus,
    /// 有 Cloud 请求在路上:按钮置灰,防止连点。
    pub cloud_busy: bool,
    /// 最近一次 Cloud 操作的结果。和 `message`(保存结果)分开,两者互不覆盖。
    pub cloud_message: Option<Message>,
    pub update_status_line: String,
    pub update_checking: bool,
}

const OK_COLOR: egui::Color32 = egui::Color32::from_rgb(60, 140, 60);
const ERR_COLOR: egui::Color32 = egui::Color32::from_rgb(180, 60, 60);

pub fn draw(ui: &mut egui::Ui, view: &mut View, locale: Locale) -> Outcome {
    let mut outcome = Outcome::None;

    egui::Panel::top("settings-tabs").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut view.tab,
                SettingsTab::General,
                locale.text("常规", "General"),
            );
            let agents_label = if view.agents.needs_attention() {
                locale.text("AI 集成 •", "Agents •")
            } else {
                locale.text("AI 集成", "Agents")
            };
            ui.selectable_value(&mut view.tab, SettingsTab::Agents, agents_label);
            ui.selectable_value(
                &mut view.tab,
                SettingsTab::Cloud,
                locale.text("Cloud 账号", "Cloud"),
            );
        });
        ui.add_space(2.0);
    });

    egui::CentralPanel::default().show(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.add_space(4.0);

        if let Some(banner) = &view.banner {
            ui.colored_label(ERR_COLOR, banner.as_str());
            ui.add_space(6.0);
        }

        let scroll_id = match view.tab {
            SettingsTab::General => "scroll-general",
            SettingsTab::Agents => "scroll-agents",
            SettingsTab::Cloud => "scroll-cloud",
        };
        egui::ScrollArea::vertical().id_salt(scroll_id).show(ui, |ui| {
            match view.tab {
                SettingsTab::General => {
                    outcome = draw_general(ui, view, locale);
                }
                SettingsTab::Agents => {
                    if let Some(a) = draw_agents(ui, &mut view.agents, locale) {
                        outcome = Outcome::Agents(a);
                    }
                }
                SettingsTab::Cloud => {
                    if let Some(a) = draw_cloud(ui, view, locale) {
                        outcome = Outcome::Cloud(a);
                    }
                }
            }
        });
    });

    outcome
}

fn draw_general(ui: &mut egui::Ui, view: &mut View, locale: Locale) -> Outcome {
    let mut outcome = Outcome::None;
    // 表单一被改动就把上一次的保存结果抹掉:保存成功后接着改端口,绿色的
    // 「已生效 · …」还挂在那里,说的已经不是当前表单里这套值了。
    // 纯赋值,不违反「同一帧可能跑两趟」的约束。
    let mut edited = false;

    egui::Grid::new("settings")
        .num_columns(2)
        .max_col_width((ui.available_width() - 150.0).max(240.0))
        .spacing([12.0, 10.0])
        .show(ui, |ui| {
            ui.label(locale.text("界面语言", "Language"));
            egui::ComboBox::from_id_salt("desktop-language")
                .selected_text(view.form.language.label(locale))
                .show_ui(ui, |ui| {
                    for language in LanguagePref::ALL {
                        edited |= ui
                            .selectable_value(&mut view.form.language, language, language.label(locale))
                            .changed();
                    }
                });
            ui.end_row();

            ui.label(locale.text("监听地址", "Listen address"));
            ui.horizontal(|ui| {
                edited |= ui
                    .add(egui::TextEdit::singleline(&mut view.form.host).desired_width(140.0))
                    .changed();
                ui.label(":");
                edited |= ui
                    .add(egui::TextEdit::singleline(&mut view.form.port).desired_width(60.0))
                    .changed();
            });
            ui.end_row();

            let effective_listen_ip = view.form.host.trim().parse::<std::net::IpAddr>().ok();
            ui.label(locale.text("打开 Web 客户端", "Open web client"));
            let (web_edited, refresh_web_addrs) = view.web_addr.draw_selector(
                ui,
                effective_listen_ip,
                &mut view.form.web_host,
                locale,
            );
            edited |= web_edited;
            if refresh_web_addrs {
                outcome = Outcome::RefreshWebAddrs;
            }
            ui.end_row();

            ui.label(locale.text("访问 token", "Access token"));
            ui.horizontal(|ui| {
                edited |= ui
                    .add(egui::TextEdit::singleline(&mut view.form.token).desired_width(180.0))
                    .changed();
                if ui.button(locale.text("重新生成", "Regenerate")).clicked() {
                    outcome = Outcome::RegenerateToken;
                }
            });
            ui.end_row();

            ui.label("");
            ui.small(locale.text(
                "框里显示的是当前生效的 token;直接保存会把它固定进 config.toml。\n\
                 留空表示不固定:不写进 config.toml,每次启动随机生成 —— 本次运行\n\
                 沿用当前 token;若服务未在监听,保存时当场生成一个新的。",
                "This is the active token. Save to keep it in config.toml.\nLeave it blank to generate a new token at each startup. The current token stays active until restart; if the server is stopped, saving generates one now.",
            ));
            ui.end_row();

            ui.label(locale.text("网页标题", "Web title"));
            edited |= ui
                .add(
                    egui::TextEdit::singleline(&mut view.form.web_title)
                        .desired_width(220.0)
                        .hint_text(locale.text("留空使用默认 gateterm", "Leave blank for gateterm")),
                )
                .changed();
            ui.end_row();

            ui.label("");
            ui.small(locale.text(
                "设置后浏览器标签页显示为「<标题> - gateterm」，刷新网页生效。",
                "Shown in the browser tab as \"<title> - gateterm\" after reloading.",
            ));
            ui.end_row();

            ui.label(locale.text("会话尺寸策略", "Session sizing"));
            ui.horizontal(|ui| {
                ui.label(view.window_size.as_str());
                ui.small(locale.text("(在 config.toml 中修改)", "(edit in config.toml)"));
            });
            ui.end_row();

            ui.label(locale.text("开机自启", "Start at login"));
            edited |= ui.checkbox(&mut view.form.autostart, "").changed();
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(locale.text("启动项", "Launchers"));
        if ui
            .button(locale.text("打开 config.toml", "Open config.toml"))
            .clicked()
        {
            outcome = Outcome::OpenConfigFile;
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("launchers-list")
        .max_height(110.0)
        .show(ui, |ui| {
            if view.launchers.is_empty() {
                ui.small(locale.text("(只有内置项)", "(built-in launchers only)"));
            }
            for (name, command) in &view.launchers {
                ui.horizontal(|ui| {
                    ui.label(name.as_str());
                    ui.small(command.as_str());
                });
            }
        });

    ui.add_space(8.0);
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(locale.text("版本与更新", "Version & Updates"));
        ui.small(concat!("v", env!("CARGO_PKG_VERSION")));
        if view.update_checking {
            ui.spinner();
        }
        if ui
            .add_enabled(
                !view.update_checking,
                egui::Button::new(locale.text("检查更新…", "Check for Updates…")),
            )
            .clicked()
        {
            outcome = Outcome::CheckForUpdates;
        }
    });
    if !view.update_status_line.is_empty() {
        ui.small(view.update_status_line.as_str());
    }

    // 「重新生成」也算改动:token 已经不是保存时那个了。
    if edited || outcome == Outcome::RegenerateToken {
        view.message = None;
    }

    ui.add_space(8.0);
    if let Some(msg) = &view.message {
        match msg {
            Message::Ok(t) => ui.colored_label(OK_COLOR, t.as_str()),
            Message::Err(t) => ui.colored_label(ERR_COLOR, t.as_str()),
        };
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(locale.text("取消", "Cancel")).clicked() {
            outcome = Outcome::Cancel;
        }
        if ui.button(locale.text("保存并应用", "Save and apply")).clicked() {
            outcome = Outcome::Save;
        }
    });

    outcome
}

/// 「Cloud 账号」区。和 `draw` 一样同一帧可能跑两趟:只改表单文本、只返回意图。
fn draw_cloud(ui: &mut egui::Ui, view: &mut View, locale: Locale) -> Option<CloudAction> {
    let mut action = None;
    let status = view.cloud_status.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label(locale.text("Cloud 账号", "Cloud account"));
        ui.small(locale.text(
            "(可选,用于把 agent 的等待提醒推送到手机)",
            "(optional, for agent notifications on your phone)",
        ));
        if view.cloud_busy {
            ui.spinner();
        }
    });
    ui.add_enabled_ui(!view.cloud_busy, |ui| match &status {
        CloudStatus::LoggedIn { email, url } => {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!(locale, "已登录:{email}", "Signed in: {email}"));
                ui.small(url.as_str());
            });
            if ui.button(locale.text("退出登录", "Sign out")).clicked() {
                action = Some(CloudAction::Logout);
            }
            ui.collapsing(locale.text("删除账号", "Delete account"), |ui| {
                ui.small(locale.text(
                    "永久删除 Cloud 账号及其全部数据,需要密码和发到邮箱的确认码。",
                    "Permanently delete your Cloud account and all its data. Requires your password and an email confirmation code.",
                ));
                egui::Grid::new("cloud-delete")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(locale.text("密码", "Password"));
                        ui.add(
                            egui::TextEdit::singleline(&mut view.cloud.password)
                                .password(true)
                                .desired_width(180.0),
                        );
                        ui.end_row();
                        ui.label(locale.text("确认码", "Confirmation code"));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut view.cloud.code)
                                    .desired_width(80.0),
                            );
                            if ui.button(locale.text("发送确认码", "Send code")).clicked() {
                                action = Some(CloudAction::SendDeleteCode);
                            }
                        });
                        ui.end_row();
                    });
                ui.checkbox(
                    &mut view.cloud.confirm_delete,
                    locale.text("我明白删除后无法恢复", "I understand this cannot be undone"),
                );
                if ui
                    .button(locale.text("永久删除账号", "Permanently delete account"))
                    .clicked()
                {
                    action = Some(CloudAction::DeleteAccount);
                }
            });
        }
        CloudStatus::LoggedOut => {
            ui.horizontal(|ui| {
                ui.label(locale.text("中继地区", "Relay region"));
                egui::ComboBox::from_id_salt("cloud-region")
                    .selected_text(view.cloud.region.label(locale))
                    .show_ui(ui, |ui| {
                        for region in RelayRegion::ALL {
                            ui.selectable_value(
                                &mut view.cloud.region,
                                region,
                                region.label(locale),
                            );
                        }
                    });
            });
            if view.cloud.region != RelayRegion::China {
                ui.small(locale.text("该地区暂未部署，稍后开放。", "This region is not available yet."));
            }
            ui.small(locale.text(
                "在浏览器中登录或创建账号，完成后自动返回客户端。",
                "Sign in or create an account in your browser, then return here automatically.",
            ));
            if ui
                .button(locale.text("通过浏览器登录", "Sign in with browser"))
                .clicked()
            {
                action = Some(CloudAction::Login);
            }
        }
    });
    if view.cloud_busy && matches!(status, CloudStatus::LoggedOut) {
        ui.horizontal(|ui| {
            ui.label(locale.text("等待浏览器完成登录…", "Waiting for browser sign-in…"));
            if ui.button(locale.text("取消登录", "Cancel sign-in")).clicked() {
                action = Some(CloudAction::CancelLogin);
            }
        });
    }
    if let Some(m) = &view.cloud_message {
        match m {
            Message::Ok(t) => ui.colored_label(OK_COLOR, t.as_str()),
            Message::Err(t) => ui.colored_label(ERR_COLOR, t.as_str()),
        };
    }
    action
}
