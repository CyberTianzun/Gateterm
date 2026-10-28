#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use gateterm_desktop_client::gpui_tokio;
use gateterm_desktop_client::ui::app::{
    CloseActivePane, FocusNextPane, GatetermWorkbench, NewShellSession, OpenHistoryModal,
    OpenNewSessionMenu, OpenSettingsModal, SplitHorizontal, SplitVertical, ToggleChatView,
    ToggleSidebar, ToggleTerminalSearch, ToggleZoomPane,
};
use gpui::{AppContext as _, Bounds, KeyBinding, WindowBounds, WindowOptions, px, size};
use gpui_component::{Root, TitleBar};

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);

    app.run(move |cx| {
        gpui_tokio::init(cx);
        gpui_component::init(cx);

        cx.bind_keys([
            KeyBinding::new("ctrl-shift-t", NewShellSession, None),
            KeyBinding::new("cmd-t", NewShellSession, None),
            KeyBinding::new("ctrl-shift-n", OpenNewSessionMenu, None),
            KeyBinding::new("cmd-n", OpenNewSessionMenu, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-f", ToggleTerminalSearch, None),
            KeyBinding::new("cmd-f", ToggleTerminalSearch, None),
            KeyBinding::new("ctrl-shift-f", ToggleTerminalSearch, None),
            KeyBinding::new("ctrl-shift-d", SplitHorizontal, None),
            KeyBinding::new("cmd-d", SplitHorizontal, None),
            KeyBinding::new("ctrl-shift-e", SplitVertical, None),
            KeyBinding::new("cmd-shift-d", SplitVertical, None),
            KeyBinding::new("ctrl-shift-z", ToggleZoomPane, None),
            KeyBinding::new("cmd-shift-enter", ToggleZoomPane, None),
            KeyBinding::new("ctrl-tab", FocusNextPane, None),
            KeyBinding::new("ctrl-shift-m", ToggleChatView, None),
            KeyBinding::new("ctrl-shift-h", OpenHistoryModal, None),
            KeyBinding::new("ctrl-,", OpenSettingsModal, None),
            KeyBinding::new("cmd-,", OpenSettingsModal, None),
            KeyBinding::new("ctrl-shift-w", CloseActivePane, None),
        ]);

        let bounds = Bounds::centered(None, size(px(1360.0), px(860.0)), cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitleBar::title_bar_options()),
            focus: true,
            show: true,
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let workbench = cx.new(|cx| GatetermWorkbench::new(window, cx));
                cx.new(|cx| Root::new(workbench, window, cx))
            })
            .expect("Failed to open Gateterm Desktop window");
        })
        .detach();
    });
}
