#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use gateterm_desktop_client::gpui_tokio;
use gateterm_desktop_client::ui::app::{
    CloseActivePane, GatetermWorkbench, NewShellSession, OpenHistoryModal, OpenNewSessionMenu,
    OpenSettingsModal, SplitHorizontal, SplitVertical, ToggleChatView, ToggleSidebar,
    ToggleTerminalSearch, ToggleZoomPane,
};
use gpui::{
    AppContext as _, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::{Root, TitleBar};

fn main() {
    // Subcommand short-circuit: `gateterm-desktop-client hook <url>`
    // runs headless and returns immediately. Agent CLIs invoke our binary on
    // every prompt/response/stop event; launching the GUI or initializing the
    // tray for each hook fire would be catastrophic.
    {
        let mut args = std::env::args().skip(1);
        if args.next().as_deref() == Some("hook") {
            let code = args
                .next()
                .map_or(0, |url| gateterm_server::agent::hook_cli::run(&url));
            std::process::exit(code);
        }
    }

    let _ = gateterm_desktop_client::system::logs::init();
    tracing::info!("gateterm-desktop-client starting");

    let app = gpui_platform::application()
        .with_assets(gateterm_desktop_client::ui::icons::GatetermAssets);

    app.run(move |cx| {
        gpui_tokio::init(cx);
        gpui_component::init(cx);

        // Only bind conflict-free Ctrl+Shift+<Key> (Windows/Linux) and Cmd+<Key> (macOS)
        // workbench shortcuts so bare keys (Tab, Shift+Tab, Escape) and single-modifier
        // terminal/agent chords (Ctrl+B, Ctrl+F, Ctrl+R, Ctrl+T, Ctrl+W, Ctrl+J, Alt+*)
        // are never intercepted before reaching the terminal or coding agent.
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-t", NewShellSession, None),
            KeyBinding::new("cmd-t", NewShellSession, None),
            KeyBinding::new("ctrl-shift-n", OpenNewSessionMenu, None),
            KeyBinding::new("cmd-n", OpenNewSessionMenu, None),
            KeyBinding::new("ctrl-shift-w", CloseActivePane, None),
            KeyBinding::new("cmd-w", CloseActivePane, None),
            KeyBinding::new("ctrl-shift-f", ToggleTerminalSearch, None),
            KeyBinding::new("cmd-f", ToggleTerminalSearch, None),
            KeyBinding::new("ctrl-shift-d", SplitHorizontal, None),
            KeyBinding::new("cmd-d", SplitHorizontal, None),
            KeyBinding::new("ctrl-shift-e", SplitVertical, None),
            KeyBinding::new("cmd-shift-d", SplitVertical, None),
            KeyBinding::new("ctrl-shift-z", ToggleZoomPane, None),
            KeyBinding::new("cmd-shift-enter", ToggleZoomPane, None),
            KeyBinding::new("ctrl-shift-b", ToggleSidebar, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-shift-m", ToggleChatView, None),
            KeyBinding::new("cmd-shift-m", ToggleChatView, None),
            KeyBinding::new("ctrl-shift-h", OpenHistoryModal, None),
            KeyBinding::new("cmd-shift-h", OpenHistoryModal, None),
            KeyBinding::new("cmd-,", OpenSettingsModal, None),
        ]);

        let bounds = Bounds::centered(None, size(px(1360.0), px(860.0)), cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Gateterm".into()),
                ..TitleBar::title_bar_options()
            }),
            app_id: Some("ai.eclosion.gateterm.desktop".into()),
            focus: true,
            show: true,
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                window.activate_window();
                gateterm_desktop_client::system::window_ext::show_and_focus_window(window, cx);
                let workbench = cx.new(|cx| GatetermWorkbench::new(window, cx));
                cx.new(|cx| Root::new(workbench, window, cx))
            })
            .expect("Failed to open Gateterm Desktop window");
            let _ = cx.update(|cx| {
                cx.activate(true);
            });
        })
        .detach();
    });
}
