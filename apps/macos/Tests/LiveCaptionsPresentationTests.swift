@testable import MeetingRaft
import XCTest

/// Экран показывает речь, а не журнал: проверяем то, что решает это
/// правило, — выборку последних строк.
@MainActor
final class LiveCaptionsPresentationTests: XCTestCase {
    private func makeViewModel() -> LiveCaptionsViewModel {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("meetingraft-live-\(UUID().uuidString)")
        return LiveCaptionsViewModel(core: MeetingCore.withDataRoot(dataRoot: root.path))
    }

    func testRecentLinesKeepsOnlyTheTail() {
        let viewModel = makeViewModel()
        for index in 0 ..< 10 {
            viewModel.ingestForTesting(
                text: "строка \(index)",
                phase: index == 9 ? .partial : .final
            )
        }

        let recent = viewModel.recentLines(limit: 3)

        XCTAssertEqual(recent.count, 3)
        XCTAssertEqual(recent.last?.text, "строка 9")
    }

    func testRecentLinesOnEmptyFeedIsEmpty() {
        XCTAssertTrue(makeViewModel().recentLines().isEmpty)
    }

    /// Лента экрана длиннее трёх строк (Epic 17): буфер хранит всё,
    /// а показывается хвост с экранного предела — не три строки.
    func testFeedShowsMoreThanTheLastThreeLines() {
        let viewModel = makeViewModel()
        let count = LiveCaptionsViewModel.screenLineLimit + 50
        for index in 0 ..< count {
            viewModel.ingestForTesting(text: "строка \(index)", phase: .final)
        }

        let feed = viewModel.feedLines

        XCTAssertEqual(feed.count, LiveCaptionsViewModel.screenLineLimit)
        XCTAssertEqual(feed.last?.text, "строка \(count - 1)")
        XCTAssertEqual(feed.first?.text, "строка \(count - LiveCaptionsViewModel.screenLineLimit)")
    }

    /// Короткая лента показывается целиком — предел режет только хвост.
    func testFeedBelowLimitShowsEverything() {
        let viewModel = makeViewModel()
        for index in 0 ..< 5 {
            viewModel.ingestForTesting(text: "строка \(index)", phase: .final)
        }

        XCTAssertEqual(viewModel.feedLines.count, 5)
    }

    /// Оверлей остаётся коротким: две строки поверх чужого экрана —
    /// это решение Presence, а не ленты.
    func testOverlayTailStaysShort() {
        let viewModel = makeViewModel()
        for index in 0 ..< 10 {
            viewModel.ingestForTesting(text: "строка \(index)", phase: .final)
        }

        XCTAssertEqual(viewModel.recentLines(limit: 2).count, 2)
    }

    /// Вне сессии таймер не должен показывать ноль: ноль читается как
    /// «идёт, но ничего не пишется».
    func testSessionStartIsNilBeforeRecording() {
        XCTAssertNil(makeViewModel().sessionStartedAt)
    }

    /// Ядро отказывает переводить на язык самой сессии. Раньше отказ
    /// проглатывался: переключатель включён, перевода нет, причины не
    /// видно нигде.
    func testTargetEqualToSessionLanguageIsReported() {
        let viewModel = makeViewModel()
        viewModel.applySessionLanguage(.en)
        let store = TranslationSettingsStore()
        store.enabled = true
        store.target = .en

        viewModel.applyTranslationSettings(store)

        XCTAssertFalse(viewModel.translationIssue.isEmpty)
    }

    /// Выключенный перевод — не проблема, о которой надо сообщать.
    func testDisabledTranslationReportsNoIssue() {
        let viewModel = makeViewModel()
        viewModel.applySessionLanguage(.en)
        let store = TranslationSettingsStore()
        store.enabled = false
        store.target = .en

        viewModel.applyTranslationSettings(store)

        XCTAssertTrue(viewModel.translationIssue.isEmpty)
    }

    func testValidTargetReportsNoIssue() {
        let viewModel = makeViewModel()
        viewModel.applySessionLanguage(.ru)
        let store = TranslationSettingsStore()
        store.enabled = true
        store.target = .en

        viewModel.applyTranslationSettings(store)

        XCTAssertTrue(viewModel.translationIssue.isEmpty)
    }
}
