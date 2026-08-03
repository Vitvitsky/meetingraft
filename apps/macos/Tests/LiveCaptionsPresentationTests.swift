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

    /// Вне сессии таймер не должен показывать ноль: ноль читается как
    /// «идёт, но ничего не пишется».
    func testSessionStartIsNilBeforeRecording() {
        XCTAssertNil(makeViewModel().sessionStartedAt)
    }
}
