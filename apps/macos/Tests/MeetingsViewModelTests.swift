@testable import MeetingRaft
import XCTest

@MainActor
final class MeetingsViewModelTests: XCTestCase {
    func testReloadPublishesMeetingsFromCore() {
        let expected = makeMeeting(id: "meeting-1")
        let core = MeetingsCoreSpy(meetings: [expected])
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload()

        XCTAssertEqual(viewModel.meetings, [expected])
        XCTAssertNil(viewModel.errorMessage)
    }

    func testReloadMeetingPublishesAllSavedContent() {
        let caption = FfiCaptionEvent(id: "caption-1", text: "Привет", phase: .final, channel: "mic")
        let transcript = makeTranscript(meetingId: "meeting-1")
        let artifact = makeArtifact(id: "artifact-1", meetingId: "meeting-1")
        let core = MeetingsCoreSpy(
            captions: [caption],
            finalTranscript: transcript,
            artifacts: [artifact]
        )
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.captions, [caption])
        XCTAssertEqual(viewModel.finalTranscript, transcript)
        XCTAssertEqual(viewModel.artifacts, [artifact])
    }

    func testReloadMeetingTreatsEmptyFinalDtoAsMissing() {
        let core = MeetingsCoreSpy()
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload(meetingId: "meeting-1")

        XCTAssertNil(viewModel.finalTranscript)
        XCTAssertTrue(viewModel.finalVersions.isEmpty)
        XCTAssertNil(viewModel.selectedFinalVersion)
    }

    func testReloadLoadsFinalVersionsDescending() {
        let v1 = FfiFinalTranscript(
            meetingId: "meeting-1",
            version: 1,
            bodyMarkdown: "# v1",
            createdAtMs: 100
        )
        let v2 = FfiFinalTranscript(
            meetingId: "meeting-1",
            version: 2,
            bodyMarkdown: "# v2",
            createdAtMs: 200
        )
        let core = MeetingsCoreSpy(finalTranscript: v2, finalVersions: [v2, v1])
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.finalVersions.map(\.version), [2, 1])
        XCTAssertEqual(viewModel.finalVersions[0].version, 2)
        XCTAssertEqual(viewModel.selectedFinalVersion, 2)
        XCTAssertEqual(viewModel.finalTranscript, v2)
        XCTAssertEqual(viewModel.selectedFinalBody, "# v2")
    }

    func testSelectedFinalBodyUsesSelectedVersion() {
        let v1 = FfiFinalTranscript(
            meetingId: "meeting-1",
            version: 1,
            bodyMarkdown: "# v1",
            createdAtMs: 100
        )
        let v2 = FfiFinalTranscript(
            meetingId: "meeting-1",
            version: 2,
            bodyMarkdown: "# v2",
            createdAtMs: 200
        )
        let core = MeetingsCoreSpy(finalTranscript: v2, finalVersions: [v2, v1])
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        viewModel.selectedFinalVersion = 1

        XCTAssertEqual(viewModel.selectedFinalBody, "# v1")
        XCTAssertEqual(viewModel.finalTranscript?.version, 2)
    }

    /// Показанный текст и версия, к которой он относится, не должны
    /// расходиться: тело трактует `nil` как «последняя», и атрибуция
    /// обязана трактовать так же — иначе на экране окажется текст без
    /// сегментов, то есть без правки и прослушивания при живых данных.
    func testEffectiveVersionFollowsTheBodyWhenNothingIsSelected() {
        let v1 = FfiFinalTranscript(
            meetingId: "meeting-1",
            version: 1,
            bodyMarkdown: "# v1",
            createdAtMs: 100
        )
        let v2 = FfiFinalTranscript(
            meetingId: "meeting-1",
            version: 2,
            bodyMarkdown: "# v2",
            createdAtMs: 200
        )
        let core = MeetingsCoreSpy(finalTranscript: v2, finalVersions: [v2, v1])
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        viewModel.selectedFinalVersion = nil

        XCTAssertFalse(viewModel.selectedFinalBody.isEmpty, "тело показано")
        XCTAssertEqual(viewModel.effectiveFinalVersion, 2, "и версия у него есть")
        XCTAssertEqual(viewModel.selectedFinalBody, "# v2")
    }

    /// Финала нет вовсе — версии нет тоже, и это не то же самое, что
    /// «не выбрана».
    func testEffectiveVersionIsNilWithoutAnyFinal() {
        let core = MeetingsCoreSpy()
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload(meetingId: "meeting-1")

        XCTAssertNil(viewModel.effectiveFinalVersion)
        XCTAssertTrue(viewModel.selectedFinalBody.isEmpty)
    }

    func testLiveFinalsTextJoinsFinalCaptions() {
        let captions = [
            FfiCaptionEvent(id: "1", text: "partial only", phase: .partial, channel: "mic"),
            FfiCaptionEvent(id: "2", text: "first final", phase: .final, channel: "mic"),
            FfiCaptionEvent(id: "3", text: "second final", phase: .final, channel: "mic"),
        ]
        let viewModel = MeetingsViewModel(core: MeetingsCoreSpy())

        XCTAssertEqual(
            viewModel.liveFinalsText(from: captions),
            "first final\n\nsecond final"
        )
    }

    func testGeneratePublishesCoreErrorWithoutReloading() async {
        let core = MeetingsCoreSpy()
        core.generateResult = FfiGenerateArtifactResult(
            artifact: makeArtifact(id: "", meetingId: ""),
            error: "final transcript not found"
        )
        let viewModel = MeetingsViewModel(core: core)

        await viewModel.generate(meetingId: "meeting-1", kind: .brief)

        XCTAssertEqual(viewModel.errorMessage, "final transcript not found")
        XCTAssertEqual(core.listArtifactsCallCount, 0)
        XCTAssertEqual(core.listMeetingsCallCount, 0)
    }

    func testGenerateRefreshesArtifactsAndMeetingBadges() async {
        let generated = makeArtifact(id: "artifact-1", meetingId: "meeting-1")
        let updatedMeeting = makeMeeting(id: "meeting-1", artifactCount: 1)
        let core = MeetingsCoreSpy()
        core.generateResult = FfiGenerateArtifactResult(artifact: generated, error: "")
        core.artifactsAfterGenerate = [generated]
        core.meetingsAfterGenerate = [updatedMeeting]
        let viewModel = MeetingsViewModel(core: core)

        await viewModel.generate(meetingId: "meeting-1", kind: .brief)

        XCTAssertEqual(viewModel.artifacts, [generated])
        XCTAssertEqual(viewModel.meetings, [updatedMeeting])
        XCTAssertEqual(viewModel.selectedArtifact, generated)
        XCTAssertNil(viewModel.errorMessage)
    }

    func testExportMarkdownWritesFinalAndLatestArtifacts() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mr-vm-export-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let transcript = FfiFinalTranscript(
            meetingId: "abcd1234-rest",
            version: 1,
            bodyMarkdown: "# Final body",
            createdAtMs: 1
        )
        let briefOld = makeArtifact(
            id: "b0", meetingId: "abcd1234-rest", kind: .brief, body: "old", createdAtMs: 10
        )
        let briefNew = makeArtifact(
            id: "b1", meetingId: "abcd1234-rest", kind: .brief, body: "new brief", createdAtMs: 20
        )
        let follow = makeArtifact(
            id: "f1", meetingId: "abcd1234-rest", kind: .followUp, body: "fu", createdAtMs: 15
        )
        let core = MeetingsCoreSpy(finalTranscript: transcript, artifacts: [briefOld, briefNew, follow])
        let vm = MeetingsViewModel(core: core)

        let result = vm.exportMarkdown(
            meetingId: "abcd1234-rest",
            startedAtMs: 1_785_715_200_000,
            folderURL: dir
        )

        guard case let .success(ok) = result else {
            return XCTFail("\(result)")
        }
        XCTAssertEqual(ok.writtenFileNames.count, 3)
        XCTAssertFalse(vm.exportStatusMessage.isEmpty)
        let briefFileName = try XCTUnwrap(ok.writtenFileNames.first { $0.contains("brief") })
        let briefURL = dir.appendingPathComponent(briefFileName)
        XCTAssertEqual(try String(contentsOf: briefURL, encoding: .utf8), "new brief")
        let finalFileName = try XCTUnwrap(ok.writtenFileNames.first { $0.contains("final") })
        let finalURL = dir.appendingPathComponent(finalFileName)
        XCTAssertEqual(try String(contentsOf: finalURL, encoding: .utf8), "# Final body")
    }

    func testExportMarkdownWritesOnlyFinalWhenNoArtifacts() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mr-vm-export-final-only-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let transcript = FfiFinalTranscript(
            meetingId: "abcd1234-rest",
            version: 1,
            bodyMarkdown: "# Final only",
            createdAtMs: 1
        )
        let core = MeetingsCoreSpy(finalTranscript: transcript, artifacts: [])
        let vm = MeetingsViewModel(core: core)

        let result = vm.exportMarkdown(
            meetingId: "abcd1234-rest",
            startedAtMs: 1_785_715_200_000,
            folderURL: dir
        )

        guard case let .success(ok) = result else {
            return XCTFail("\(result)")
        }
        XCTAssertEqual(ok.writtenFileNames.count, 1)
        XCTAssertTrue(ok.writtenFileNames[0].contains("final"))
        let finalURL = dir.appendingPathComponent(ok.writtenFileNames[0])
        XCTAssertEqual(try String(contentsOf: finalURL, encoding: .utf8), "# Final only")
    }

    func testExportMarkdownFailsWithoutFinal() {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mr-vm-export-empty-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let core = MeetingsCoreSpy(
            finalTranscript: FfiFinalTranscript(meetingId: "", version: 0, bodyMarkdown: "", createdAtMs: 0)
        )
        let vm = MeetingsViewModel(core: core)

        let result = vm.exportMarkdown(meetingId: "m1", startedAtMs: 1, folderURL: dir)

        guard case let .failure(error) = result else {
            return XCTFail("expected failure")
        }
        let expected = String(localized: "A Final transcript is required")
        XCTAssertEqual(error.message, expected)
        XCTAssertEqual(vm.exportStatusMessage, expected)
    }

    func testApplyProviderConfigUpdatesCoreBeforeGenerate() async {
        let generated = makeArtifact(id: "artifact-1", meetingId: "meeting-1")
        let core = MeetingsCoreSpy()
        core.generateResult = FfiGenerateArtifactResult(artifact: generated, error: "")
        core.artifactsAfterGenerate = [generated]
        let viewModel = MeetingsViewModel(core: core)

        viewModel.applyProviderConfig(
            apiBaseUrl: "http://localhost:8080",
            apiToken: "test-token",
            llmEngineCode: "ollama",
            llmModelId: "gemma2",
            llmBaseUrl: "http://127.0.0.1:11434",
            llmProviderId: "default"
        )
        await viewModel.generate(meetingId: "meeting-1", kind: .brief)

        XCTAssertEqual(core.apiBaseUrl, "http://localhost:8080")
        XCTAssertEqual(core.apiToken, "test-token")
        XCTAssertEqual(core.lastLlmEngineCode, "ollama")
        XCTAssertEqual(core.lastLlmModelId, "gemma2")
        XCTAssertEqual(core.lastLlmBaseUrl, "http://127.0.0.1:11434")
        XCTAssertEqual(core.lastLlmProviderId, "default")
        XCTAssertEqual(viewModel.selectedArtifact, generated)
    }

    private func makeMeeting(
        id: String,
        title: String = "",
        artifactCount: UInt64 = 0,
        endedAtMs: UInt64 = 0
    ) -> FfiMeetingSummary {
        FfiMeetingSummary(
            id: id,
            title: title,
            startedAtMs: 1_754_159_200_000,
            endedAtMs: endedAtMs,
            hasFinal: true,
            artifactCount: artifactCount,
            audioDeletedAtMs: 0
        )
    }

    private func makeTranscript(meetingId: String) -> FfiFinalTranscript {
        FfiFinalTranscript(
            meetingId: meetingId,
            version: 1,
            bodyMarkdown: "# Итоги",
            createdAtMs: 1_754_159_300_000
        )
    }

    private func makeArtifact(
        id: String,
        meetingId: String,
        kind: FfiArtifactKind = .brief,
        body: String = "# Brief",
        createdAtMs: UInt64 = 1_754_159_400_000,
        isStale: Bool = false
    ) -> FfiArtifact {
        FfiArtifact(
            id: id,
            meetingId: meetingId,
            kind: kind,
            templateId: kind == .followUp ? "follow-up.v1" : "brief.v1",
            bodyMarkdown: body,
            createdAtMs: createdAtMs,
            isStale: isStale,
            sourceVersion: 1
        )
    }
}

