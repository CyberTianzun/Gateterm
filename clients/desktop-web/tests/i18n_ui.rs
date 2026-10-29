//! Render the real settings panel without creating a window or GPU context.
use gateterm_desktop_web::i18n::Locale;
use gateterm_desktop_web::settings::{
    cloud::{CloudForm, CloudStatus},
    pairing::Pairing,
    state::Form,
    ui::{View, draw},
};

fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => {
            assert!(
                text.pos.x + text.galley.size().x <= 560.5,
                "settings text overflows the window: {}",
                text.galley.text()
            );
            texts.push(text.galley.text().to_owned());
        }
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect_text(shape, texts)),
        _ => {}
    }
}

#[test]
fn settings_switch_language_in_place_and_fit_the_window() {
    let ctx = egui::Context::default();
    gateterm_desktop_web::fonts::install(&ctx);
    let mut view = View {
        form: Form::from_current("127.0.0.1:7683".parse().unwrap(), false),
        pairing: Pairing::new(None, None),
        window_size: "smallest".into(),
        launchers: vec![],
        banner: None,
        message: None,
        cloud: CloudForm::default(),
        cloud_status: CloudStatus::LoggedOut,
        cloud_busy: false,
        cloud_message: None,
    };
    for locale in [Locale::En, Locale::ZhCn, Locale::En] {
        let mut texts = vec![];
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(560.0, 680.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                draw(ui, &mut view, locale);
            });
            // No renderer consumes this in a headless test.
            output.textures_delta.clear();
            texts.clear();
            for shape in &output.shapes {
                collect_text(&shape.shape, &mut texts);
            }
        }
        assert!(
            texts
                .iter()
                .any(|text| text == locale.text("界面语言", "Language")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|text| text == locale.text("监听地址", "Listen address")),
            "{texts:?}"
        );
        if locale == Locale::En {
            assert!(
                !texts
                    .iter()
                    .any(|text| text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))),
                "{texts:?}"
            );
        }
    }
}
