@testable import MeetingRaft
import XCTest

/// Библиотека встреч: название, длительность, поиск, переименование, удаление.
@MainActor
final class MeetingsLibraryTests: XCTestCase {
    private func makeMeeting(
        id: String,
        title: String = "",
        startedAtMs: UInt64 = 1_754_159_200_000,
        endedAtMs: UInt64 = 0
    ) -> FfiMeetingSummary {
        FfiMeetingSummary(
            id: id,
            title: title,
            startedAtMs: startedAtMs,
            endedAtMs: endedAtMs,
            hasFinal: false,
            artifactCount: 0
        )
    }

    /// Пустое название заменяется датой, а не показывается пустым.
    func testDisplayTitleFallsBackToDate() {
        let core = LibraryCoreSpy()
        let viewModel = MeetingsViewModel(core: core)
        let meeting = makeMeeting(id: "m1")

        let title = viewModel.displayTitle(for: meeting)

        XCTAssertFalse(title.isEmpty)
        XCTAssertFalse(title.trimmingCharacters(in: .whitespaces).isEmpty)
    }

    func testDisplayTitleUsesStoredTitle() {
        let viewModel = MeetingsViewModel(core: LibraryCoreSpy())
        let meeting = makeMeeting(id: "m1", title: "Ретро спринта")

        XCTAssertEqual(viewModel.displayTitle(for: meeting), "Ретро спринта")
    }

    /// Незавершённая встреча не имеет длительности.
    func testDurationIsNilWhileRecording() {
        let viewModel = MeetingsViewModel(core: LibraryCoreSpy())

        XCTAssertNil(viewModel.duration(for: makeMeeting(id: "m1")))
    }

    func testDurationComputedFromEndTimestamp() {
        let viewModel = MeetingsViewModel(core: LibraryCoreSpy())
        let meeting = makeMeeting(
            id: "m1",
            startedAtMs: 1_000_000,
            endedAtMs: 1_090_000
        )

        XCTAssertEqual(viewModel.duration(for: meeting), .milliseconds(90000))
    }

    func testRenameUpdatesListOnSuccess() {
        let core = LibraryCoreSpy()
        core.meetings = [makeMeeting(id: "m1", title: "Черновик")]
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload()

        viewModel.rename(meetingId: "m1", title: "  Ретро спринта  ")

        XCTAssertEqual(core.renameCalls.first?.1, "Ретро спринта", "название триммится")
        XCTAssertEqual(viewModel.meetings.first?.title, "Ретро спринта")
        XCTAssertNil(viewModel.errorMessage)
    }

    func testRenameFailureSurfacesErrorAndKeepsList() {
        let core = LibraryCoreSpy()
        core.meetings = [makeMeeting(id: "m1", title: "Черновик")]
        core.renameError = "meeting not found: m1"
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload()

        viewModel.rename(meetingId: "m1", title: "Новое")

        XCTAssertEqual(viewModel.errorMessage, "meeting not found: m1")
        XCTAssertEqual(viewModel.meetings.first?.title, "Черновик")
    }

    func testDeleteRemovesMeetingFromList() {
        let core = LibraryCoreSpy()
        core.meetings = [makeMeeting(id: "m1"), makeMeeting(id: "m2")]
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload()

        viewModel.delete(meetingId: "m1")

        XCTAssertEqual(viewModel.meetings.map(\.id), ["m2"])
        XCTAssertNil(viewModel.errorMessage)
    }

    func testDeleteFailureSurfacesError() {
        let core = LibraryCoreSpy()
        core.meetings = [makeMeeting(id: "m1")]
        core.deleteError = "meeting is being recorded"
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload()

        viewModel.delete(meetingId: "m1")

        XCTAssertEqual(viewModel.errorMessage, "meeting is being recorded")
        XCTAssertEqual(viewModel.meetings.count, 1)
    }

    /// Дебаунс схлопывает серию нажатий в один запрос.
    func testSearchDebouncesRapidTyping() async {
        let core = LibraryCoreSpy()
        core.searchHits = [
            FfiSearchHit(meetingId: "m1", kind: "final", refId: "1", snippet: "…биллинга…"),
        ]
        let viewModel = MeetingsViewModel(core: core, searchDebounceNanoseconds: 20_000_000)

        viewModel.query = "б"
        viewModel.query = "би"
        viewModel.query = "билл"
        try? await Task.sleep(nanoseconds: 120_000_000)

        XCTAssertEqual(core.searchQueries, ["билл"])
        XCTAssertEqual(viewModel.searchHits.count, 1)
        XCTAssertFalse(viewModel.isSearching)
    }

