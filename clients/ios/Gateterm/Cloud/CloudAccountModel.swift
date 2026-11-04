import AuthenticationServices
import CryptoKit
import Foundation
import Observation
import UIKit
import UserNotifications

enum RelayRegion: String, CaseIterable, Identifiable {
    case china, northAmerica, global
    var id: String { rawValue }

    var url: String {
        switch self {
        case .china: "https://gateterm.signal.top/"
        case .northAmerica: "https://gateterm.eclosion.ai/"
        case .global: "https://global.gateterm.eclosion.ai/"
        }
    }

    var label: String {
        switch self {
        case .china: L.t("cloud_region_china")
        case .northAmerica: L.t("cloud_region_north_america")
        case .global: L.t("cloud_region_global")
        }
    }

    var deployed: Bool { self == .china }

    static func from(url: String) -> RelayRegion {
        let trimmed = url.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        return allCases.first { $0.url.trimmingCharacters(in: CharacterSet(charactersIn: "/")) == trimmed } ?? .china
    }
}

enum PushRegistration: Equatable {
    case idle, registering, registered, unavailable, deviceTokenFailed, timedOut, cloudError, disabled, unsupportedProvider

    var registered: Bool { self == .registered }

    var text: String {
        switch self {
        case .idle: L.t("cloud_push_not_registered")
        case .registering: L.t("cloud_push_registering")
        case .registered: L.t("cloud_push_registered")
        case .unavailable: L.t("cloud_push_unavailable")
        case .deviceTokenFailed: L.t("cloud_push_missing_play_services")
        case .timedOut: L.t("cloud_push_timeout")
        case .cloudError: L.t("cloud_push_cloud_error")
        case .disabled: L.t("cloud_push_disabled")
        case .unsupportedProvider: L.t("cloud_push_unsupported_provider")
        }
    }
}

/// The browser sign-in reuses the redirect URI Cloud already accepts for the mobile apps;
/// `ASWebAuthenticationSession` intercepts it, so no URL scheme needs to be registered.
private let callbackScheme = "ai.eclosion.gateterm.android"
private let callbackURI = "\(callbackScheme)://cloud/auth/callback"

struct CloudMessage: Equatable {
    let ok: Bool
    let text: String
}

/// Cloud account (optional): browser sign-in with PKCE, logout, account deletion and APNs registration.
/// The long-lived token is kept by the Rust core in `Application Support/cloud.json`.
@MainActor
@Observable
final class CloudAccountModel: NSObject {
    private(set) var status: CloudStatus?
    var region: RelayRegion = .china
    var password = ""
    var code = ""
    var confirmDelete = false
    private(set) var busy = false
    private(set) var waitingForBrowser = false
    private(set) var message: CloudMessage?
    private(set) var push: PushRegistration = .idle
    private(set) var notificationsAllowed = true

    @ObservationIgnored private var accountStorage: CloudAccount?
    @ObservationIgnored private var authSession: ASWebAuthenticationSession?
    @ObservationIgnored private var tokenWaiters: [CheckedContinuation<Data?, Never>] = []
    @ObservationIgnored private var deviceToken: Data?

