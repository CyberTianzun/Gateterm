//! Render the real settings panel without creating a window or GPU context.
use std::collections::HashMap;
use std::path::PathBuf;

use gateterm_desktop_web::i18n::Locale;
use gateterm_desktop_web::settings::{
    agents::AgentsView,
    cloud::{CloudForm, CloudStatus},
    state::{Form, Message},
    ui::{SettingsTab, View, draw},
    web_addr::WebAddrPicker,
};
use gateterm_desktop_web::update_ui::{self, UpdatePhase, UpdateView};
use gateterm_server::agent::{
    AgentStatus, Confidence, Detected, Integration,
    ops::{AgentPlanSummary, PlanGroup},
};
use gateterm_updater::{ArchiveFormat, AvailableUpdate, DownloadProgress, TargetArtifact};

fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>, max_width: f32) {
    match shape {
        egui::Shape::Text(text) => {
            assert!(
                text.pos.x + text.galley.size().x <= max_width + 0.5,
                "text overflows the window (x={}, width={}, max={}): {}",
                text.pos.x,
                text.galley.size().x,
                max_width,
                text.galley.text()
            );
            texts.push(text.galley.text().to_owned());
        }
        egui::Shape::Vec(shapes) => shapes
            .iter()
            .for_each(|shape| collect_text(shape, texts, max_width)),
        _ => {}
    }
}

fn render_view(ctx: &egui::Context, view: &mut View, locale: Locale, height: f32) -> Vec<String> {
    let mut texts = vec![];
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, height),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            draw(ui, view, locale);
        });
        output.textures_delta.clear();
        texts.clear();
        for shape in &output.shapes {
            collect_text(&shape.shape, &mut texts, 600.0);
        }
    }
    if locale == Locale::En {
        assert!(
            !texts
                .iter()
                .any(|text| text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))),
            "Locale::En rendered CJK characters: {texts:?}"
        );
    }
    texts
}

fn render_update_view(
    ctx: &egui::Context,
    view: &mut UpdateView,
    locale: Locale,
    height: f32,
) -> Vec<String> {
    let mut texts = vec![];
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(520.0, height),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            update_ui::draw(ui, view, locale);
        });
        output.textures_delta.clear();
        texts.clear();
        for shape in &output.shapes {
            collect_text(&shape.shape, &mut texts, 520.0);
        }
    }
    if locale == Locale::En {
        assert!(
            !texts
                .iter()
                .any(|text| text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))),
            "Locale::En rendered CJK characters in update window: {texts:?}"
        );
    }
    texts
}