    /// Пустой запрос очищает результаты и не идёт в ядро.
    func testEmptyQueryClearsHitsWithoutSearching() async {
        let core = LibraryCoreSpy()
        core.searchHits = [
            FfiSearchHit(meetingId: "m1", kind: "final", refId: "1", snippet: "…"),
        ]
        let viewModel = MeetingsViewModel(core: core, searchDebounceNanoseconds: 10_000_000)

        viewModel.query = "биллинг"
        try? await Task.sleep(nanoseconds: 60_000_000)
        XCTAssertEqual(viewModel.searchHits.count, 1)

        viewModel.query = "   "
        try? await Task.sleep(nanoseconds: 60_000_000)

        XCTAssertTrue(viewModel.searchHits.isEmpty)
        XCTAssertEqual(core.searchQueries, ["биллинг"], "пробелы не уходят в поиск")
    }
}

/// Минимальный спай: библиотечные методы настоящие, остальное — заглушки.
private final class LibraryCoreSpy: MeetingsCoreProviding {
    var meetings: [FfiMeetingSummary] = []
    var renameError = ""
    var deleteError = ""
    var searchHits: [FfiSearchHit] = []
    private(set) var renameCalls: [(String, String)] = []
    private(set) var deleteCalls: [String] = []
    private(set) var searchQueries: [String] = []

    func listMeetings() -> [FfiMeetingSummary] {
        meetings
    }

    func renameMeeting(meetingId: String, title: String) -> String {
        renameCalls.append((meetingId, title))
        guard renameError.isEmpty else { return renameError }
        meetings = meetings.map { meeting in
            guard meeting.id == meetingId else { return meeting }
            return FfiMeetingSummary(
                id: meeting.id,
                title: title,
                startedAtMs: meeting.startedAtMs,
                endedAtMs: meeting.endedAtMs,
                hasFinal: meeting.hasFinal,
                artifactCount: meeting.artifactCount
            )
        }
        return ""
    }

    func deleteMeeting(meetingId: String) -> String {
        deleteCalls.append(meetingId)
        guard deleteError.isEmpty else { return deleteError }
        meetings.removeAll { $0.id == meetingId }
        return ""
    }

    func searchMeetings(query: String, limit _: UInt32) -> [FfiSearchHit] {
        searchQueries.append(query)
        return searchHits
    }

    func listCaptions(meetingId _: String) -> [FfiCaptionEvent] {
        []
    }

    func getFinalTranscript(meetingId _: String) -> FfiFinalTranscript {
        FfiFinalTranscript(meetingId: "", version: 0, bodyMarkdown: "", createdAtMs: 0)
    }

    func listFinalTranscripts(meetingId _: String) -> [FfiFinalTranscript] {
        []
    }

    func getFinalTranscriptVersion(meetingId _: String, version _: UInt32) -> FfiFinalTranscript {
        FfiFinalTranscript(meetingId: "", version: 0, bodyMarkdown: "", createdAtMs: 0)
    }

    func listArtifacts(meetingId _: String) -> [FfiArtifact] {
        []
    }

    func setApiConfig(baseUrl _: String, token _: String) {}

    func setLlmConfig(engineCode _: String, modelId _: String, baseUrl _: String, providerId _: String) {}

    func generateArtifact(meetingId _: String, kind _: FfiArtifactKind) -> FfiGenerateArtifactResult {
        FfiGenerateArtifactResult(
            artifact: FfiArtifact(
                id: "",
                meetingId: "",
                kind: .brief,
                templateId: "",
                bodyMarkdown: "",
                createdAtMs: 0
            ),
            error: ""
        )
    }

    func submitBackendJob(meetingId _: String, kindCode _: String) -> FfiBackendJob {
        FfiBackendJob(id: "", meetingId: "", kind: "", status: "", error: "", artifactIds: [])
    }

    func getBackendJob(jobId _: String) -> FfiBackendJob {
        FfiBackendJob(id: "", meetingId: "", kind: "", status: "", error: "", artifactIds: [])
    }

    func getBackendArtifact(artifactId _: String) -> FfiBackendArtifact {
        FfiBackendArtifact(id: "", kind: "", bodyMarkdown: "", createdAt: "", error: "")
    }
}
