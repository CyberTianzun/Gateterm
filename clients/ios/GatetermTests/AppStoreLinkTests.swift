import XCTest
@testable import Gateterm

final class AppStoreLinkTests: XCTestCase {
    func testNilBlankAndNonNumericAppIDsReturnNil() {
        let invalidIDs: [String?] = [
            nil,
            "",
            "   \n\t ",
            "id1234567890",
            "123-456",
            "123 456",
            "abc",
            "١٢٣٤٥٦",
        ]
        for raw in invalidIDs {
            XCTAssertNil(AppStoreLink.url(forAppID: raw), "Expected nil for \(String(describing: raw))")
        }
    }

    func testValidNumericAppIDBuildsAppStoreURLAndTrimsWhitespace() {
        XCTAssertEqual(
            AppStoreLink.url(forAppID: "1234567890")?.absoluteString,
            "https://apps.apple.com/app/id1234567890"
        )
        XCTAssertEqual(
            AppStoreLink.url(forAppID: "  1234567890 \n")?.absoluteString,
            "https://apps.apple.com/app/id1234567890"
        )
    }

    func testCurrentURLIsNilByDefaultBeforeLaunchConfiguration() {
        XCTAssertNil(AppStoreLink.appStoreAppID)
        XCTAssertNil(AppStoreLink.currentURL)
    }
}
