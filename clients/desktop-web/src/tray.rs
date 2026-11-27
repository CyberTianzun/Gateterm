//! 托盘图标与菜单。
//!
//! 菜单事件不靠轮询:所有点击都转成 winit 的 user event 发进主事件循环,这样所有
//! 状态变更都在同一个线程上串行发生。
//!
//! - Windows / macOS:tray-icon / muda,注册全局 handler。
//! - Linux:直接走 freedesktop StatusNotifierItem(D-Bus,`ksni`)。tray-icon 在
//!   Linux 上要求 GTK 主循环 + libappindicator,而这里的主循环是 winit;SNI 是
//!   Waybar / KDE / Hyprland 系(Omarchy 等)状态栏原生支持的协议,也不依赖 GTK。

use anyhow::{Context, Result};
use winit::event_loop::EventLoopProxy;

use crate::app::{MenuAction, UserEvent};
use crate::i18n::Locale;

const ACTIONS: [MenuAction; 6] = [
    MenuAction::OpenWeb,
    MenuAction::CopyUrl,
    MenuAction::Settings,
    MenuAction::CheckForUpdates,
    MenuAction::ViewLogs,
    MenuAction::Quit,
];

fn labels(locale: Locale) -> [&'static str; 6] {
    [
        locale.text("打开 Web 客户端", "Open web client"),
        locale.text("复制访问链接", "Copy access link"),
        locale.text("设置…", "Settings…"),
        locale.text("检查更新…", "Check for Updates…"),
        locale.text("查看日志…", "View logs…"),
        locale.text("退出", "Quit"),
    ]
}

fn starting_text(locale: Locale) -> &'static str {
    locale.text("gateterm — 启动中…", "gateterm — Starting…")
}

