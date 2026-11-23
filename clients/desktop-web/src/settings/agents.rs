//! In-process agent hook integration UI state and rendering.

use std::collections::HashMap;

use gateterm_server::agent::{
    ops::{AgentPlanSummary, PlanGroup},
    AgentStatus, Integration,
};

use crate::i18n::Locale;
use crate::settings::state::Message;
use crate::tr;

const OK_COLOR: egui::Color32 = egui::Color32::from_rgb(60, 140, 60);
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(190, 125, 25);
const ERR_COLOR: egui::Color32 = egui::Color32::from_rgb(180, 60, 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActionKind {
    Install,
    Uninstall,
    Purge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentUiAction {
    Refresh,
    Preview(String),
    Install(String),
    Uninstall(String),
    Purge(String),
}

#[derive(Debug, Clone, Default)]
pub struct AgentsView {
    pub listen_port: Option<u16>,
    pub install_enabled: bool,
    pub loading: bool,
    pub busy_id: Option<String>,
    pub items: Vec<AgentStatus>,
    pub expanded_preview: Option<String>,
    pub previews: HashMap<String, Result<AgentPlanSummary, String>>,
    pub confirm_purge_id: Option<String>,
    pub messages: HashMap<String, Message>,
}

impl AgentsView {
    pub fn needs_attention(&self) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item.integration,
                Integration::StalePort | Integration::Outdated | Integration::NeedsActivation
            )
        })
    }
}

fn is_primary(item: &AgentStatus) -> bool {
    item.detected.installed || item.integration != Integration::NotInstalled
}

pub fn partition_items(items: &[AgentStatus]) -> (Vec<&AgentStatus>, Vec<&AgentStatus>) {
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    for item in items {
        if is_primary(item) {
            primary.push(item);
        } else {
            secondary.push(item);
        }
    }
    (primary, secondary)
}

pub fn activation_text(key: &str, locale: Locale) -> Option<&'static str> {
    match key {
        "codex-hooks-review" => Some(locale.text(
            "装好之后还要在 Codex 里执行 /hooks 审核一遍才会生效 —— 这是 Codex 自己的安全闸,只能由你来点头。每次重装(端口或程序位置变了)之后都要重新审核",
            "After installing, run /hooks inside Codex to review them — that gate is Codex's own, and only you can clear it. Every reinstall (new port or binary location) needs a fresh review",
        )),
        "pi-reload" => Some(locale.text(
            "装的是一个 pi 扩展,新启动的 pi 自动加载;已经在跑的 pi 要执行 /reload 或重启。pi 没有授权审批,只上报状态和聊天记录",
            "This installs a pi extension. New pi sessions load it automatically; running ones need /reload or a restart. pi has no approval prompts, so only status and chat are reported",
        )),
        "grok-reload" => Some(locale.text(
            "装在 ~/.grok/hooks/ 里,新启动的 grok 自动生效;已经在跑的要在 /hooks 里重新加载或重启。grok 没有授权审批,只上报状态和聊天记录",
            "Installed under ~/.grok/hooks/. New grok sessions pick it up; running ones need a reload from /hooks or a restart. grok has no approval prompts, so only status and chat are reported",
        )),
        "antigravity-restart" => Some(locale.text(
            "写在 ~/.gemini/config/hooks.json 的 gateterm 条目里,新启动的 agy 自动生效,已经在跑的要重启。只上报状态和聊天记录,不碰 agy 的授权确认",
            "Written as the gateterm entry in ~/.gemini/config/hooks.json. New agy sessions pick it up; running ones need a restart. Only status and chat are reported — agy's own permission prompts are left alone",
        )),
        _ => None,
    }
}

fn status_badge(
    integration: Integration,
    locale: Locale,
    muted: egui::Color32,
) -> (&'static str, egui::Color32) {
    match integration {
        Integration::Installed => (locale.text("已安装", "Installed"), OK_COLOR),
        Integration::StalePort => (locale.text("端口不匹配", "Stale port"), WARN_COLOR),
        Integration::Outdated => (locale.text("需更新", "Outdated"), WARN_COLOR),
        Integration::NeedsActivation => (locale.text("待放行", "Needs activation"), WARN_COLOR),
        Integration::NotInstalled => (locale.text("未安装", "Not installed"), muted),
    }
}

