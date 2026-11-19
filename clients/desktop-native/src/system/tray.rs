//! System tray icon and menu for `clients/desktop-native`, aligned with `clients/desktop-web/src/tray.rs`
//! plus a "Show Main Window" action and left-click tray restoration.
//!
//! Windows / macOS use `tray-icon`; Linux talks freedesktop StatusNotifierItem over D-Bus via
//! `ksni` (no GTK main loop required, natively supported by Waybar / KDE / Hyprland setups such
//! as Omarchy).

use anyhow::{Context, Result};
use tokio::sync::mpsc;

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

const ACTIONS: [TrayAction; 6] = [
    TrayAction::ShowWindow,
    TrayAction::OpenWeb,
    TrayAction::CopyUrl,
    TrayAction::OpenSettings,
    TrayAction::ViewLogs,
    TrayAction::Quit,
];

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

fn starting_text(locale: Locale) -> &'static str {
    locale.t("gateterm — 启动中…", "gateterm — Starting…")
}

/// Decodes an 8-bit RGBA PNG into `(width, height, rgba)`.
fn decode_png_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
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
    Ok((info.width, info.height, buf))
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    pub struct SystemTray {
        icon: TrayIcon,
        status: MenuItem,
        items: [MenuItem; 6],
    }

    fn decode_icon() -> Result<Icon> {
        #[cfg(target_os = "macos")]
        let bytes = include_bytes!("../../../desktop-web/assets/tray-macos-template.png");
        #[cfg(not(target_os = "macos"))]
        let bytes = include_bytes!("../../../desktop-web/assets/icon.png");
        let (width, height, rgba) = decode_png_rgba(bytes)?;
        Icon::from_rgba(rgba, width, height).context("Failed to build tray icon")
    }

    impl SystemTray {
        pub fn new(tx: mpsc::UnboundedSender<TrayAction>, locale: Locale) -> Result<Self> {
            let status = MenuItem::new(starting_text(locale), false, None);
            let items = labels(locale).map(|text| MenuItem::new(text, true, None));
            let ids: Vec<(MenuId, TrayAction)> = items
                .iter()
                .map(|item| item.id().clone())
                .zip(ACTIONS)
                .collect();
            let [show_win, open_web, copy_url, settings, view_logs, quit] = &items;

            let menu = Menu::new();
            menu.append_items(&[
                &status,
                &PredefinedMenuItem::separator(),
                show_win,
                open_web,
                copy_url,
                &PredefinedMenuItem::separator(),
                settings,
                view_logs,
                &PredefinedMenuItem::separator(),
                quit,
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
                items,
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
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::StandardItem;

    /// Status bars pick the best size; offering a single large pixmap renders blurry or not at all
    /// on some hosts.
    const ICON_PNGS: [&[u8]; 5] = [
        include_bytes!("../../../desktop-web/assets/linux/icon-16.png"),
        include_bytes!("../../../desktop-web/assets/linux/icon-24.png"),
        include_bytes!("../../../desktop-web/assets/linux/icon-32.png"),
        include_bytes!("../../../desktop-web/assets/linux/icon-48.png"),
        include_bytes!("../../../desktop-web/assets/linux/icon-64.png"),
    ];

    /// SNI pixmaps are ARGB32 in network byte order; PNG decodes to RGBA.
    pub(super) fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| [a, r, g, b])
            .collect()
    }

    fn decode_icons() -> Result<Vec<ksni::Icon>> {
        ICON_PNGS
            .iter()
            .map(|bytes| {
                let (width, height, rgba) = decode_png_rgba(bytes)?;
                Ok(ksni::Icon {
                    width: width as i32,
                    height: height as i32,
                    data: rgba_to_argb(&rgba),
                })
            })
            .collect()
    }

    struct SniTray {
        tx: mpsc::UnboundedSender<TrayAction>,
        icons: Vec<ksni::Icon>,
        status: String,
        labels: [&'static str; 6],
    }

    impl ksni::Tray for SniTray {
        fn id(&self) -> String {
            "gateterm-desktop-native".into()
        }

        fn title(&self) -> String {
            "Gateterm".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icons.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Gateterm".into(),
                description: self.status.clone(),
                ..Default::default()
            }
        }

        /// Left click restores the main window, matching Windows / macOS.
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayAction::ShowWindow);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let item = |index: usize| -> ksni::MenuItem<Self> {
                let action = ACTIONS[index];
                StandardItem {
                    label: self.labels[index].into(),
                    activate: Box::new(move |this: &mut Self| {
                        let _ = this.tx.send(action);
                    }),
                    ..Default::default()
                }
                .into()
            };
            vec![
                StandardItem {
                    label: self.status.clone(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                item(0),
                item(1),
                item(2),
                ksni::MenuItem::Separator,
                item(3),
                item(4),
                ksni::MenuItem::Separator,
                item(5),
            ]
        }
    }

    pub struct SystemTray {
        handle: Handle<SniTray>,
    }

    impl SystemTray {
        pub fn new(tx: mpsc::UnboundedSender<TrayAction>, locale: Locale) -> Result<Self> {
            let tray = SniTray {
                tx,
                icons: decode_icons()?,
                status: starting_text(locale).into(),
                labels: labels(locale),
            };
            // At login the StatusNotifierWatcher (status bar) may come up after us; treat a missing
            // watcher as a soft error so the icon appears once it is online.
            let handle = tray
                .assume_sni_available(true)
                .spawn()
                .context("Failed to create tray icon (StatusNotifierItem)")?;
            Ok(Self { handle })
        }

        pub fn set_language(&mut self, locale: Locale) {
            self.handle.update(|t| t.labels = labels(locale));
        }

        pub fn set_status(&mut self, text: &str) {
            let text = text.to_owned();
            self.handle.update(move |t| t.status = text);
        }
    }

    impl Drop for SystemTray {
        fn drop(&mut self) {
            let _ = self.handle.shutdown();
        }
    }
}

pub use imp::SystemTray;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_label_has_an_action() {
        assert_eq!(labels(Locale::En).len(), ACTIONS.len());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sni_pixmap_is_argb_in_network_order() {
        assert_eq!(
            imp::rgba_to_argb(&[1, 2, 3, 4, 5, 6, 7, 8]),
            vec![4, 1, 2, 3, 8, 5, 6, 7]
        );
    }
}
