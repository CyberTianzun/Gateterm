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
use gateterm_server::agent::{
    AgentStatus, Confidence, Detected, Integration,
    ops::{AgentPlanSummary, PlanGroup},
};

fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => {
            assert!(
                text.pos.x + text.galley.size().x <= 600.5,
                "settings text overflows the window (x={}, width={}): {}",
                text.pos.x,
                text.galley.size().x,
                text.galley.text()
            );
            texts.push(text.galley.text().to_owned());
        }
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect_text(shape, texts)),
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
            collect_text(&shape.shape, &mut texts);
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
