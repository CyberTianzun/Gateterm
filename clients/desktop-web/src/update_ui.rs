//! Sparkle-style self-update window UI state and egui rendering.
//!
//! Pure UI rendering: never performs filesystem or network IO directly; returns
//! [`UpdateOutcome`] for `app.rs` to execute outside `EguiWindow::redraw_ui`.

use gateterm_updater::{AvailableUpdate, DownloadProgress};

use crate::i18n::Locale;
use crate::tr;

const OK_COLOR: egui::Color32 = egui::Color32::from_rgb(60, 140, 60);
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(190, 125, 25);
const WARN_BG_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(28, 18, 2, 26);
const ERR_COLOR: egui::Color32 = egui::Color32::from_rgb(180, 60, 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePhase {
    Checking,
    Available,
    Downloading(DownloadProgress),
    Preparing,
    UpToDate,
    Error {
        message: String,
        fallback_url: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateView {
    pub current_version: String,
    pub update: Option<AvailableUpdate>,
    pub phase: UpdatePhase,
    pub active_sessions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    None,
    UpdateAndRestart,
    OpenDownloadPage(String),
    SkipVersion(String),
    Cancel,
    RetryCheck,
}

pub fn terminal_warning_text(active_sessions: usize, locale: Locale) -> String {
    match active_sessions {
        0 => locale
            .text(
                "更新将重启 Gateterm 并关闭所有本地终端会话。",
                "Updating will restart Gateterm and close any local terminal sessions.",
            )
            .to_owned(),
        1 => locale
            .text(
                "注意：更新将重启 Gateterm，并关闭当前正在运行的 1 个本地终端会话（含其中运行的程序 / Agent）。",
                "Warning: Updating will restart Gateterm and close 1 running local terminal session (and programs/agents inside it).",
            )
            .to_owned(),
        n => tr!(
            locale,
            "注意：更新将重启 Gateterm，并关闭当前正在运行的 {n} 个本地终端会话（含其中运行的程序 / Agent）。",
            "Warning: Updating will restart Gateterm and close {n} running local terminal sessions (and programs/agents inside them)."
        ),
    }
}

fn format_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

pub fn draw(ui: &mut egui::Ui, view: &mut UpdateView, locale: Locale) -> UpdateOutcome {
    let mut outcome = UpdateOutcome::None;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

    egui::Panel::top("update-header").show(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.add_space(6.0);
        match (&view.phase, &view.update) {
            (UpdatePhase::Checking, _) => {
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    ui.strong(locale.text("正在检查更新…", "Checking for updates…"));
                });
                ui.small(tr!(
                    locale,
                    "当前版本：v{}",
                    "Current version: v{}",
                    view.current_version
                ));
            }
            (UpdatePhase::UpToDate, _) => {
                ui.colored_label(
                    OK_COLOR,
                    egui::RichText::new(locale.text("当前已是最新版本", "You're up to date"))
                        .strong(),
                );
                ui.small(tr!(
                    locale,
                    "Gateterm v{} 已是最新版本。",
                    "Gateterm v{} is the latest version.",
                    view.current_version
                ));
            }
            (UpdatePhase::Error { .. }, None) => {
                ui.colored_label(
                    ERR_COLOR,
                    egui::RichText::new(locale.text("检查更新失败", "Update check failed"))
                        .strong(),
                );
                ui.small(tr!(
                    locale,
                    "当前版本：v{}",
                    "Current version: v{}",
                    view.current_version
                ));
            }
            (_, Some(update)) => {
                ui.strong(locale.text("发现新版本", "A new version of Gateterm is available!"));
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "v{} → v{}",
                            view.current_version, update.version
                        ))
                        .strong(),
                    );
                    if !update.pub_date.is_empty() {
                        ui.small(update.pub_date.as_str());
                    }
                });
            }
            (_, None) => {
                ui.strong("Gateterm");
                ui.small(format!("v{}", view.current_version));
            }
        }
        ui.add_space(4.0);
    });

    egui::Panel::bottom("update-footer").show(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.add_space(4.0);

        if let Some(update) = &view.update
            && update.can_self_apply
            && !matches!(view.phase, UpdatePhase::Checking | UpdatePhase::UpToDate)
        {
            egui::Frame::NONE
                .fill(WARN_BG_COLOR)
                .stroke(egui::Stroke::new(1.0, WARN_COLOR))
                .corner_radius(4.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.colored_label(
                        WARN_COLOR,
                        egui::RichText::new(terminal_warning_text(view.active_sessions, locale))
                            .small(),
                    );
                });
            ui.add_space(6.0);
        }

        match &view.phase {
            UpdatePhase::Downloading(progress) => {
                let fraction = if progress.total > 0 {
                    (progress.downloaded as f32 / progress.total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let phase_label = if progress.total > 0 && progress.downloaded >= progress.total {
                    locale.text("正在校验并解压更新…", "Verifying and extracting update…")
                } else {
                    locale.text("正在下载更新…", "Downloading update…")
                };
                ui.horizontal_wrapped(|ui| {
                    ui.small(phase_label);
                    ui.small("·");
                    ui.small(format!(
                        "{} / {}",
                        format_mb(progress.downloaded),
                        format_mb(progress.total)
                    ));
                });
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                ui.add_space(6.0);
            }
            UpdatePhase::Preparing => {
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    ui.small(locale.text("正在准备重启…", "Preparing to restart…"));
                });
                ui.add(egui::ProgressBar::new(1.0).show_percentage());
                ui.add_space(6.0);
            }
            UpdatePhase::Error {
                message,
                fallback_url,
            } => {
                ui.colored_label(ERR_COLOR, egui::RichText::new(message.as_str()).small());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button(locale.text("重试", "Retry")).clicked() {
                        outcome = UpdateOutcome::RetryCheck;
                    }
                    if let Some(url) = fallback_url
                        && ui
                            .button(locale.text("前往 GitHub 手动下载", "Open GitHub Release"))
                            .clicked()
                    {
                        outcome = UpdateOutcome::OpenDownloadPage(url.clone());
                    }
                });
                ui.add_space(6.0);
            }
            UpdatePhase::Checking | UpdatePhase::Available | UpdatePhase::UpToDate => {}
        }

        ui.separator();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let can_skip = view.update.is_some()
                && matches!(view.phase, UpdatePhase::Available | UpdatePhase::Error { .. });
            if can_skip
                && let Some(update) = &view.update
                && ui
                    .button(locale.text("跳过此版本", "Skip This Version"))
                    .clicked()
            {
                outcome = UpdateOutcome::SkipVersion(update.version.clone());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match &view.phase {
                    UpdatePhase::Available => {
                        if let Some(update) = &view.update {
                            let primary_label = if update.can_self_apply {
                                locale.text("更新并重启", "Update & Restart")
                            } else {
                                locale.text("前往下载页面", "Open Download Page")
                            };
                            if ui.button(primary_label).clicked() {
                                outcome = if update.can_self_apply {
                                    UpdateOutcome::UpdateAndRestart
                                } else {
                                    UpdateOutcome::OpenDownloadPage(update.release_url.clone())
                                };
                            }
                        }
                        if ui.button(locale.text("取消", "Cancel")).clicked() {
                            outcome = UpdateOutcome::Cancel;
                        }
                    }
                    UpdatePhase::Downloading(_) => {
                        if let Some(update) = &view.update {
                            let primary_label = if update.can_self_apply {
                                locale.text("更新并重启", "Update & Restart")
                            } else {
                                locale.text("前往下载页面", "Open Download Page")
                            };
                            ui.add_enabled(false, egui::Button::new(primary_label));
                        }
                        if ui.button(locale.text("取消", "Cancel")).clicked() {
                            outcome = UpdateOutcome::Cancel;
                        }
                    }
                    UpdatePhase::Preparing => {
                        ui.add_enabled(
                            false,
                            egui::Button::new(locale.text("更新并重启", "Update & Restart")),
                        );
                        ui.add_enabled(false, egui::Button::new(locale.text("取消", "Cancel")));
                    }
                    UpdatePhase::Checking | UpdatePhase::Error { .. } => {
                        if ui.button(locale.text("取消", "Cancel")).clicked() {
                            outcome = UpdateOutcome::Cancel;
                        }
                    }
                    UpdatePhase::UpToDate => {
                        if ui.button(locale.text("关闭", "Close")).clicked() {
                            outcome = UpdateOutcome::Cancel;
                        }
                    }
                }
            });
        });
        ui.add_space(4.0);
    });

    egui::CentralPanel::default().show(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.add_space(4.0);

        if let Some(update) = &view.update {
            ui.small(locale.text("更新内容", "Release Notes"));
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("update-release-notes")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        let notes = if update.notes.trim().is_empty() {
                            locale.text("（此版本暂无更新说明）", "(No release notes provided.)")
                        } else {
                            update.notes.as_str()
                        };
                        ui.label(notes);
                    });
            });
        } else {
            match &view.phase {
                UpdatePhase::Checking => {
                    ui.small(locale.text(
                        "正在获取并验证签名更新清单…",
                        "Fetching and verifying the signed release manifest…",
                    ));
                }
                UpdatePhase::UpToDate => {
                    ui.label(tr!(
                        locale,
                        "当前运行的 Gateterm v{} 已是最新稳定版本。",
                        "You are running the latest stable version of Gateterm (v{}).",
                        view.current_version
                    ));
                }
                UpdatePhase::Error { .. } => {
                    ui.small(locale.text(
                        "无法获取更新信息。请检查网络连接后重试。",
                        "Could not retrieve update information. Check your network connection and retry.",
                    ));
                }
                UpdatePhase::Available
                | UpdatePhase::Downloading(_)
                | UpdatePhase::Preparing => {}
            }
        }
    });

    outcome
}