/// Обращения к spy сериализованы `await` в тестах: presentation model
/// ждёт результат фонового вызова, прежде чем читать счётчики.
private final class MeetingsCoreSpy: MeetingsCoreProviding, @unchecked Sendable {
    var meetings: [FfiMeetingSummary]
    var captions: [FfiCaptionEvent]
    var finalTranscript: FfiFinalTranscript
    var finalVersions: [FfiFinalTranscript]
    var artifacts: [FfiArtifact]
    var generateResult: FfiGenerateArtifactResult
    var meetingsAfterGenerate: [FfiMeetingSummary]?
    var artifactsAfterGenerate: [FfiArtifact]?
    var renameError = ""
    var deleteError = ""
    var searchHits: [FfiSearchHit] = []
    private(set) var renameCalls: [(String, String)] = []
    private(set) var deleteCalls: [String] = []
    private(set) var searchQueries: [String] = []
    private(set) var listMeetingsCallCount = 0
    private(set) var listArtifactsCallCount = 0
    private(set) var apiBaseUrl = ""
    private(set) var apiToken = ""
    private(set) var lastLlmEngineCode = ""
    private(set) var lastLlmModelId = ""
    private(set) var lastLlmBaseUrl = ""
    private(set) var lastLlmProviderId = ""

    init(
        meetings: [FfiMeetingSummary] = [],
        captions: [FfiCaptionEvent] = [],
        finalTranscript: FfiFinalTranscript = FfiFinalTranscript(
            meetingId: "",
            version: 0,
            bodyMarkdown: "",
            createdAtMs: 0
        ),
        finalVersions: [FfiFinalTranscript]? = nil,
        artifacts: [FfiArtifact] = []
    ) {
        self.meetings = meetings
        self.captions = captions
        self.finalTranscript = finalTranscript
        if let finalVersions {
            self.finalVersions = finalVersions
        } else if finalTranscript.meetingId.isEmpty {
            self.finalVersions = []
        } else {
            self.finalVersions = [finalTranscript]
        }
        self.artifacts = artifacts
        generateResult = FfiGenerateArtifactResult(
            artifact: FfiArtifact(
                id: "",
                meetingId: "",
                kind: .brief,
                templateId: "",
                bodyMarkdown: "",
                createdAtMs: 0,
                isStale: false,
                sourceVersion: 0
            ),
            error: ""
        )
    }

