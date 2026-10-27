//! 托盘图标与菜单。
//!
//! 菜单事件不靠轮询:tray-icon / muda 支持注册全局 handler,我们在 handler 里
//! 把事件转成 winit 的 user event 发进主事件循环,这样所有状态变更都在同一个
//! 线程上串行发生。

use anyhow::{Context, Result};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::event_loop::EventLoopProxy;

use crate::app::{MenuAction, UserEvent};
use crate::i18n::Locale;

pub struct Tray {
    icon: TrayIcon,
    status: MenuItem,
    items: [MenuItem; 5],
}

fn labels(locale: Locale) -> [&'static str; 5] {
    [locale.text("打开 Web 客户端", "Open web client"),
     locale.text("复制访问链接", "Copy access link"),
     locale.text("设置…", "Settings…"),
     locale.text("查看日志…", "View logs…"),
     locale.text("退出", "Quit")]
}

fn decode_png_icon(bytes: &[u8]) -> Result<Icon> {
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
    Icon::from_rgba(buf, info.width, info.height).context("托盘图标构造失败")
}

fn decode_icon() -> Result<Icon> {
    // 图标嵌进二进制,和 server 用 rust-embed 嵌前端是同一个思路:分发只有一个文件。
    // macOS 状态栏使用纯黑 + alpha 的模板图(随亮/暗色菜单栏自动反转);
    // Windows / Linux 使用透明背景的彩色剪影图标。
    #[cfg(target_os = "macos")]
    let bytes = include_bytes!("../assets/tray-macos-template.png");
    #[cfg(not(target_os = "macos"))]
    let bytes = include_bytes!("../assets/icon.png");
    decode_png_icon(bytes)
}

impl Tray {
    pub fn new(proxy: EventLoopProxy<UserEvent>, locale: Locale) -> Result<Self> {
        let status = MenuItem::new(locale.text("gateterm — 启动中…", "gateterm — Starting…"), false, None);
        let [open_web, copy_url, settings, view_logs, quit] = labels(locale).map(|text| MenuItem::new(text, true, None));

        let ids: Vec<(MenuId, MenuAction)> = vec![
            (open_web.id().clone(), MenuAction::OpenWeb),
            (copy_url.id().clone(), MenuAction::CopyUrl),
            (settings.id().clone(), MenuAction::Settings),
            (view_logs.id().clone(), MenuAction::ViewLogs),
            (quit.id().clone(), MenuAction::Quit),
        ];

        let menu = Menu::new();
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
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

        Ok(Self { icon, status, items: [open_web, copy_url, settings, view_logs, quit] })
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
