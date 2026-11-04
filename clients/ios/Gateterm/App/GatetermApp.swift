import SwiftUI
import UIKit
import UserNotifications

@main
struct GatetermApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(delegate.model)
                .environment(delegate.cloud)
                .onChange(of: scenePhase) { _, phase in
                    // `.inactive` (Control Center, app switcher peek) keeps terminals attached.
                    switch phase {
                    case .active:
                        delegate.model.setForeground(true)
                        delegate.cloud.refreshStatus()
                    case .background:
                        delegate.model.setForeground(false)
                    default:
                        break
                    }
                }
        }
    }
}

@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    let model = AppModel()
    let cloud = CloudAccountModel()

    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        TermFonts.registerBundledFonts()
        UNUserNotificationCenter.current().delegate = self
        model.restoreDesiredConnections()
        if cloud.status != nil { Task { await cloud.registerPush() } }
        #if DEBUG
        seedForDevelopment()
        #endif
        return true
    }

    #if DEBUG
    /// Development only: `-GatetermSeedURL <access URL>` adds and connects a server, and
    /// `-GatetermOpenSession YES` opens (or creates) a session, so simulator runs need no typing.
    private func seedForDevelopment() {
        let defaults = UserDefaults.standard
        guard let url = defaults.string(forKey: "GatetermSeedURL") else { return }
        if !model.connections.contains(where: { $0.url == url }) {
            model.startAdd(prefill: url)
            model.editor?.name = "Dev"
            model.saveEditor()
        }
        guard let id = model.connections.first(where: { $0.url == url })?.id else { return }
        model.connect(id)
        guard defaults.bool(forKey: "GatetermOpenSession") else { return }
        Task {
            for _ in 0..<50 where model.status(id) != .connected { try? await Task.sleep(for: .milliseconds(100)) }
            try? await Task.sleep(for: .milliseconds(300))
            if let session = model.sessions.first { model.openSession(session.id) } else { model.newSession(.fallback) }
        }
    }
    #endif

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        cloud.didRegister(deviceToken: deviceToken)
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        cloud.didRegister(deviceToken: nil)
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async
        -> UNNotificationPresentationOptions {
        [.banner, .list, .sound]
    }

    /// Tapping an agent alert opens that server and session (the server must have been connected once).
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let push = AgentPush.parse(info) else { return }
        await MainActor.run {
            model.openFromNotification(cloudServerId: push.serverId, session: push.session, agentId: push.agentId)
        }
    }
}