/// Pure UI rendering for the Agents tab. Never executes filesystem IO directly;
/// returns [`AgentUiAction`] for `app.rs` to run off-thread.
pub fn draw_agents(
    ui: &mut egui::Ui,
    view: &mut AgentsView,
    locale: Locale,
) -> Option<AgentUiAction> {
    let mut action: Option<AgentUiAction> = None;
    let any_busy = view.busy_id.is_some();

    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!any_busy, egui::Button::new(locale.text("刷新", "Refresh")))
            .clicked()
        {
            action = Some(AgentUiAction::Refresh);
        }
        if view.loading || any_busy {
            ui.spinner();
        }
    });
    ui.small(locale.text(
        "安装会修改 AI 自己的配置文件，改前会自动备份；卸载可完全还原。",
        "Installing edits the agent's own config file (backed up first); uninstalling restores it.",
    ));

    if view.listen_port.is_none() {
        ui.add_space(4.0);
        ui.colored_label(
            ERR_COLOR,
            egui::RichText::new(locale.text(
                "当前服务未监听，无法安装或重新安装 hook。请先在「常规」中保存并启动监听端口。",
                "The server is not listening. Save and start a listener on the General tab before installing or reinstalling hooks.",
            ))
            .small(),
        );
    }
    if !view.install_enabled {
        ui.add_space(4.0);
        ui.colored_label(
            WARN_COLOR,
            egui::RichText::new(locale.text(
                "配置文件中 agents.install_enabled = false，已禁用安装和卸载（仍可清理残留 hook）。",
                "Installing and uninstalling are disabled (agents.install_enabled = false in config.toml). Purging existing hooks remains available.",
            ))
            .small(),
        );
    }

    ui.add_space(6.0);

    let mut primary_indices = Vec::new();
    let mut secondary_indices = Vec::new();
    for (idx, item) in view.items.iter().enumerate() {
        if is_primary(item) {
            primary_indices.push(idx);
        } else {
            secondary_indices.push(idx);
        }
    }

    if view.loading && view.items.is_empty() {
        ui.small(locale.text("正在扫描本机 AI Agent…", "Scanning local AI agents..."));
    } else if primary_indices.is_empty() {
        ui.small(locale.text("未检测到已安装的 AI Agent", "No installed AI agents detected"));
    } else {
        for idx in primary_indices {
            draw_card(ui, view, idx, locale, &mut action);
            ui.add_space(4.0);
        }
    }

    if !secondary_indices.is_empty() {
        ui.add_space(4.0);
        let label = tr!(
            locale,
            "未检测到 ({})",
            "Not detected ({})",
            secondary_indices.len()
        );
        egui::CollapsingHeader::new(label)
            .id_salt("agents-not-detected")
            .default_open(false)
            .show(ui, |ui| {
                for idx in secondary_indices {
                    draw_card(ui, view, idx, locale, &mut action);
                    ui.add_space(4.0);
                }
            });
    }

    action
}

