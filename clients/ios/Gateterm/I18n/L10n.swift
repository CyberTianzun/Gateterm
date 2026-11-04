import Foundation
import Observation
import SwiftUI

/// 用户语言偏好。和 web 对齐:auto 跟随系统,其余钉死一种语言;默认跟随系统。
enum LocalePref: String, CaseIterable, Identifiable {
    case auto, zhCN = "zh-CN", en
    var id: String { rawValue }
    static let `default`: LocalePref = .auto
}

enum AppLocale: String { case zhCN = "zh-CN", en }

func resolveLocale(_ pref: LocalePref, systemTags: [String] = Locale.preferredLanguages) -> AppLocale {
    switch pref {
    case .zhCN: return .zhCN
    case .en: return .en
    case .auto:
        for tag in systemTags {
            let primary = tag.lowercased().split(whereSeparator: { $0 == "-" || $0 == "_" }).first ?? ""
            if primary == "zh" { return .zhCN }
            if primary == "en" { return .en }
        }
        return .en
    }
}

@Observable
private final class LocaleState: @unchecked Sendable {
    var locale: AppLocale
    init(_ locale: AppLocale) { self.locale = locale }
}

/// Runtime-switchable strings. Shared wording comes from the Android resources (see
/// `scripts/gen-strings.py`); `ios` overrides Android-specific sentences and adds iOS-only keys.
enum L {
    private static let prefKey = "gateterm.locale"
    private static let state = LocaleState(resolveLocale(pref))

    static var pref: LocalePref {
        get { LocalePref(rawValue: UserDefaults.standard.string(forKey: prefKey) ?? "") ?? .default }
        set {
            UserDefaults.standard.set(newValue.rawValue, forKey: prefKey)
            state.locale = resolveLocale(newValue)
        }
    }

    static var locale: AppLocale { state.locale }

    @discardableResult
    static func refresh(systemTags: [String] = Locale.preferredLanguages) -> AppLocale {
        let resolved = resolveLocale(pref, systemTags: systemTags)
        if state.locale != resolved { state.locale = resolved }
        return resolved
    }

    static func t(_ key: String) -> String {
        let zh = locale == .zhCN
        if let value = (zh ? ios.zh : ios.en)[key] { return value }
        return (zh ? AndroidStrings.zh : AndroidStrings.en)[key] ?? AndroidStrings.zh[key] ?? key
    }

    static func t(_ key: String, _ args: CVarArg...) -> String {
        String(format: t(key), locale: Locale(identifier: locale.rawValue), arguments: args)
    }