fn mock_agents_view() -> AgentsView {
    let items = vec![
        AgentStatus {
            id: "claude-code",
            name: "Claude Code",
            detected: Detected {
                installed: true,
                confidence: Confidence::High,
                reason: "cli-on-path",
                detail: "CLI found on PATH (/usr/local/bin/claude)".into(),
                config_path: Some(PathBuf::from("/Users/tester/.claude/settings.json")),
            },
            integration: Integration::Installed,
            activation: None,
            conflicts: vec![],
        },
        AgentStatus {
            id: "pi",
            name: "pi",
            detected: Detected {
                installed: true,
                confidence: Confidence::Medium,
                reason: "dir-present",
                detail: "Found existing directory ~/.pi/agent".into(),
                config_path: Some(PathBuf::from("/Users/tester/.pi/agent/extensions/gateterm.ts")),
            },
            integration: Integration::Outdated,
            activation: Some("pi-reload"),
            conflicts: vec![],
        },
        AgentStatus {
            id: "grok",
            name: "Grok CLI",
            detected: Detected {
                installed: true,
                confidence: Confidence::Medium,
                reason: "dir-present",
                detail: "Found existing directory ~/.grok".into(),
                config_path: Some(PathBuf::from("/Users/tester/.grok/hooks/gateterm.json")),
            },
            integration: Integration::StalePort,
            activation: Some("grok-reload"),
            conflicts: vec![],
        },
        AgentStatus {
            id: "codex",
            name: "Codex",
            detected: Detected {
                installed: true,
                confidence: Confidence::High,
                reason: "cli-on-path",
                detail: "CLI found on PATH (/usr/local/bin/codex)".into(),
                config_path: Some(PathBuf::from("/Users/tester/.codex/hooks.json")),
            },
            integration: Integration::NeedsActivation,
            activation: Some("codex-hooks-review"),
            conflicts: vec![],
        },
        AgentStatus {
            id: "antigravity",
            name: "Antigravity",
            detected: Detected {
                installed: true,
                confidence: Confidence::Medium,
                reason: "config-present",
                detail: "Config exists at ~/.gemini/config/hooks.json".into(),
                config_path: Some(PathBuf::from("/Users/tester/.gemini/config/hooks.json")),
            },
            integration: Integration::NotInstalled,
            activation: Some("antigravity-restart"),
            conflicts: vec![
                "PreToolUse: foreign blocking hook http://127.0.0.1:9999/other-tool".into(),
            ],
        },
        AgentStatus {
            id: "copilot",
            name: "GitHub Copilot CLI",
            detected: Detected {
                installed: false,
                confidence: Confidence::Low,
                reason: "not-found",
                detail: "Not found on PATH or home directory".into(),
                config_path: None,
            },
            integration: Integration::NotInstalled,
            activation: None,
            conflicts: vec![],
        },
    ];

    let mut previews = HashMap::new();
    previews.insert(
        "antigravity".into(),
        Ok(AgentPlanSummary {
            install: vec![PlanGroup {
                path: PathBuf::from("/Users/tester/.gemini/config/hooks.json"),
                events: vec!["SessionStart".into(), "Stop".into()],
            }],
            uninstall: vec![PlanGroup {
                path: PathBuf::from("/Users/tester/.gemini/config/hooks.json"),
                events: vec!["SessionStart".into(), "Stop".into()],
            }],
            include_blocking: false,
        }),
    );

    let mut messages = HashMap::new();
    messages.insert("claude-code".into(), Message::Ok("Installed".into()));
    messages.insert("pi".into(), Message::Err("Failed to update hooks".into()));

    AgentsView {
        listen_port: Some(7683),
        install_enabled: true,
        loading: false,
        busy_id: None,
        items,
        expanded_preview: Some("antigravity".into()),
        previews,
        confirm_purge_id: Some("codex".into()),
        messages,
    }
}

#[test]
fn settings_switch_language_in_place_and_fit_the_window() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);
    for listen_str in ["127.0.0.1:7683", "0.0.0.0:7683"] {
        let listen = listen_str.parse().unwrap();
        let mut view = View {
            tab: SettingsTab::General,
            form: Form::from_current(listen, false),
            web_addr: WebAddrPicker::new(Some(listen)),
            window_size: "smallest".into(),
            launchers: vec![],
            banner: Some("Test banner: check persistent header across tabs".into()),
            message: None,
            agents: mock_agents_view(),
            cloud: CloudForm::default(),
            cloud_status: CloudStatus::LoggedOut,
            cloud_busy: false,
            cloud_message: None,
            update_status_line: "v1.0.3".into(),
            update_checking: false,
        };
        for locale in [Locale::En, Locale::ZhCn, Locale::En] {
            view.tab = SettingsTab::General;
            let general_texts = render_view(&ctx, &mut view, locale, 680.0);
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("界面语言", "Language")),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("监听地址", "Listen address")),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("打开 Web 客户端", "Open web client")),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("网页标题", "Web title")),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("版本与更新", "Version & Updates")),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text.contains(env!("CARGO_PKG_VERSION"))),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("检查更新…", "Check for Updates…")),
                "{general_texts:?}"
            );
            assert!(
                general_texts
                    .iter()
                    .any(|text| text == locale.text("保存并应用", "Save and apply")),
                "{general_texts:?}"
            );

            view.tab = SettingsTab::Agents;
            let agents_viewport_texts = render_view(&ctx, &mut view, locale, 680.0);
            assert!(
                agents_viewport_texts
                    .iter()
                    .any(|text| text == locale.text("AI 集成 •", "Agents •")),
                "{agents_viewport_texts:?}"
            );
            assert!(
                agents_viewport_texts
                    .iter()
                    .any(|text| text == locale.text("刷新", "Refresh")),
                "{agents_viewport_texts:?}"
            );
            assert!(
                !agents_viewport_texts
                    .iter()
                    .any(|text| text == locale.text("保存并应用", "Save and apply")),
                "Save button should not render on Agents tab: {agents_viewport_texts:?}"
            );

            // Render at taller height (same 600px width) so ScrollArea doesn't clip lower cards,
            // expanded preview, or the collapsed Not detected group from overflow/CJK checks.
            let agents_full_texts = render_view(&ctx, &mut view, locale, 1800.0);
            assert!(
                agents_full_texts
                    .iter()
                    .any(|text| text == locale.text("确认清理", "Confirm purge")),
                "{agents_full_texts:?}"
            );
            assert!(
                agents_full_texts
                    .iter()
                    .any(|text| text == locale.text("未检测到 (1)", "Not detected (1)")),
                "{agents_full_texts:?}"
            );
            assert!(
                agents_full_texts
                    .iter()
                    .any(|text| text.starts_with(locale.text("写入：", "Write:"))),
                "{agents_full_texts:?}"
            );

            view.tab = SettingsTab::Cloud;
            let cloud_texts = render_view(&ctx, &mut view, locale, 680.0);
            assert!(
                cloud_texts
                    .iter()
                    .any(|text| text == locale.text("通过浏览器登录", "Sign in with browser")),
                "{cloud_texts:?}"
            );
            assert!(
                !cloud_texts
                    .iter()
                    .any(|text| text == locale.text("保存并应用", "Save and apply")),
                "Save button should not render on Cloud tab: {cloud_texts:?}"
            );
        }
    }
}

