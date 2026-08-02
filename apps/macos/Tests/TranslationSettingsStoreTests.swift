@testable import MeetingRaft
import XCTest

@MainActor
final class TranslationSettingsStoreTests: XCTestCase {
    func testDefaultBackendIsAuto() {
        let store = TranslationSettingsStore()
        XCTAssertEqual(store.backend, .auto)
        XCTAssertFalse(store.enabled)
    }

    func testBackendsIncludeAppleAndBackend() {
        let store = TranslationSettingsStore()
        XCTAssertTrue(store.backends.contains(.apple))
        XCTAssertTrue(store.backends.contains(.backend))
        XCTAssertTrue(store.backends.contains(.localLlm))
    }
}
