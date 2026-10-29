//! Platform window visibility helpers for hiding to system tray on close (`X`)
//! and restoring/focusing the window from the system tray.

use gpui::{App, Window};

#[cfg(target_os = "windows")]
mod win32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
        pub fn SetForegroundWindow(hwnd: isize) -> i32;
        pub fn IsIconic(hwnd: isize) -> i32;
    }

    pub const SW_HIDE: i32 = 0;
    pub const SW_SHOW: i32 = 5;
    pub const SW_RESTORE: i32 = 9;
}

pub fn hide_window_to_tray(window: &mut Window, cx: &mut App) {
    let _ = cx;
    #[cfg(target_os = "windows")]
    if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window)
        && let raw_window_handle::RawWindowHandle::Win32(win32_handle) = handle.as_raw()
    {
        unsafe {
            win32::ShowWindow(win32_handle.hwnd.get(), win32::SW_HIDE);
        }
    }

    #[cfg(target_os = "macos")]
    {
        cx.hide();
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = cx;
        window.minimize_window();
    }
}

pub fn show_and_focus_window(window: &mut Window, cx: &mut App) {
    #[cfg(target_os = "windows")]
    if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window)
        && let raw_window_handle::RawWindowHandle::Win32(win32_handle) = handle.as_raw()
    {
        let hwnd = win32_handle.hwnd.get();
        unsafe {
            if win32::IsIconic(hwnd) != 0 {
                win32::ShowWindow(hwnd, win32::SW_RESTORE);
            } else {
                win32::ShowWindow(hwnd, win32::SW_SHOW);
            }
            win32::SetForegroundWindow(hwnd);
        }
    }

    #[cfg(target_os = "macos")]
    {
        cx.activate(true);
    }

    let _ = cx;
    window.activate_window();
}
