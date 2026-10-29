#[cfg(target_os = "macos")]
#[test]
fn the_plist_names_the_executable_and_runs_at_load() {
    let xml = gateterm_desktop_web::autostart::plist_xml("/Applications/gateterm.app/Contents/MacOS/gateterm-desktop-web");

    assert!(xml.contains("<key>Label</key>"));
    assert!(xml.contains("com.gateterm.desktop"));
    assert!(xml.contains("/Applications/gateterm.app/Contents/MacOS/gateterm-desktop-web"));
    assert!(xml.contains("<key>RunAtLoad</key>"));
    assert!(xml.contains("<true/>"));
    // LaunchAgent 只该在登录时拉起一次,不该被当成需要常活的守护
    assert!(!xml.contains("KeepAlive"));
}

#[cfg(target_os = "macos")]
#[test]
fn a_path_with_an_ampersand_is_escaped() {
    let xml = gateterm_desktop_web::autostart::plist_xml("/Users/a&b/gateterm-desktop-web");
    assert!(xml.contains("/Users/a&amp;b/gateterm-desktop-web"), "路径没做 XML 转义:\n{xml}");
    assert!(!xml.contains("a&b"));
}

// 不写「启用→读回→禁用」的往返测试:它会真的改动当前用户的登录项
// (~/Library/LaunchAgents 或 HKCU Run),测试中途 panic 就会把 gateterm 留在
// 开机启动里。`is_enabled` / `set` 的真实读写由 Task 8 的手动验收覆盖。
