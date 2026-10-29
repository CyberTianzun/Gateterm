//! System tray icon and menu for `clients/gateterm-desktop-native`, aligned with `clients/gateterm-desktop-web/src/tray.rs`
//! plus a "Show Main Window" action and left-click tray restoration.

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::i18n::Locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ShowWindow,
    OpenWeb,
    CopyUrl,
    OpenSettings,
    ViewLogs,
    Quit,
}

pub struct SystemTray {
    icon: TrayIcon,
    status: MenuItem,
    items: [MenuItem; 6],
}

fn labels(locale: Locale) -> [&'static str; 6] {
    [
        locale.t("显示主窗口", "Show main window"),
        locale.t("打开 Web 客户端", "Open web client"),
        locale.t("复制访问链接", "Copy access link"),
        locale.t("设置…", "Settings…"),
        locale.t("查看日志…", "View logs…"),
        locale.t("退出", "Quit"),
    ]
}

fn decode_png_icon(bytes: &[u8]) -> Result<Icon> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .context("Tray icon is not a valid PNG")?;
    let size = reader
        .output_buffer_size()
        .context("Tray icon buffer size overflow")?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .context("Failed to decode tray icon PNG")?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight,
        "Tray icon must be 8-bit RGBA"
    );
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).context("Failed to build tray icon")
}

fn decode_icon() -> Result<Icon> {
    #[cfg(target_os = "macos")]
    let bytes = include_bytes!("../../../gateterm-desktop-web/assets/tray-macos-template.png");
    #[cfg(not(target_os = "macos"))]
    let bytes = include_bytes!("../../../gateterm-desktop-web/assets/icon.png");
    decode_png_icon(bytes)
}

impl SystemTray {
    pub fn new(tx: mpsc::UnboundedSender<TrayAction>, locale: Locale) -> Result<Self> {
        let status = MenuItem::new(
            locale.t("gateterm — 启动中…", "gateterm — Starting…"),
            false,
            None,
        );
        let [show_win, open_web, copy_url, settings, view_logs, quit] =
            labels(locale).map(|text| MenuItem::new(text, true, None));

        let ids: Vec<(MenuId, TrayAction)> = vec![
            (show_win.id().clone(), TrayAction::ShowWindow),
            (open_web.id().clone(), TrayAction::OpenWeb),
            (copy_url.id().clone(), TrayAction::CopyUrl),
            (settings.id().clone(), TrayAction::OpenSettings),
            (view_logs.id().clone(), TrayAction::ViewLogs),
            (quit.id().clone(), TrayAction::Quit),
        ];

        let menu = Menu::new();
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &show_win,
            &open_web,
            &copy_url,
            &PredefinedMenuItem::separator(),
            &settings,
            &view_logs,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Gateterm")
            .with_icon(decode_icon()?);
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_as_template(true);

        let icon = builder.build().context("Failed to create tray icon")?;

        let tx_menu = tx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, action)) = ids.iter().find(|(id, _)| *id == e.id) {
                let _ = tx_menu.send(*action);
            }
        }));

        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                let _ = tx.send(TrayAction::ShowWindow);
            }
            _ => {}
        }));

        Ok(Self {
            icon,
            status,
            items: [show_win, open_web, copy_url, settings, view_logs, quit],
        })
    }

    pub fn set_language(&mut self, locale: Locale) {
        for (item, text) in self.items.iter().zip(labels(locale)) {
            item.set_text(text);
        }
    }

    pub fn set_status(&mut self, text: &str) {
        self.status.set_text(text);
        let _ = self.icon.set_tooltip(Some(text));
    }
}
