@testable import MeetingRaft
import XCTest

@MainActor
final class FinalRebuildViewModelTests: XCTestCase {
    private func progress(
        state: String,
        done: UInt32 = 0,
        total: UInt32 = 100,
        error: String = "",
        note: String = ""
    ) -> FfiRebuildProgress {
        FfiRebuildProgress(
            jobId: "j1",
            meetingId: "m1",
            state: state,
            done: done,
            total: total,
            error: error,
            note: note
        )
    }

    func testStartAsksCoreAndTracksJob() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        core.progressToReturn = progress(state: "running", done: 40)
        let viewModel = FinalRebuildViewModel(core: core)

        viewModel.start(meetingId: "m1")
        viewModel.refresh()

        XCTAssertEqual(core.startCalls, ["m1"])
        XCTAssertEqual(viewModel.jobId, "j1")
        XCTAssertEqual(viewModel.fraction, 0.4, accuracy: 0.001)
        XCTAssertTrue(viewModel.isRunning)
    }

    /// Provenance берётся только у успешного прохода.
    func testProvenanceOnlyFromSucceededPass() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        let viewModel = FinalRebuildViewModel(core: core)
        viewModel.start(meetingId: "m1")

        core.progressToReturn = progress(state: "running", note: "re-ASR large-v3")
        viewModel.refresh()
        XCTAssertTrue(viewModel.provenance.isEmpty, "идущий проход ещё ничего не доказал")

        core.progressToReturn = progress(state: "succeeded", done: 100, note: "re-ASR large-v3")
        viewModel.refresh()
        XCTAssertEqual(viewModel.provenance, "re-ASR large-v3")
    }

    func testFailedPassShowsErrorAndNoProvenance() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        let viewModel = FinalRebuildViewModel(core: core)
        viewModel.start(meetingId: "m1")

        core.progressToReturn = progress(state: "failed", error: "модель не скачана", note: "x")
        viewModel.refresh()

        XCTAssertEqual(viewModel.errorMessage, "модель не скачана")
        XCTAssertTrue(viewModel.provenance.isEmpty)
        XCTAssertFalse(viewModel.isRunning)
        XCTAssertEqual(viewModel.statusText, "модель не скачана")
    }

    /// Отмена не должна выглядеть сбоем.
    func testCancelledPassIsNotAnError() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        let viewModel = FinalRebuildViewModel(core: core)
        viewModel.start(meetingId: "m1")

        core.progressToReturn = progress(state: "cancelled")
        viewModel.refresh()

        XCTAssertTrue(viewModel.errorMessage.isEmpty)
        XCTAssertFalse(viewModel.isRunning)
    }

    func testCancelForwardsJobIdToCore() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        let viewModel = FinalRebuildViewModel(core: core)
        viewModel.start(meetingId: "m1")

        viewModel.cancel()

        XCTAssertEqual(core.cancelCalls, ["j1"])
    }

    /// Проход, начатый до открытия экрана, подхватывается.
    func testAttachPicksUpRunningJob() {
        let core = RebuildCoreSpy()
        core.activeJobId = "j-existing"
        core.progressToReturn = progress(state: "running", done: 10)
        let viewModel = FinalRebuildViewModel(core: core)

        viewModel.attach(meetingId: "m1")
        viewModel.refresh()

        XCTAssertEqual(viewModel.jobId, "j-existing")
        XCTAssertTrue(viewModel.isRunning)
    }

    func testAttachWithoutRunningJobDoesNothing() {
        let core = RebuildCoreSpy()
        let viewModel = FinalRebuildViewModel(core: core)

        viewModel.attach(meetingId: "m1")

        XCTAssertTrue(viewModel.jobId.isEmpty)
        XCTAssertFalse(viewModel.isRunning)
    }

    /// Повторный старт при идущем проходе не должен плодить задачи.
    func testStartIsIgnoredWhileRunning() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        core.progressToReturn = progress(state: "running")
        let viewModel = FinalRebuildViewModel(core: core)
        viewModel.start(meetingId: "m1")
        viewModel.refresh()

        viewModel.start(meetingId: "m1")

        XCTAssertEqual(core.startCalls, ["m1"], "второй запуск не должен уйти в ядро")
    }

    func testZeroTotalDoesNotDivideByZero() {
        let core = RebuildCoreSpy()
        core.startedJobId = "j1"
        core.progressToReturn = progress(state: "queued", done: 0, total: 0)
        let viewModel = FinalRebuildViewModel(core: core)
        viewModel.start(meetingId: "m1")

        viewModel.refresh()

        XCTAssertEqual(viewModel.fraction, 0)
    }
}

private final class RebuildCoreSpy: FinalRebuildCoreProviding {
    var startedJobId = ""
    var activeJobId = ""
    var progressToReturn = FfiRebuildProgress(
        jobId: "", meetingId: "", state: "", done: 0, total: 0, error: "", note: ""
    )
    var diffToReturn: [FfiDiffSpan] = []
    private(set) var startCalls: [String] = []
    private(set) var cancelCalls: [String] = []

    func startFinalRebuild(meetingId: String) -> String {
        startCalls.append(meetingId)
        return startedJobId
    }

    func finalRebuildProgress(jobId _: String) -> FfiRebuildProgress {
        progressToReturn
    }

    func cancelFinalRebuild(jobId: String) {
        cancelCalls.append(jobId)
    }

    func activeFinalRebuild(meetingId _: String) -> String {
        activeJobId
    }

    func diffLiveVsFinal(meetingId _: String, version _: UInt32) -> [FfiDiffSpan] {
        diffToReturn
    }
}