/// 解码一张 8 位 RGBA PNG,返回 (宽, 高, RGBA 像素)。
fn decode_png_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    // png 0.18 的 Decoder 要 BufRead + Seek,&[u8] 只满足前者,套一层 Cursor
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().context("托盘图标不是合法 PNG")?;
    // 0.18 起 output_buffer_size 返回 Option:尺寸大到 usize 装不下时是 None
    let size = reader.output_buffer_size().context("托盘图标尺寸过大")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).context("托盘图标解码失败")?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight,
        "托盘图标必须是 8 位 RGBA"
    );
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    pub struct Tray {
        icon: TrayIcon,
        status: MenuItem,
        items: [MenuItem; 6],
    }

    fn decode_icon() -> Result<Icon> {
        // 图标嵌进二进制,和 server 用 rust-embed 嵌前端是同一个思路:分发只有一个文件。
        // macOS 状态栏使用纯黑 + alpha 的模板图(随亮/暗色菜单栏自动反转);
        // Windows 使用透明背景的彩色剪影图标。
        #[cfg(target_os = "macos")]
        let bytes = include_bytes!("../assets/tray-macos-template.png");
        #[cfg(not(target_os = "macos"))]
        let bytes = include_bytes!("../assets/icon.png");
        let (width, height, rgba) = decode_png_rgba(bytes)?;
        Icon::from_rgba(rgba, width, height).context("托盘图标构造失败")
    }

    impl Tray {
        pub fn new(proxy: EventLoopProxy<UserEvent>, locale: Locale) -> Result<Self> {
            let status = MenuItem::new(starting_text(locale), false, None);
            let items = labels(locale).map(|text| MenuItem::new(text, true, None));
            let ids: Vec<(MenuId, MenuAction)> =
                items.iter().map(|item| item.id().clone()).zip(ACTIONS).collect();
            let [open_web, copy_url, settings, check_updates, view_logs, quit] = &items;

            let menu = Menu::new();
            menu.append_items(&[
                &status,
                &PredefinedMenuItem::separator(),
                open_web,
                copy_url,
                &PredefinedMenuItem::separator(),
                settings,
                check_updates,
                view_logs,
                &PredefinedMenuItem::separator(),
                quit,
            ])?;

            let builder = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("gateterm")
                .with_icon(decode_icon()?);
            // macOS 的菜单栏会随亮/暗色反转模板图;不设这个,暗色下就是一坨黑块
            #[cfg(target_os = "macos")]
            let builder = builder.with_icon_as_template(true);

            let icon = builder.build().context("无法创建托盘图标")?;

            // handler 是进程级全局的,放在 build() 成功之后再注册:build 失败时不留下
            // 一个指向不存在的托盘的全局 handler。
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some((_, action)) = ids.iter().find(|(id, _)| *id == e.id) {
                    let _ = proxy.send_event(UserEvent::MenuClicked(*action));
                }
            }));

            Ok(Self { icon, status, items })
        }

        pub fn set_language(&mut self, locale: Locale) {
            for (item, text) in self.items.iter().zip(labels(locale)) {
                item.set_text(text);
            }
        }

        /// 状态行 + tooltip 用同一段文字。tooltip 只是设个字符串,比每次会话变化都
        /// 重建整个菜单便宜得多。
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

    /// 状态栏各自挑最合适的尺寸;只给一张大图的话,有的实现会糊、有的会直接不显示。
    const ICON_PNGS: [&[u8]; 5] = [
        include_bytes!("../assets/linux/icon-16.png"),
        include_bytes!("../assets/linux/icon-24.png"),
        include_bytes!("../assets/linux/icon-32.png"),
        include_bytes!("../assets/linux/icon-48.png"),
        include_bytes!("../assets/linux/icon-64.png"),
    ];

    /// SNI 的 pixmap 是 ARGB32、网络字节序,PNG 解出来是 RGBA:每个像素转一下。
    pub(super) fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect()
    }

    fn decode_icons() -> Result<Vec<ksni::Icon>> {
        ICON_PNGS
            .iter()
            .map(|bytes| {
                let (width, height, rgba) = decode_png_rgba(bytes)?;
                Ok(ksni::Icon { width: width as i32, height: height as i32, data: rgba_to_argb(&rgba) })
            })
            .collect()
    }

    /// 交给 ksni 后台线程持有的托盘状态。菜单每次被打开时由 ksni 调 `menu()` 现建,
    /// 所以改文字只需要改这里的字段再 `Handle::update` 一下。
    struct SniTray {
        proxy: EventLoopProxy<UserEvent>,
        icons: Vec<ksni::Icon>,
        status: String,
        labels: [&'static str; 6],
    }

    impl ksni::Tray for SniTray {
        // 和 Windows / macOS 一致:左键也是弹菜单,而不是直接执行某个动作。
        const MENU_ON_ACTIVATE: bool = true;

        fn id(&self) -> String {
            "gateterm-desktop-web".into()
        }

        fn title(&self) -> String {
            "gateterm".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icons.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: "gateterm".into(), description: self.status.clone(), ..Default::default() }
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let item = |index: usize| -> ksni::MenuItem<Self> {
                let action = ACTIONS[index];
                StandardItem {
                    label: self.labels[index].into(),
                    activate: Box::new(move |this: &mut Self| {
                        let _ = this.proxy.send_event(UserEvent::MenuClicked(action));
                    }),
                    ..Default::default()
                }
                .into()
            };
            vec![
                StandardItem { label: self.status.clone(), enabled: false, ..Default::default() }.into(),
                ksni::MenuItem::Separator,
                item(0),
                item(1),
                ksni::MenuItem::Separator,
                item(2),
                item(3),
                item(4),
                ksni::MenuItem::Separator,
                item(5),
            ]
        }
    }

    pub struct Tray {
        handle: Handle<SniTray>,
    }

    impl Tray {
        pub fn new(proxy: EventLoopProxy<UserEvent>, locale: Locale) -> Result<Self> {
            let tray = SniTray {
                proxy,
                icons: decode_icons()?,
                status: starting_text(locale).into(),
                labels: labels(locale),
            };
            // 开机自启时状态栏(StatusNotifierWatcher)可能比我们晚起来:把「暂时没有
            // watcher」当成软错误,等它上线后图标自然会出现,而不是让托盘重试几次就退出。
            // 连 D-Bus session bus 都连不上这种硬错误仍然会返回 Err,走 app 的重试 / 弹框。
            let handle = tray
                .assume_sni_available(true)
                .spawn()
                .context("无法创建托盘图标(StatusNotifierItem)")?;
            Ok(Self { handle })
        }

        pub fn set_language(&mut self, locale: Locale) {
            self.handle.update(|t| t.labels = labels(locale));
        }

        /// 状态行 + tooltip 用同一段文字。
        pub fn set_status(&mut self, text: &str) {
            let text = text.to_owned();
            self.handle.update(move |t| t.status = text);
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            // 把图标从状态栏上撤掉;不等它结束,退出路径上不该被 D-Bus 卡住。
            let _ = self.handle.shutdown();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
compile_error!("gateterm-desktop-web 只支持 Windows、macOS 与 Linux");

pub use imp::Tray;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_label_has_an_action() {
        assert_eq!(labels(Locale::En).len(), ACTIONS.len());
        assert_eq!(labels(Locale::ZhCn).len(), ACTIONS.len());
        assert!(ACTIONS.contains(&MenuAction::CheckForUpdates));
        for label in labels(Locale::En) {
            assert!(!label.is_empty());
            assert!(
                !label.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "English tray label leaked Chinese: {label}"
            );
        }
    }

    #[test]
    fn bundled_tray_icon_decodes_as_rgba() {
        let (w, h, rgba) = decode_png_rgba(include_bytes!("../assets/icon.png")).unwrap();
        assert_eq!(rgba.len(), (w * h * 4) as usize);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sni_pixmap_is_argb_in_network_order() {
        assert_eq!(imp::rgba_to_argb(&[1, 2, 3, 4, 5, 6, 7, 8]), vec![4, 1, 2, 3, 8, 5, 6, 7]);
    }
}