#[test]
fn agents_tab_warnings_fit_without_chinese_in_english() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);
    let listen = "127.0.0.1:7683".parse().unwrap();
    let mut agents = mock_agents_view();
    agents.listen_port = None;
    agents.install_enabled = false;
    agents.items.retain(|item| !item.detected.installed);

    let mut view = View {
        tab: SettingsTab::Agents,
        form: Form::from_current(listen, false),
        web_addr: WebAddrPicker::new(Some(listen)),
        window_size: "smallest".into(),
        launchers: vec![],
        banner: None,
        message: None,
        agents,
        cloud: CloudForm::default(),
        cloud_status: CloudStatus::LoggedOut,
        cloud_busy: false,
        cloud_message: None,
        update_status_line: "v1.0.3".into(),
        update_checking: false,
    };

    for locale in [Locale::En, Locale::ZhCn] {
        let texts = render_view(&ctx, &mut view, locale, 680.0);
        assert!(
            texts.iter().any(|t| {
                t == locale.text("未检测到已安装的 AI Agent", "No installed AI agents detected")
            }),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == locale.text("未检测到 (1)", "Not detected (1)")),
            "{texts:?}"
        );
    }
}

#[test]
fn agents_tab_clearing_feedback_on_locale_switch_removes_prior_chinese_text() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);
    let listen = "127.0.0.1:7683".parse().unwrap();
    let mut agents = mock_agents_view();
    agents.messages.insert(
        "claude-code".into(),
        Message::Err("读写配置失败：permission denied".into()),
    );
    agents
        .previews
        .insert("pi".into(), Err("未知的 Agent：pi".into()));
    agents.expanded_preview = Some("pi".into());

    let mut view = View {
        tab: SettingsTab::Agents,
        form: Form::from_current(listen, false),
        web_addr: WebAddrPicker::new(Some(listen)),
        window_size: "smallest".into(),
        launchers: vec![],
        banner: None,
        message: None,
        agents,
        cloud: CloudForm::default(),
        cloud_status: CloudStatus::LoggedOut,
        cloud_busy: false,
        cloud_message: None,
        update_status_line: "v1.0.3".into(),
        update_checking: false,
    };

    let _zh_texts = render_view(&ctx, &mut view, Locale::ZhCn, 1800.0);
    view.agents.clear_localized_feedback();
    let en_texts = render_view(&ctx, &mut view, Locale::En, 1800.0);
    assert!(!en_texts.iter().any(|t| t.contains("permission denied")));
    assert!(!en_texts.iter().any(|t| t.contains("未知")));
}