fn draw_card(
    ui: &mut egui::Ui,
    view: &mut AgentsView,
    idx: usize,
    locale: Locale,
    action: &mut Option<AgentUiAction>,
) {
    let id = view.items[idx].id;
    let name = view.items[idx].name;
    let integration = view.items[idx].integration;
    let activation = view.items[idx].activation;

    let any_busy = view.busy_id.is_some();
    let can_install = !any_busy && view.listen_port.is_some() && view.install_enabled;
    let can_uninstall = !any_busy && view.install_enabled;
    let can_other = !any_busy;

    let is_expanded = view.expanded_preview.as_deref() == Some(id);
    let is_confirming_purge = view.confirm_purge_id.as_deref() == Some(id);

    ui.group(|ui| {
        ui.set_width(ui.available_width());

        let (badge_text, badge_color) =
            status_badge(integration, locale, ui.visuals().weak_text_color());
        ui.horizontal_wrapped(|ui| {
            ui.strong(name);
            ui.colored_label(badge_color, egui::RichText::new(badge_text).small());
        });

        if !view.items[idx].detected.detail.is_empty() {
            ui.small(view.items[idx].detected.detail.as_str());
        }

        match integration {
            Integration::StalePort => {
                ui.colored_label(
                    WARN_COLOR,
                    egui::RichText::new(locale.text(
                        "已安装，但端口对不上（请点击「重新安装」对齐当前端口）",
                        "Installed, but pointing at a different port (click Reinstall to match this server)",
                    ))
                    .small(),
                );
            }
            Integration::Outdated => {
                ui.colored_label(
                    WARN_COLOR,
                    egui::RichText::new(locale.text(
                        "已安装，但和当前服务不一致（请点击「重新安装」更新配置）",
                        "Installed, but out of sync with this server (click Reinstall to update)",
                    ))
                    .small(),
                );
            }
            _ => {}
        }

        if let Some(key) = activation
            && let Some(hint) = activation_text(key, locale)
        {
            ui.colored_label(WARN_COLOR, egui::RichText::new(hint).small());
        }

        if !view.items[idx].conflicts.is_empty() {
            for conflict in &view.items[idx].conflicts {
                ui.colored_label(ERR_COLOR, egui::RichText::new(conflict.as_str()).small());
            }
            ui.small(locale.text(
                "检测到别家阻塞式 hook，只会安装状态上报部分",
                "Another program already has a blocking hook on this event; only status reporting will be installed",
            ));
        }

        let mut clicked_preview = false;
        let mut clicked_purge = false;

        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            if integration == Integration::NotInstalled {
                if ui
                    .add_enabled(can_install, egui::Button::new(locale.text("安装", "Install")))
                    .clicked()
                {
                    *action = Some(AgentUiAction::Install(id.to_string()));
                }
            } else {
                if ui
                    .add_enabled(
                        can_uninstall,
                        egui::Button::new(locale.text("卸载", "Uninstall")),
                    )
                    .clicked()
                {
                    *action = Some(AgentUiAction::Uninstall(id.to_string()));
                }
                if matches!(integration, Integration::StalePort | Integration::Outdated)
                    && ui
                        .add_enabled(
                            can_install,
                            egui::Button::new(locale.text("重新安装", "Reinstall")),
                        )
                        .clicked()
                {
                    *action = Some(AgentUiAction::Install(id.to_string()));
                }
            }

            let preview_label = if is_expanded {
                locale.text("收起", "Hide")
            } else {
                locale.text("看看会改什么", "Preview")
            };
            if ui
                .add_enabled(can_other, egui::Button::new(preview_label))
                .clicked()
            {
                clicked_preview = true;
            }

            let purge_btn = ui
                .add_enabled(
                    can_other,
                    egui::Button::new(locale.text("清理 Gateterm hooks", "Purge")),
                )
                .on_hover_text(locale.text(
                    "删掉这个 AI 配置里所有 gateterm 写过的 hook（不论端口和事件），用于清理残留；改前会自动备份",
                    "Remove every hook gateterm ever wrote into this AI's config (any port, any event) to clean up leftovers; backed up first",
                ));
            if purge_btn.clicked() {
                clicked_purge = true;
            }
        });

        if clicked_preview {
            if is_expanded {
                view.expanded_preview = None;
            } else {
                *action = Some(AgentUiAction::Preview(id.to_string()));
            }
        }
        if clicked_purge {
            view.confirm_purge_id = if is_confirming_purge {
                None
            } else {
                Some(id.to_string())
            };
        }

        if view.confirm_purge_id.as_deref() == Some(id) {
            let mut close_confirm = false;
            let mut do_purge = false;
            ui.add_space(4.0);
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.small(tr!(
                    locale,
                    "清空 {} 配置里所有 gateterm 的 hook？你自己的 hook 不受影响，改前会自动备份。",
                    "Remove all gateterm hooks from {}'s config? Your own hooks are untouched, and the file is backed up first.",
                    name
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            can_other,
                            egui::Button::new(
                                egui::RichText::new(locale.text("确认清理", "Confirm purge"))
                                    .color(ERR_COLOR),
                            ),
                        )
                        .clicked()
                    {
                        do_purge = true;
                        close_confirm = true;
                    }
                    if ui.button(locale.text("取消", "Cancel")).clicked() {
                        close_confirm = true;
                    }
                });
            });
            if close_confirm {
                view.confirm_purge_id = None;
            }
            if do_purge {
                *action = Some(AgentUiAction::Purge(id.to_string()));
            }
        }

        if view.expanded_preview.as_deref() == Some(id) {
            ui.add_space(4.0);
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                match view.previews.get(id) {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.small(locale.text("正在加载预演…", "Loading preview..."));
                        });
                    }
                    Some(Err(err)) => {
                        ui.colored_label(
                            ERR_COLOR,
                            egui::RichText::new(tr!(
                                locale,
                                "读不到预演结果：{err}",
                                "Could not load preview: {err}"
                            ))
                            .small(),
                        );
                    }
                    Some(Ok(summary)) => {
                        if !summary.include_blocking {
                            ui.colored_label(
                                WARN_COLOR,
                                egui::RichText::new(locale.text(
                                    "只装状态上报，不装授权拦截（见上方冲突）",
                                    "Status reporting only, no approval interception (see conflict above)",
                                ))
                                .small(),
                            );
                        }
                        let show_install = matches!(
                            integration,
                            Integration::NotInstalled | Integration::StalePort | Integration::Outdated
                        );
                        let show_uninstall = integration != Integration::NotInstalled;
                        if show_install {
                            draw_plan_groups(
                                ui,
                                locale.text("写入：", "Write:"),
                                &summary.install,
                                locale,
                            );
                        }
                        if show_uninstall {
                            draw_plan_groups(
                                ui,
                                locale.text("移除：", "Remove:"),
                                &summary.uninstall,
                                locale,
                            );
                        }
                    }
                }
            });
        }

        if let Some(msg) = view.messages.get(id) {
            ui.add_space(2.0);
            match msg {
                Message::Ok(t) => ui.colored_label(OK_COLOR, egui::RichText::new(t.as_str()).small()),
                Message::Err(t) => ui.colored_label(ERR_COLOR, egui::RichText::new(t.as_str()).small()),
            };
        }
    });
}

