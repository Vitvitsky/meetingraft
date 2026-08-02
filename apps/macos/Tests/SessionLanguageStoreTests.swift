import XCTest
@testable import MeetingRaft

final class SessionLanguageStoreTests: XCTestCase {
    func testDefaultPrimaryIsRussian() {
        let store = SessionLanguageStore()
        XCTAssertEqual(store.primary, .ru)
    }

    func testAllowedLanguagesAreRuEnEs() {
        let store = SessionLanguageStore()
        XCTAssertEqual(store.allowed, [.ru, .en, .es])
    }
}