fn mock_available_update(can_self_apply: bool) -> AvailableUpdate {
    AvailableUpdate {
        version: "1.0.4".into(),
        tag: "release-v1.0.4".into(),
        pub_date: "2026-11-27T08:00:00Z".into(),
        release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4".into(),
        notes: "## What's Changed\n- Fast Sparkle-style self-update\n- Stability improvements"
            .into(),
        artifact: can_self_apply.then(|| TargetArtifact {
            url: "https://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/gateterm-desktop-web-v1.0.4-x86_64-pc-windows-msvc.zip".into(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            size: 14_820_352,
            format: ArchiveFormat::Zip,
            binary_path: "gateterm-desktop-web.exe".into(),
        }),
        can_self_apply,
    }
}

#[test]
fn update_window_available_self_apply_renders_notes_session_warning_and_three_buttons() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);
    let mut view = UpdateView {
        current_version: "1.0.3".into(),
        update: Some(mock_available_update(true)),
        phase: UpdatePhase::Available,
        active_sessions: 3,
    };

    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut view, locale, 480.0);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("v1.0.3") && t.contains("v1.0.4")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Fast Sparkle-style self-update")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains('3')
                && t.contains(locale.text("本地终端会话", "local terminal sessions"))),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == locale.text("更新并重启", "Update & Restart")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == locale.text("取消", "Cancel")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == locale.text("跳过此版本", "Skip This Version")),
            "{texts:?}"
        );
    }
}

#[test]
fn update_window_available_macos_fallback_shows_open_download_page_without_restart() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);
    let mut view = UpdateView {
        current_version: "1.0.3".into(),
        update: Some(mock_available_update(false)),
        phase: UpdatePhase::Available,
        active_sessions: 3,
    };

    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut view, locale, 480.0);
        assert!(
            texts
                .iter()
                .any(|t| t == locale.text("前往下载页面", "Open Download Page")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == locale.text("取消", "Cancel")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == locale.text("跳过此版本", "Skip This Version")),
            "{texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t == "Update & Restart" || t == "更新并重启"),
            "macOS fallback must not show self-apply restart button: {texts:?}"
        );
    }
}

#[test]
fn update_window_downloading_uptodate_and_error_render_without_cjk_in_english() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);

    let mut checking = UpdateView {
        current_version: "1.0.3".into(),
        update: None,
        phase: UpdatePhase::Checking,
        active_sessions: 0,
    };
    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut checking, locale, 480.0);
        assert!(texts.iter().any(|t| t.contains("1.0.3")), "{texts:?}");
        assert!(
            texts.iter().any(|t| t == locale.text("取消", "Cancel")),
            "{texts:?}"
        );
    }

    let mut downloading = UpdateView {
        current_version: "1.0.3".into(),
        update: Some(mock_available_update(true)),
        phase: UpdatePhase::Downloading(DownloadProgress {
            downloaded: 6_710_886,
            total: 14_820_352,
        }),
        active_sessions: 0,
    };
    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut downloading, locale, 480.0);
        assert!(texts.iter().any(|t| t.contains("6.4 MB")), "{texts:?}");
        assert!(
            texts
                .iter()
                .any(|t| t.contains(locale.text("关闭所有本地终端会话", "close any local terminal sessions"))),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == locale.text("取消", "Cancel")),
            "{texts:?}"
        );
    }

    let mut preparing = UpdateView {
        current_version: "1.0.3".into(),
        update: Some(mock_available_update(true)),
        phase: UpdatePhase::Preparing,
        active_sessions: 1,
    };
    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut preparing, locale, 480.0);
        assert!(
            texts
                .iter()
                .any(|t| t.contains(locale.text("1 个本地终端会话", "1 running local terminal session"))),
            "{texts:?}"
        );
    }

    let mut up_to_date = UpdateView {
        current_version: "1.0.3".into(),
        update: None,
        phase: UpdatePhase::UpToDate,
        active_sessions: 0,
    };
    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut up_to_date, locale, 480.0);
        assert!(texts.iter().any(|t| t.contains("1.0.3")), "{texts:?}");
    }

    let mut errored = UpdateView {
        current_version: "1.0.3".into(),
        update: Some(mock_available_update(true)),
        phase: UpdatePhase::Error {
            message: "Target directory is not writable".into(),
            fallback_url: Some(
                "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4".into(),
            ),
        },
        active_sessions: 1,
    };
    for locale in [Locale::ZhCn, Locale::En] {
        let texts = render_update_view(&ctx, &mut errored, locale, 480.0);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Target directory is not writable")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == locale.text("重试", "Retry")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == locale.text("前往 GitHub 手动下载", "Open GitHub Release")),
            "{texts:?}"
        );
    }
}