fn draw_plan_groups(
    ui: &mut egui::Ui,
    prefix: &str,
    groups: &[PlanGroup],
    locale: Locale,
) {
    if groups.is_empty() {
        ui.small(format!(
            "{prefix} {}",
            locale.text("(无文件变更)", "(no file changes)")
        ));
        return;
    }
    for group in groups {
        ui.small(group.path.display().to_string());
        ui.small(format!("{prefix} {}", group.events.join(", ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateterm_server::agent::{Confidence, Detected};

    fn mock_status(id: &'static str, detected: bool, integration: Integration) -> AgentStatus {
        AgentStatus {
            id,
            name: id,
            detected: Detected {
                installed: detected,
                confidence: Confidence::High,
                reason: "mock",
                detail: "Mock detail".into(),
                config_path: None,
            },
            integration,
            activation: None,
            conflicts: Vec::new(),
        }
    }

    #[test]
    fn needs_attention_flags_stale_outdated_and_activation_only() {
        for (integration, expected) in [
            (Integration::Installed, false),
            (Integration::NotInstalled, false),
            (Integration::StalePort, true),
            (Integration::Outdated, true),
            (Integration::NeedsActivation, true),
        ] {
            let view = AgentsView {
                items: vec![mock_status("claude-code", true, integration)],
                ..Default::default()
            };
            assert_eq!(
                view.needs_attention(),
                expected,
                "unexpected needs_attention for {integration:?}"
            );
        }
    }

    #[test]
    fn partition_items_puts_detected_or_integrated_first() {
        let items = vec![
            mock_status("undetected-uninstalled", false, Integration::NotInstalled),
            mock_status("detected-uninstalled", true, Integration::NotInstalled),
            mock_status("undetected-but-stale", false, Integration::StalePort),
            mock_status("detected-installed", true, Integration::Installed),
        ];

        let (primary, secondary) = partition_items(&items);
        let primary_ids: Vec<&str> = primary.into_iter().map(|a| a.id).collect();
        let secondary_ids: Vec<&str> = secondary.into_iter().map(|a| a.id).collect();
        assert_eq!(
            primary_ids,
            vec!["detected-uninstalled", "undetected-but-stale", "detected-installed"]
        );
        assert_eq!(secondary_ids, vec!["undetected-uninstalled"]);
    }

    #[test]
    fn activation_text_covers_all_known_keys_in_both_locales() {
        for key in [
            "codex-hooks-review",
            "pi-reload",
            "grok-reload",
            "antigravity-restart",
        ] {
            let en = activation_text(key, Locale::En).expect(key);
            let zh = activation_text(key, Locale::ZhCn).expect(key);
            assert!(!en.is_empty());
            assert!(!zh.is_empty());
            assert_ne!(en, zh);
            assert!(
                !en.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "English activation hint for {key} contains Chinese: {en}"
            );
        }
        assert_eq!(activation_text("unknown-key", Locale::En), None);
    }
}
