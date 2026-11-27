//! Sparkle-style self-update modal state, pure presentation helpers, and GPUI overlay.

use std::time::{SystemTime, UNIX_EPOCH};

use gateterm_updater::{AvailableUpdate, CheckOutcome, DownloadError, DownloadProgress};
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    progress::Progress,
    scroll::ScrollableElement as _,
    spinner::Spinner,
    v_flex,
};

use super::app::GatetermWorkbench;
use crate::i18n::Locale;

pub(crate) const DEFAULT_RELEASES_URL: &str =
    "https://github.com/CyberTianzun/Gateterm/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateModalPhase {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading {
        downloaded: u64,
        total: u64,
    },
    Preparing,
    UpToDate,
    Error {
        message: String,
        fallback_url: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct UpdateModalState {
    pub open: bool,
    pub phase: UpdateModalPhase,
    pub update: Option<AvailableUpdate>,
    pub pending_on_show: bool,
    pub last_checked_at: Option<u64>,
    pub check_generation: u64,
    pub download_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPresentAction {
    ShowNow,
    DeferUntilShown,
    Suppress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedAction<T> {
    Ignored,
    ReadyToApply(T),
    Failed,
}

impl UpdateModalState {
    /// Returns `true` while an update download or final apply preparation is in progress.
    pub fn is_downloading_or_preparing(&self) -> bool {
        matches!(
            self.phase,
            UpdateModalPhase::Downloading { .. } | UpdateModalPhase::Preparing
        )
    }

    /// Transitions the modal into `Checking` for a manual user-triggered check,
    /// returning the new `check_generation` or `None` if a download/handoff is already running.
    pub fn begin_manual_check(&mut self) -> Option<u64> {
        if self.is_downloading_or_preparing() {
            self.open = true;
            return None;
        }
        self.check_generation = self.check_generation.wrapping_add(1);
        self.open = true;
        self.pending_on_show = false;
        self.update = None;
        self.phase = UpdateModalPhase::Checking;
        Some(self.check_generation)
    }

    /// Applies an update check outcome (manual or background) respecting stale generations,
    /// skip preferences, and hidden-to-tray presentation rules.
    /// Returns `true` when the outcome was applied and the update modal is now open.
    pub fn apply_check_result(
        &mut self,
        event_generation: u64,
        manual: bool,
        is_startup: bool,
        window_hidden_to_tray: bool,
        locale: Locale,
        result: Result<CheckOutcome, String>,
    ) -> bool {
        if manual {
            if event_generation != self.check_generation
                || !self.open
                || self.is_downloading_or_preparing()
            {
                return false;
            }
            match result {
                Ok(CheckOutcome::UpdateAvailable { update, .. }) => {
                    self.update = Some(update);
                    self.last_checked_at = Some(now_unix_secs());
                    self.phase = UpdateModalPhase::Available;
                }
                Ok(CheckOutcome::UpToDate { checked_at, .. }) => {
                    self.update = None;
                    self.last_checked_at = Some(checked_at);
                    self.phase = UpdateModalPhase::UpToDate;
                }
                Err(err_msg) => {
                    self.update = None;
                    let message = match locale {
                        Locale::ZhCn => format!("检查更新失败：{err_msg}"),
                        Locale::En => format!("Update check failed: {err_msg}"),
                    };
                    self.phase = UpdateModalPhase::Error {
                        message,
                        fallback_url: Some(DEFAULT_RELEASES_URL.to_owned()),
                    };
                }
            }
            return true;
        }

        if self.is_downloading_or_preparing() || self.phase == UpdateModalPhase::Checking {
            return false;
        }

        match result {
            Ok(CheckOutcome::UpdateAvailable { update, skipped }) => {
                self.update = Some(update);
                self.last_checked_at = Some(now_unix_secs());
                if is_startup && self.check_generation == 0 && !self.open {
                    self.phase = UpdateModalPhase::Available;
                    match should_present_auto_update(window_hidden_to_tray, skipped) {
                        AutoPresentAction::ShowNow => {
                            self.open = true;
                        }
                        AutoPresentAction::DeferUntilShown => {
                            self.pending_on_show = true;
                        }
                        AutoPresentAction::Suppress => {}
                    }
                } else if !self.open {
                    self.phase = UpdateModalPhase::Available;
                }
            }
            Ok(CheckOutcome::UpToDate { checked_at, .. }) => {
                self.update = None;
                self.last_checked_at = Some(checked_at);
                self.pending_on_show = false;
                if !self.open {
                    self.phase = UpdateModalPhase::UpToDate;
                }
            }
            Err(_) => {}
        }
        self.open
    }

    /// Starts a self-apply download if the active candidate supports it and no download is active.
    pub fn begin_download(&mut self, total: u64) -> Option<u64> {
        if self.is_downloading_or_preparing()
            || !self.update.as_ref().is_some_and(|u| u.can_self_apply)
        {
            return None;
        }
        self.download_generation = self.download_generation.wrapping_add(1);
        self.phase = UpdateModalPhase::Downloading {
            downloaded: 0,
            total,
        };
        Some(self.download_generation)
    }

    /// Updates download progress only if `event_generation` matches the current download.
    pub fn apply_download_progress(
        &mut self,
        event_generation: u64,
        progress: DownloadProgress,
    ) -> bool {
        if event_generation != self.download_generation || !self.open {
            return false;
        }
        if !matches!(self.phase, UpdateModalPhase::Downloading { .. }) {
            return false;
        }
        self.phase = UpdateModalPhase::Downloading {
            downloaded: progress.downloaded,
            total: progress.total,
        };
        true
    }

    /// Transitions `Downloading` into `Preparing` or `Error` when `event_generation` matches.
    /// Returns [`StagedAction::Ignored`] after cancellation or stale generations.
    pub fn apply_staged_result<T>(
        &mut self,
        event_generation: u64,
        result: Result<T, String>,
    ) -> StagedAction<T> {
        if event_generation != self.download_generation
            || !self.open
            || !matches!(self.phase, UpdateModalPhase::Downloading { .. })
        {
            return StagedAction::Ignored;
        }
        match result {
            Ok(staged) => {
                self.phase = UpdateModalPhase::Preparing;
                StagedAction::ReadyToApply(staged)
            }
            Err(message) => {
                let fallback_url = self
                    .update
                    .as_ref()
                    .map(|u| u.release_url.clone())
                    .or_else(|| Some(DEFAULT_RELEASES_URL.to_owned()));
                self.phase = UpdateModalPhase::Error {
                    message,
                    fallback_url,
                };
                StagedAction::Failed
            }
        }
    }

    /// Sets an error phase directly (e.g. post-shutdown handoff failure or rollback notice).
    pub fn set_error(&mut self, message: String, fallback_url: Option<String>) {
        self.phase = UpdateModalPhase::Error {
            message,
            fallback_url,
        };
    }

    /// Cancels any active check or download by bumping both generation counters and closing the modal.
    pub fn cancel(&mut self) {
        self.check_generation = self.check_generation.wrapping_add(1);
        self.download_generation = self.download_generation.wrapping_add(1);
        self.open = false;
        self.pending_on_show = false;
        self.phase = if self.update.is_some() {
            UpdateModalPhase::Available
        } else if self.last_checked_at.is_some() {
            UpdateModalPhase::UpToDate
        } else {
            UpdateModalPhase::Idle
        };
    }

    /// Presents a deferred startup update or rollback notice when the tray window is restored.
    pub fn on_show_window(&mut self) -> bool {
        if self.pending_on_show {
            self.pending_on_show = false;
            if self.update.is_some() || matches!(self.phase, UpdateModalPhase::Error { .. }) {
                self.open = true;
                return true;
            }
        }
        false
    }

    /// Concise status summary displayed in the Settings -> Version & Updates card.
    pub fn status_line(&self, current_version: &str, locale: Locale) -> String {
        match &self.phase {
            UpdateModalPhase::Checking => locale
                .t("正在检查更新…", "Checking for updates…")
                .to_owned(),
            UpdateModalPhase::Downloading { .. } => locale
                .t("正在下载更新…", "Downloading update…")
                .to_owned(),
            UpdateModalPhase::Preparing => locale
                .t("正在准备重启…", "Preparing to restart…")
                .to_owned(),
            UpdateModalPhase::Error { message, .. } if self.update.is_none() => message.clone(),
            _ => {
                if let Some(update) = &self.update {
                    match locale {
                        Locale::ZhCn => format!("发现新版本 v{}", update.version),
                        Locale::En => format!("Update available: v{}", update.version),
                    }
                } else if matches!(self.phase, UpdateModalPhase::UpToDate)
                    || self.last_checked_at.is_some()
                {
                    match locale {
                        Locale::ZhCn => format!("已是最新版本（v{current_version}）"),
                        Locale::En => format!("Up to date (v{current_version})"),
                    }
                } else {
                    locale.t("尚未检查更新", "Not checked yet").to_owned()
                }
            }
        }
    }
}

/// Returns `(main_warning, remote_sessions_subnote)` describing how restarting affects
/// active local terminal sessions versus remote server sessions.
pub fn terminal_warning_copy(active_local_sessions: usize, locale: Locale) -> (String, String) {
    let main = match active_local_sessions {
        0 => locale
            .t(
                "更新将重启 Gateterm 并关闭所有本地终端会话。",
                "Updating will restart Gateterm and close any local terminal sessions.",
            )
            .to_owned(),
        1 => locale
            .t(
                "注意：更新将重启 Gateterm，并关闭当前正在运行的 1 个本地终端会话（含其中运行的程序 / Agent）。",
                "Warning: Updating will restart Gateterm and close 1 running local terminal session (and programs/agents inside it).",
            )
            .to_owned(),
        n => match locale {
            Locale::ZhCn => format!(
                "注意：更新将重启 Gateterm，并关闭当前正在运行的 {n} 个本地终端会话（含其中运行的程序 / Agent）。"
            ),
            Locale::En => format!(
                "Warning: Updating will restart Gateterm and close {n} running local terminal sessions (and programs/agents inside them)."
            ),
        },
    };
    let remote = locale
        .t(
            "注：远程服务器上的会话不受本地客户端重启影响。",
            "Note: Sessions running on remote servers are unaffected by restarting this local client.",
        )
        .to_owned();
    (main, remote)
}

/// Returns the localized primary action button label for self-apply vs macOS browser fallback.
pub fn primary_action_label(can_self_apply: bool, locale: Locale) -> &'static str {
    if can_self_apply {
        locale.t("更新并重启", "Update & Restart")
    } else {
        locale.t("前往下载页面", "Open Download Page")
    }
}

/// Decides whether an automatic startup update notice should show now, defer until the
/// tray window is restored, or stay suppressed because the version was skipped.
pub fn should_present_auto_update(
    window_hidden_to_tray: bool,
    skipped: bool,
) -> AutoPresentAction {
    if skipped {
        AutoPresentAction::Suppress
    } else if window_hidden_to_tray {
        AutoPresentAction::DeferUntilShown
    } else {
        AutoPresentAction::ShowNow
    }
}

/// Formats [`DownloadError`] variants for localized display inside the update modal.
pub fn format_download_error(locale: Locale, err: &DownloadError) -> String {
    match err {
        DownloadError::NotWritable { path, source } => match locale {
            Locale::ZhCn => format!(
                "当前安装目录不可写（{}）：{source}\n\n请点击下方按钮前往 GitHub 手动下载安装。",
                path.display()
            ),
            Locale::En => format!(
                "The installation directory is not writable ({}): {source}\n\nPlease download the update manually from GitHub.",
                path.display()
            ),
        },
        DownloadError::Cancelled => locale.t("已取消下载。", "Download cancelled.").to_owned(),
        DownloadError::SizeMismatch { expected, actual } => match locale {
            Locale::ZhCn => {
                format!("安装包大小校验失败（预期 {expected} 字节，实际 {actual} 字节）。")
            }
            Locale::En => format!(
                "Downloaded archive size mismatch (expected {expected} bytes, got {actual} bytes)."
            ),
        },
        DownloadError::HashMismatch { .. } => locale
            .t(
                "安装包 SHA-256 校验失败，文件可能已损坏或被篡改。",
                "Downloaded archive SHA-256 verification failed. The file may be corrupted.",
            )
            .to_owned(),
        DownloadError::UnsafeArchivePath(entry) => match locale {
            Locale::ZhCn => format!("更新包包含不安全路径：{entry}"),
            Locale::En => format!("Update archive contains an unsafe entry path: {entry}"),
        },
        DownloadError::BinaryNotFound(binary) => match locale {
            Locale::ZhCn => format!("更新包中未找到可执行文件：{binary}"),
            Locale::En => format!("Expected executable not found in update archive: {binary}"),
        },
        DownloadError::Other(inner) => match locale {
            Locale::ZhCn => format!("下载或解压更新失败：{inner:#}"),
            Locale::En => format!("Failed to download or extract update: {inner:#}"),
        },
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn format_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

#[inline(never)]
pub fn render_update_modal(
    workbench: &GatetermWorkbench,
    cx: &mut Context<GatetermWorkbench>,
) -> Option<AnyElement> {
    if !workbench.update_modal.open {
        return None;
    }

    let locale = workbench.locale;
    let state = &workbench.update_modal;
    let current_version = env!("CARGO_PKG_VERSION");
    let is_preparing = matches!(state.phase, UpdateModalPhase::Preparing);
    let is_downloading = matches!(state.phase, UpdateModalPhase::Downloading { .. });
    let active_local_sessions = workbench
        .local_supervisor
        .as_ref()
        .map(|s| s.session_count())
        .unwrap_or(0);

    let header_icon: AnyElement = match &state.phase {
        UpdateModalPhase::Checking | UpdateModalPhase::Preparing => Spinner::new()
            .small()
            .color(cx.theme().primary)
            .into_any_element(),
        UpdateModalPhase::UpToDate => Icon::new(IconName::CircleCheck)
            .small()
            .text_color(cx.theme().success)
            .into_any_element(),
        UpdateModalPhase::Error { .. } => Icon::new(IconName::TriangleAlert)
            .small()
            .text_color(cx.theme().danger)
            .into_any_element(),
        UpdateModalPhase::Idle | UpdateModalPhase::Available | UpdateModalPhase::Downloading { .. } => {
            Icon::new(IconName::ArrowUp)
                .small()
                .text_color(cx.theme().primary)
                .into_any_element()
        }
    };

    let (header_title, header_subtitle): (SharedString, Option<SharedString>) =
        match (&state.phase, &state.update) {
            (UpdateModalPhase::Checking, _) => (
                locale.t("正在检查更新…", "Checking for Updates…").into(),
                Some(format!("v{current_version}").into()),
            ),
            (UpdateModalPhase::UpToDate, _) => (
                locale.t("当前已是最新版本", "You're Up to Date").into(),
                Some(format!("v{current_version}").into()),
            ),
            (UpdateModalPhase::Error { .. }, None) => (
                locale.t("更新出错", "Update Error").into(),
                Some(format!("v{current_version}").into()),
            ),
            (_, Some(update)) => {
                let sub = if update.pub_date.is_empty() {
                    format!("v{current_version} → v{}", update.version)
                } else {
                    format!(
                        "v{current_version} → v{} · {}",
                        update.version, update.pub_date
                    )
                };
                (
                    locale
                        .t("发现 Gateterm 新版本", "A New Version of Gateterm Is Available")
                        .into(),
                    Some(sub.into()),
                )
            }
            (_, None) => ("Gateterm".into(), Some(format!("v{current_version}").into())),
        };

    let mut body = v_flex().w_full().p_4().gap_3();

    if let Some(update) = &state.update {
        let notes_text = if update.notes.trim().is_empty() {
            locale
                .t("（此版本暂无更新说明）", "(No release notes provided.)")
                .to_owned()
        } else {
            update.notes.clone()
        };
        body = body.child(
            v_flex()
                .w_full()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().muted_foreground)
                        .child(locale.t("更新内容", "Release Notes")),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(200.0))
                        .rounded(px(6.0))
                        .bg(cx.theme().secondary.opacity(0.45))
                        .border_1()
                        .border_color(cx.theme().border)
                        .overflow_hidden()
                        .child(
                            div()
                                .id("update-release-notes-scroll")
                                .size_full()
                                .overflow_y_scrollbar()
                                .p_3()
                                .child(
                                    div()
                                        .w_full()
                                        .min_w(px(0.0))
                                        .overflow_x_hidden()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(SharedString::from(notes_text)),
                                ),
                        ),
                ),
        );
    } else {
        let status_body: AnyElement = match &state.phase {
            UpdateModalPhase::Checking => h_flex()
                .w_full()
                .py_6()
                .items_center()
                .justify_center()
                .gap_2p5()
                .child(Spinner::new().small().color(cx.theme().primary))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(locale.t(
                            "正在获取并验证签名更新清单…",
                            "Fetching and verifying the signed release manifest…",
                        )),
                )
                .into_any_element(),
            UpdateModalPhase::UpToDate => div()
                .w_full()
                .py_4()
                .text_xs()
                .text_color(cx.theme().foreground)
                .child(SharedString::from(match locale {
                    Locale::ZhCn => {
                        format!("当前运行的 Gateterm v{current_version} 已是最新稳定版本。")
                    }
                    Locale::En => format!(
                        "You are running the latest stable version of Gateterm (v{current_version})."
                    ),
                }))
                .into_any_element(),
            UpdateModalPhase::Error { .. } => div()
                .w_full()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(locale.t(
                    "无法完成更新检查或应用。请查看下方详情并重试或手动下载。",
                    "Could not complete the update check or installation. See details below to retry or download manually.",
                ))
                .into_any_element(),
            UpdateModalPhase::Idle
            | UpdateModalPhase::Available
            | UpdateModalPhase::Downloading { .. }
            | UpdateModalPhase::Preparing => div().into_any_element(),
        };
        body = body.child(status_body);
    }

    if let Some(update) = &state.update
        && update.can_self_apply
        && !matches!(
            state.phase,
            UpdateModalPhase::Checking | UpdateModalPhase::UpToDate | UpdateModalPhase::Idle
        )
    {
        let (warn_main, warn_sub) = terminal_warning_copy(active_local_sessions, locale);
        let warning_color = cx.theme().warning;
        body = body.child(
            h_flex()
                .w_full()
                .p_2p5()
                .gap_2()
                .items_start()
                .rounded(px(6.0))
                .bg(warning_color.opacity(0.12))
                .border_1()
                .border_color(warning_color.opacity(0.45))
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .small()
                        .text_color(warning_color),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(warning_color)
                                .child(SharedString::from(warn_main)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(SharedString::from(warn_sub)),
                        ),
                ),
        );
    }

    match &state.phase {
        UpdateModalPhase::Downloading { downloaded, total } => {
            let pct = if *total > 0 {
                ((*downloaded as f64 / *total as f64) * 100.0).clamp(0.0, 100.0) as f32
            } else {
                0.0
            };
            let phase_text = if *total > 0 && *downloaded >= *total {
                locale.t("正在校验并解压更新…", "Verifying and extracting update…")
            } else {
                locale.t("正在下载更新…", "Downloading update…")
            };
            body = body.child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(phase_text),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SharedString::from(format!(
                                        "{} / {} ({:.0}%)",
                                        format_mb(*downloaded),
                                        format_mb(*total),
                                        pct
                                    ))),
                            ),
                    )
                    .child(Progress::new("update-download-progress").small().value(pct)),
            );
        }
        UpdateModalPhase::Preparing => {
            body = body.child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(Spinner::new().xsmall().color(cx.theme().primary))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(locale.t("正在准备重启…", "Preparing to restart…")),
                            ),
                    )
                    .child(
                        Progress::new("update-preparing-progress")
                            .small()
                            .value(100.0),
                    ),
            );
        }
        UpdateModalPhase::Error {
            message,
            fallback_url,
        } => {
            let danger_color = cx.theme().danger;
            let mut actions = h_flex().items_center().gap_2().flex_wrap();
            if !workbench.quitting {
                actions = actions.child(
                    Button::new("update-modal-retry")
                        .xsmall()
                        .secondary()
                        .icon(IconName::Redo)
                        .label(locale.t("重试", "Retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.trigger_manual_update_check(cx);
                        })),
                );
            }
            if let Some(url) = fallback_url.clone() {
                actions = actions.child(
                    Button::new("update-modal-release-link")
                        .xsmall()
                        .secondary()
                        .icon(IconName::ExternalLink)
                        .label(locale.t("前往 GitHub 手动下载", "Open Release Page"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_update_release_page(url.clone(), cx);
                        })),
                );
            }
            body = body.child(
                v_flex()
                    .w_full()
                    .p_2p5()
                    .gap_2()
                    .rounded(px(6.0))
                    .bg(danger_color.opacity(0.12))
                    .border_1()
                    .border_color(danger_color.opacity(0.45))
                    .child(
                        div()
                            .text_xs()
                            .text_color(danger_color)
                            .child(SharedString::from(message.clone())),
                    )
                    .child(actions),
            );
        }
        UpdateModalPhase::Idle
        | UpdateModalPhase::Checking
        | UpdateModalPhase::Available
        | UpdateModalPhase::UpToDate => {}
    }

    let can_skip = !workbench.quitting
        && state.update.is_some()
        && matches!(
            state.phase,
            UpdateModalPhase::Available | UpdateModalPhase::Error { .. }
        );

    let mut footer_left = h_flex().items_center().gap_2();
    if can_skip && let Some(update) = &state.update {
        let skip_version = update.version.clone();
        footer_left = footer_left.child(
            Button::new("update-modal-skip")
                .small()
                .ghost()
                .label(locale.t("跳过此版本", "Skip This Version"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.skip_current_update_version(skip_version.clone(), cx);
                })),
        );
    }

    let secondary_label = if matches!(state.phase, UpdateModalPhase::UpToDate) || workbench.quitting
    {
        locale.t("关闭", "Close")
    } else {
        locale.t("取消", "Cancel")
    };

    let mut footer_right = h_flex().items_center().gap_2().child(
        Button::new("update-modal-cancel")
            .small()
            .ghost()
            .disabled(is_preparing)
            .label(secondary_label)
            .on_click(cx.listener(|this, _, _, cx| {
                this.cancel_or_close_update_modal(cx);
            })),
    );

    if let Some(update) = &state.update
        && matches!(
            state.phase,
            UpdateModalPhase::Available
                | UpdateModalPhase::Downloading { .. }
                | UpdateModalPhase::Preparing
        )
    {
        let can_self_apply = update.can_self_apply;
        let release_url = update.release_url.clone();
        footer_right = footer_right.child(
            Button::new("update-modal-primary")
                .small()
                .primary()
                .disabled(is_downloading || is_preparing)
                .label(primary_action_label(can_self_apply, locale))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if can_self_apply {
                        this.start_update_download_and_apply(cx);
                    } else {
                        this.open_update_release_page(release_url.clone(), cx);
                    }
                })),
        );
    }

    let dialog = v_flex()
        .id("update-modal-card")
        .occlude()
        .w(px(560.0))
        .rounded(px(8.0))
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            h_flex()
                .w_full()
                .px_4()
                .py_3()
                .items_center()
                .justify_between()
                .bg(cx.theme().tab_bar)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .items_center()
                        .gap_2p5()
                        .child(header_icon)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(header_title),
                                )
                                .children(header_subtitle.map(|sub| {
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(sub)
                                })),
                        ),
                )
                .child(
                    Button::new("update-modal-close")
                        .xsmall()
                        .ghost()
                        .disabled(is_preparing)
                        .icon(IconName::Close)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_or_close_update_modal(cx);
                        })),
                ),
        )
        .child(body)
        .child(
            h_flex()
                .w_full()
                .px_4()
                .py_3()
                .items_center()
                .justify_between()
                .bg(cx.theme().tab_bar)
                .border_t_1()
                .border_color(cx.theme().border)
                .child(footer_left)
                .child(footer_right),
        );

    Some(
        div()
            .id("update-modal-backdrop")
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
                    if !this.update_modal.is_downloading_or_preparing() && !this.quitting {
                        this.cancel_or_close_update_modal(cx);
                    }
                }),
            )
            .child(dialog)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use gateterm_updater::{
        ArchiveFormat, AvailableUpdate, CheckOutcome, DownloadProgress, TargetArtifact,
    };

    fn sample_update(can_self_apply: bool) -> AvailableUpdate {
        AvailableUpdate {
            version: "1.0.4".to_owned(),
            tag: "release-v1.0.4".to_owned(),
            pub_date: "2026-11-27T08:00:00Z".to_owned(),
            release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4"
                .to_owned(),
            notes: "Bug fixes".to_owned(),
            artifact: can_self_apply.then(|| TargetArtifact {
                url: "https://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/app.zip"
                    .to_owned(),
                sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
                size: 1024,
                format: ArchiveFormat::Zip,
                binary_path: "gateterm-desktop-native.exe".to_owned(),
            }),
            can_self_apply,
        }
    }

    #[test]
    fn terminal_warning_copy_includes_session_count_and_remote_note() {
        let (zh_main, zh_sub) = terminal_warning_copy(2, Locale::ZhCn);
        assert!(zh_main.contains('2') && zh_sub.contains("远程"));
        let (en_main, en_sub) = terminal_warning_copy(2, Locale::En);
        assert!(en_main.contains('2') && en_sub.to_lowercase().contains("remote"));
        assert!(!en_main.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        assert!(!en_sub.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));

        let (en_zero, _) = terminal_warning_copy(0, Locale::En);
        assert!(!en_zero.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        let (en_one, _) = terminal_warning_copy(1, Locale::En);
        assert!(en_one.contains('1') && en_one.contains("session "));
    }

    #[test]
    fn auto_present_respects_skip_and_hidden_to_tray_state() {
        assert_eq!(should_present_auto_update(false, false), AutoPresentAction::ShowNow);
        assert_eq!(
            should_present_auto_update(true, false),
            AutoPresentAction::DeferUntilShown
        );
        assert_eq!(should_present_auto_update(false, true), AutoPresentAction::Suppress);
        assert_eq!(should_present_auto_update(true, true), AutoPresentAction::Suppress);
    }

    #[test]
    fn primary_action_label_switches_between_self_apply_and_macos_download_link() {
        assert_eq!(primary_action_label(true, Locale::En), "Update & Restart");
        assert_eq!(primary_action_label(false, Locale::En), "Open Download Page");
        assert_eq!(primary_action_label(true, Locale::ZhCn), "更新并重启");
        assert_eq!(primary_action_label(false, Locale::ZhCn), "前往下载页面");
    }

    #[test]
    fn cancel_and_stale_generation_guards_ignore_late_check_or_download_results() {
        let update = sample_update(true);
        let mut state = UpdateModalState::default();

        // Hidden startup update sets pending_on_show without opening while hidden.
        assert!(!state.apply_check_result(
            0,
            false,
            true,
            true,
            Locale::En,
            Ok(CheckOutcome::UpdateAvailable {
                update: update.clone(),
                skipped: false,
            }),
        ));
        assert!(!state.open);
        assert!(state.pending_on_show);
        assert!(state.on_show_window());
        assert!(state.open);
        assert!(!state.pending_on_show);

        // Subsequent 24h check updates metadata without opening when modal is closed.
        state.cancel();
        assert!(!state.open);
        state.apply_check_result(
            0,
            false,
            false,
            false,
            Locale::En,
            Ok(CheckOutcome::UpdateAvailable {
                update: update.clone(),
                skipped: false,
            }),
        );
        assert!(!state.open);
        assert_eq!(state.update.as_ref().map(|u| u.version.as_str()), Some("1.0.4"));

        // Manual check -> cancel -> late check result is ignored and never opens modal.
        let check_gen = state.begin_manual_check().unwrap();
        assert!(state.open);
        state.cancel();
        assert!(!state.open);
        assert!(!state.apply_check_result(
            check_gen,
            true,
            false,
            false,
            Locale::En,
            Ok(CheckOutcome::UpdateAvailable {
                update: update.clone(),
                skipped: false,
            }),
        ));
        assert!(!state.open);

        // Fresh manual check -> Available -> Downloading -> Cancel -> late progress/staged ignored.
        let check_gen2 = state.begin_manual_check().unwrap();
        assert!(state.apply_check_result(
            check_gen2,
            true,
            false,
            false,
            Locale::En,
            Ok(CheckOutcome::UpdateAvailable {
                update: update.clone(),
                skipped: false,
            }),
        ));
        let dl_gen = state.begin_download(1024).unwrap();
        state.cancel();
        assert!(!state.open);
        assert!(!state.apply_download_progress(
            dl_gen,
            DownloadProgress {
                downloaded: 512,
                total: 1024,
            },
        ));
        assert!(matches!(
            state.apply_staged_result(dl_gen, Ok("staged")),
            StagedAction::Ignored
        ));
        assert!(!matches!(state.phase, UpdateModalPhase::Preparing));
    }

    #[test]
    fn manual_check_bypasses_skip_and_en_copy_contains_no_cjk() {
        let update = sample_update(false);
        let mut state = UpdateModalState::default();

        // Startup skipped update stays suppressed.
        assert!(!state.apply_check_result(
            0,
            false,
            true,
            false,
            Locale::En,
            Ok(CheckOutcome::UpdateAvailable {
                update: update.clone(),
                skipped: true,
            }),
        ));
        assert!(!state.open);
        assert!(!state.pending_on_show);

        // Manual check ignores skipped=true and opens Available.
        let check_gen = state.begin_manual_check().unwrap();
        assert!(state.apply_check_result(
            check_gen,
            true,
            false,
            false,
            Locale::En,
            Ok(CheckOutcome::UpdateAvailable {
                update: update.clone(),
                skipped: true,
            }),
        ));
        assert!(state.open);
        assert_eq!(state.phase, UpdateModalPhase::Available);
        assert!(!state
            .status_line("1.0.3", Locale::En)
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));

        // Download / preflight error formatting in English contains zero CJK characters.
        let err_msg = format_download_error(
            Locale::En,
            &DownloadError::NotWritable {
                path: std::path::PathBuf::from("/opt/gateterm"),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            },
        );
        assert!(!err_msg.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
    }
}