    private static var credentialURL: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var url = dir.appendingPathComponent("cloud.json")
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
        return url
    }

    /// Never load the native account for users who have never signed in.
    private var account: CloudAccount {
        if let accountStorage { return accountStorage }
        let created = CloudAccount(credPath: Self.credentialURL.path, label: UIDevice.current.model)
        accountStorage = created
        return created
    }

    override init() {
        super.init()
        refreshStatus()
        if let status { region = .from(url: status.url) }
    }

    func refreshStatus() {
        guard !busy else { return }
        let signedIn = accountStorage != nil || FileManager.default.fileExists(atPath: Self.credentialURL.path)
        let next = signedIn ? account.status() : nil
        if next == nil && status != nil { password = ""; code = ""; confirmDelete = false }
        status = next
        UNUserNotificationCenter.current().getNotificationSettings { settings in
            let allowed = settings.authorizationStatus != .denied
            Task { @MainActor in self.notificationsAllowed = allowed }
        }
    }

    // MARK: Browser sign-in

    func login() {
        guard !busy, !waitingForBrowser, status == nil else { return }
        let verifier = Self.nonce(64), state = Self.nonce(32)
        let label = String(UIDevice.current.name.prefix(64))
        var components = URLComponents(string: region.url + "auth/mobile")!
        components.queryItems = [
            .init(name: "redirect_uri", value: callbackURI), .init(name: "state", value: state),
            .init(name: "code_challenge", value: Self.challenge(verifier)), .init(name: "code_challenge_method", value: "S256"),
            .init(name: "label", value: label.isEmpty ? "iOS" : label), .init(name: "lang", value: L.locale.rawValue),
        ]
        guard let url = components.url else { return }
        let regionURL = region.url
        let session = ASWebAuthenticationSession(url: url, callbackURLScheme: callbackScheme) { [weak self] callback, error in
            Task { @MainActor in
                guard let self else { return }
                self.waitingForBrowser = false
                self.authSession = nil
                if let error = error as? ASWebAuthenticationSessionError, error.code == .canceledLogin {
                    self.message = CloudMessage(ok: true, text: L.t("cloud_login_cancelled"))
                    return
                }
                guard let callback, let code = Self.callbackCode(callback, expectedState: state) else {
                    self.message = CloudMessage(ok: false, text: L.t(error == nil ? "cloud_err_authorization_code" : "cloud_browser_unavailable"))
                    return
                }
                await self.finish(regionURL: regionURL, code: code, verifier: verifier)
            }
        }
        session.presentationContextProvider = self
        session.prefersEphemeralWebBrowserSession = false
        authSession = session
        message = nil
        waitingForBrowser = session.start()
        if !waitingForBrowser { message = CloudMessage(ok: false, text: L.t("cloud_browser_unavailable")) }
    }

    func cancelLogin() {
        authSession?.cancel()
        authSession = nil
        waitingForBrowser = false
        message = CloudMessage(ok: true, text: L.t("cloud_login_cancelled"))
    }

    private func finish(regionURL: String, code: String, verifier: String) async {
        busy = true
        defer { busy = false }
        do {
            _ = try await account.finishBrowserLogin(url: regionURL, code: code, verifier: verifier, redirectUri: callbackURI)
            status = account.status()
            message = CloudMessage(ok: true, text: L.t("cloud_ok_logged_in"))
            await registerPush(askPermission: true)
        } catch {
            message = CloudMessage(ok: false, text: Self.failureText(error))
        }
        status = account.status()
    }

    static func callbackCode(_ url: URL, expectedState: String) -> String? {
        guard url.scheme == callbackScheme, url.host == "cloud", url.path == "/auth/callback", url.port == nil, url.user == nil,
              url.fragment == nil, let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
              Set(items.map(\.name)).count == items.count,
              let state = items.first(where: { $0.name == "state" })?.value,
              Data(state.utf8) == Data(expectedState.utf8),
              let code = items.first(where: { $0.name == "code" })?.value, !code.isEmpty, code.count <= 256 else { return nil }
        return code
    }

    private static func nonce(_ size: Int) -> String {
        var bytes = [UInt8](repeating: 0, count: size)
        _ = SecRandomCopyBytes(kSecRandomDefault, size, &bytes)
        return base64URL(Data(bytes))
    }

    static func challenge(_ verifier: String) -> String { base64URL(Data(SHA256.hash(data: Data(verifier.utf8)))) }

    private static func base64URL(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    // MARK: Account operations

    func logout() {
        run { model in
            let notified = try await model.account.logout()
            model.push = .idle
            return L.t(notified ? "cloud_ok_logged_out" : "cloud_ok_logged_out_local")
        }
    }

    func sendDeleteCode() {
        run { model in L.t("cloud_ok_delete_code_sent", try await model.account.requestDeleteCode()) }
    }

    func deleteAccount() {
        guard confirmDelete else { message = CloudMessage(ok: false, text: L.t("cloud_form_confirm_delete")); return }
        guard !password.isEmpty else { message = CloudMessage(ok: false, text: L.t("cloud_form_need_password")); return }
        let digits = code.trimmingCharacters(in: .whitespaces)
        guard digits.count == 6, digits.allSatisfy(\.isASCII), digits.allSatisfy(\.isNumber) else {
            message = CloudMessage(ok: false, text: L.t("cloud_form_bad_code")); return
        }
        let password = password
        run { model in
            try await model.account.deleteAccount(password: password, code: digits)
            model.push = .idle
            return L.t("cloud_ok_deleted")
        }
    }

    private func run(_ action: @escaping (CloudAccountModel) async throws -> String) {
        guard !busy else { return }
        busy = true
        message = nil
        Task {
            do {
                let text = try await action(self)
                message = CloudMessage(ok: true, text: text)
                password = ""; code = ""; confirmDelete = false
            } catch {
                message = CloudMessage(ok: false, text: Self.failureText(error))
            }
            busy = false
            refreshStatus()
        }
    }

    static func failureText(_ error: Error) -> String {
        guard let error = error as? CloudFfiError else { return L.t("cloud_err_generic") }
        switch error {
        case .NotLoggedIn: return L.t("cloud_err_not_logged_in")
        case .Unauthorized: return L.t("cloud_err_unauthorized")
        case .RateLimited: return L.t("cloud_err_rate_limited")
        case .Unreachable: return L.t("cloud_err_unreachable")
        case .Local: return L.t("cloud_err_local")
        case let .Api(code):
            let keys = ["invalid_credentials": "cloud_err_invalid_credentials", "invalid_authorization_code": "cloud_err_authorization_code",
                        "invalid_code": "cloud_err_invalid_code", "code_exhausted": "cloud_err_code_exhausted",
                        "email_taken": "cloud_err_email_taken", "weak_password": "cloud_err_weak_password",
                        "invalid_email": "cloud_err_invalid_email", "wrong_password": "cloud_err_wrong_password"]
            return L.t(keys[code] ?? "cloud_err_generic")
        }
    }

    // MARK: Push (APNs)

    /// Called at launch and after sign-in. Cloud decides the provider; iOS can only deliver via APNs.
    func registerPush(askPermission: Bool = false) async {
        guard status != nil, push != .registering else { return }
        push = .registering
        do {
            let config = try await account.pushConfig()
            switch config.platform {
            case "none":
                push = .disabled
                return
            case "apns":
                break
            default:
                push = .unsupportedProvider
                return
            }
            if askPermission {
                _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge])
            }
            guard let token = await requestDeviceToken() else {
                push = .deviceTokenFailed
                return
            }
            try await account.registerPushTarget(platform: "apns", token: token.map { String(format: "%02x", $0) }.joined())
            push = .registered
        } catch {
            push = .cloudError
        }
        refreshStatus()
    }

    private func requestDeviceToken() async -> Data? {
        if let deviceToken { return deviceToken }
        return await withCheckedContinuation { continuation in
            tokenWaiters.append(continuation)
            UIApplication.shared.registerForRemoteNotifications()
            Task {
                try? await Task.sleep(for: .seconds(15))
                self.resolveToken(nil)
            }
        }
    }

    func didRegister(deviceToken: Data?) {
        if let deviceToken { self.deviceToken = deviceToken }
        resolveToken(deviceToken)
        if deviceToken != nil && status != nil && push == .idle { Task { await registerPush() } }
    }

    private func resolveToken(_ token: Data?) {
        let waiters = tokenWaiters
        tokenWaiters.removeAll()
        waiters.forEach { $0.resume(returning: token) }
    }
}

extension CloudAccountModel: ASWebAuthenticationPresentationContextProviding {
    nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        MainActor.assumeIsolated {
            UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
                .flatMap(\.windows).first { $0.isKeyWindow } ?? ASPresentationAnchor()
        }
    }
}
