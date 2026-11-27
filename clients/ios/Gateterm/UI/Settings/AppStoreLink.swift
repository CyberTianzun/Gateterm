import Foundation

/// App Store listing link for the About section in Settings.
/// Set `appStoreAppID` to the numeric App Store Connect Apple ID once the app is live
/// (for example `"1234567890"`) to enable the "View on App Store" link automatically.
enum AppStoreLink {
    /// Post-launch configuration: replace `nil` with the numeric App Store ID string.
    static let appStoreAppID: String? = nil

    static func url(forAppID appID: String?) -> URL? {
        guard let trimmed = appID?.trimmingCharacters(in: .whitespacesAndNewlines),
              !trimmed.isEmpty,
              trimmed.allSatisfy({ $0.isASCII && $0.isNumber }) else {
            return nil
        }
        return URL(string: "https://apps.apple.com/app/id\(trimmed)")
    }

    static var currentURL: URL? {
        url(forAppID: appStoreAppID)
    }
}