    func listMeetings() -> [FfiMeetingSummary] {
        listMeetingsCallCount += 1
        if let meetingsAfterGenerate {
            meetings = meetingsAfterGenerate
        }
        return meetings
    }

    func renameMeeting(meetingId: String, title: String) -> String {
        renameCalls.append((meetingId, title))
        if renameError.isEmpty {
            meetings = meetings.map { meeting in
                guard meeting.id == meetingId else { return meeting }
                return FfiMeetingSummary(
                    id: meeting.id,
                    title: title,
                    startedAtMs: meeting.startedAtMs,
                    endedAtMs: meeting.endedAtMs,
                    hasFinal: meeting.hasFinal,
                    artifactCount: meeting.artifactCount,
                    audioDeletedAtMs: meeting.audioDeletedAtMs
                )
            }
        }
        return renameError
    }

    func deleteMeeting(meetingId: String) -> String {
        deleteCalls.append(meetingId)
        if deleteError.isEmpty {
            meetings.removeAll { $0.id == meetingId }
        }
        return deleteError
    }

    // Epic 22: удаление аудио отдельно от встречи. Дубли обязаны знать о
    // каждом методе протокола, иначе тестовая цель не собирается.
    var deletedAudioCalls: [String] = []
    var deleteAudioError = ""
    var audioBytesByMeeting: [String: UInt64] = [:]
    var sweepPreview: [FfiAudioSweepEntry] = []