    private static let ios: (zh: [String: String], en: [String: String]) = (
        zh: [
            "editor_warning_loopback": "这个地址指向这台设备自己。请改用电脑的局域网或 Tailscale 地址,不要用 127.0.0.1。",
            "cloud_intro": "可选。登录后,agent 在等你时会推送通知到这台设备。",
            "cloud_browser_hint": "注册、登录和找回密码在系统登录页中完成,成功后自动返回 App。",
            "cloud_browser_unavailable": "无法打开登录页,请稍后重试",
            "cloud_push_missing_play_services": "无法向 Apple 推送服务注册这台设备",
            "cloud_push_unsupported_provider": "Cloud 尚未为 iOS 开启 Apple 推送(APNs),登录不受影响",
            "cloud_push_unavailable": "此构建未启用推送通知",
            "cloud_push_registered": "推送设备已登记(APNs)",
            "connections_background_title": "后台连接",
            "connections_background_hint": "iOS 会在 App 进入后台后很快挂起网络连接。回到前台时会自动重连并恢复会话;agent 提醒请登录 Cloud 账号接收推送。",
            "qr_open_settings": "打开系统设置",
            "session_empty_body": "点右上角 +,从启动项里选一个命令。",
            "servers_empty_body": "点右上角 + 添加服务器。可以扫描 Desktop 设置中的二维码,或粘贴守护进程启动时打印的链接。",
            "term_keys_hint": "轻点呼出键盘。拖动翻看历史,双指捏合调整字号,长按选择文字。",
            "ios_done": "完成",
            "ios_close": "关闭",
            "ios_more": "更多",
            "ios_settings_general": "通用",
            "ios_settings_terminal": "终端",
            "ios_settings_config": "配置",
            "ios_select_server": "选择一台服务器",
            "ios_select_server_body": "从左侧列表选择服务器以查看它的会话。",
            "ios_select_session": "选择一个会话",
            "ios_select_session_body": "从会话列表中打开一个终端,或新建会话。",
            "ios_not_connected": "未连接",
            "ios_connect_hint": "正在连接服务器…",
            "ios_rename": "重命名",
            "ios_open_sessions": "已打开",
            "ios_all_sessions": "全部会话",
            "ios_sessions": "会话",
            "ios_selection_copy": "复制",
            "ios_selection_paste": "粘贴",
            "ios_font_size_step": "字号",
            "ios_close_tab": "关闭标签",
            "ios_disconnect_confirm": "断开与 %@ 的连接?",
            "ios_kill_confirm": "结束会话「%@」?服务器上的进程会被终止。",
            "ios_cloud_login_cancelled": "已取消登录",
            "ios_hardware_keyboard": "已连接外接键盘",
            "ios_scan_permission_denied": "相机权限被拒绝。请在系统设置里允许 Gateterm 使用相机。",
            "ios_scan_unavailable": "这台设备没有可用的相机。",
            "ios_reconnecting_banner": "连接已中断,正在重连…",
            "ios_view_history": "历史会话",
            "ios_chat_view": "聊天",
            "ios_terminal_view": "终端",
        ],
        en: [
            "editor_warning_loopback": "This address points to this device itself. Use the computer's LAN or Tailscale address instead of 127.0.0.1.",
            "cloud_intro": "Optional. Once signed in, this device gets a notification when an agent is waiting for you.",
            "cloud_browser_hint": "Sign up, sign in and password reset happen on the system sign-in page; you return to the app automatically.",
            "cloud_browser_unavailable": "Could not open the sign-in page. Please try again later.",
            "cloud_push_missing_play_services": "Could not register this device with Apple Push Notification service",
            "cloud_push_unsupported_provider": "Cloud has not enabled Apple push (APNs) for iOS yet. Sign-in is unaffected.",
            "cloud_push_unavailable": "Push notifications are not enabled in this build",
            "cloud_push_registered": "Push device registered (APNs)",
            "connections_background_title": "Background connections",
            "connections_background_hint": "iOS suspends network connections soon after the app goes to the background. Gateterm reconnects and restores sessions when you return; sign in to Cloud to get agent alerts as push notifications.",
            "qr_open_settings": "Open Settings",
            "session_empty_body": "Tap + at the top right and pick a launcher.",
            "servers_empty_body": "Tap + at the top right to add a server. Scan the QR code in Desktop settings, or paste the link the daemon printed at startup.",
            "term_keys_hint": "Tap to show the keyboard. Drag to scroll history, pinch to change the font size, long press to select text.",
            "ios_done": "Done",
            "ios_close": "Close",
            "ios_more": "More",
            "ios_settings_general": "General",
            "ios_settings_terminal": "Terminal",
            "ios_settings_config": "Configuration",
            "ios_select_server": "Select a Server",
            "ios_select_server_body": "Choose a server from the sidebar to see its sessions.",
            "ios_select_session": "Select a Session",
            "ios_select_session_body": "Open a terminal from the session list, or create a new session.",
            "ios_not_connected": "Not connected",
            "ios_connect_hint": "Connecting to the server…",
            "ios_rename": "Rename",
            "ios_open_sessions": "Open",
            "ios_all_sessions": "All Sessions",
            "ios_sessions": "Sessions",
            "ios_selection_copy": "Copy",
            "ios_selection_paste": "Paste",
            "ios_font_size_step": "Font Size",
            "ios_close_tab": "Close Tab",
            "ios_disconnect_confirm": "Disconnect from %@?",
            "ios_kill_confirm": "End session “%@”? The process on the server will be terminated.",
            "ios_cloud_login_cancelled": "Sign-in cancelled",
            "ios_hardware_keyboard": "Hardware keyboard connected",
            "ios_scan_permission_denied": "Camera access was denied. Allow Gateterm to use the camera in Settings.",
            "ios_scan_unavailable": "No camera is available on this device.",
            "ios_reconnecting_banner": "Connection lost. Reconnecting…",
            "ios_view_history": "History",
            "ios_chat_view": "Chat",
            "ios_terminal_view": "Terminal",
        ]
    )
}
