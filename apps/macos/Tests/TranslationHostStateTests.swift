@testable import MeetingRaft
import XCTest

final class TranslationHostStateTests: XCTestCase {
    /// Ключ сверяем с тем же ключом, а не с обрывком текста: обрывок
    /// ломается от перевода и от языка машины.
    func testNeedsDownloadExplainsItself() {
        XCTAssertEqual(
            TranslationHostState.needsDownload.columnMessage,
            String(localized: "Apple translation needs a language download")
        )
    }

    func testUnsupportedExplainsItself() {
        XCTAssertEqual(
            TranslationHostState.unsupported.columnMessage,
            String(localized: "This language pair is not supported")
        )
    }

    func testDownloadingExplainsItself() {
        XCTAssertEqual(
            TranslationHostState.downloading.columnMessage,
            String(localized: "Downloading language…")
        )
    }

    func testFailureShowsItsMessage() {
        XCTAssertEqual(
            TranslationHostState.failed("boom").columnMessage,
            "boom"
        )
    }

    /// Готовая пара и выключенный перевод не пишут ничего: колонка
    /// показывает обычную заглушку.
    func testReadyAndOffStaySilent() {
        XCTAssertNil(TranslationHostState.ready.columnMessage)
        XCTAssertNil(TranslationHostState.off.columnMessage)
    }

    func testDownloadOfferedOnlyWhenItCanHelp() {
        XCTAssertTrue(TranslationHostState.needsDownload.canDownload)
        XCTAssertTrue(TranslationHostState.failed("boom").canDownload)
        XCTAssertFalse(TranslationHostState.ready.canDownload)
        XCTAssertFalse(TranslationHostState.unsupported.canDownload)
        XCTAssertFalse(TranslationHostState.downloading.canDownload)
        XCTAssertFalse(TranslationHostState.off.canDownload)
    }
}