    func deleteMeetingAudio(meetingId: String) -> String {
        deletedAudioCalls.append(meetingId)
        guard deleteAudioError.isEmpty else { return deleteAudioError }
        audioBytesByMeeting[meetingId] = 0
        return ""
    }

    func meetingAudioBytes(meetingId: String) -> UInt64 {
        audioBytesByMeeting[meetingId] ?? 0
    }

    func previewAudioSweep(olderThanMs: UInt64) -> [FfiAudioSweepEntry] {
        sweepPreview.filter { $0.startedAtMs < olderThanMs }
    }

    func runAudioSweep(olderThanMs: UInt64) -> FfiAudioSweepResult {
        let taken = previewAudioSweep(olderThanMs: olderThanMs)
        for entry in taken {
            _ = deleteMeetingAudio(meetingId: entry.meetingId)
        }
        return FfiAudioSweepResult(
            deletedCount: UInt32(taken.count),
            freedBytes: taken.reduce(0) { $0 + $1.bytes },
            skipped: []
        )
    }

    func searchMeetings(query: String, limit: UInt32) -> [FfiSearchHit] {
        searchQueries.append(query)
        _ = limit
        return searchHits
    }

    func listCaptions(meetingId _: String) -> [FfiCaptionEvent] {
        captions
    }

    func getFinalTranscript(meetingId _: String) -> FfiFinalTranscript {
        finalTranscript
    }

    func listFinalTranscripts(meetingId _: String) -> [FfiFinalTranscript] {
        finalVersions
    }

    func getFinalTranscriptVersion(meetingId _: String, version: UInt32) -> FfiFinalTranscript {
        finalVersions.first(where: { $0.version == version })
            ?? FfiFinalTranscript(meetingId: "", version: 0, bodyMarkdown: "", createdAtMs: 0)
    }

    func listArtifacts(meetingId _: String) -> [FfiArtifact] {
        listArtifactsCallCount += 1
        if let artifactsAfterGenerate {
            artifacts = artifactsAfterGenerate
        }
        return artifacts
    }

    func generateArtifact(
        meetingId _: String,
        kind _: FfiArtifactKind
    ) -> FfiGenerateArtifactResult {
        generateResult
    }

    func setApiConfig(baseUrl: String, token: String) {
        apiBaseUrl = baseUrl
        apiToken = token
    }

    func setLlmConfig(engineCode: String, modelId: String, baseUrl: String, providerId: String) {
        lastLlmEngineCode = engineCode
        lastLlmModelId = modelId
        lastLlmBaseUrl = baseUrl
        lastLlmProviderId = providerId
    }
}
