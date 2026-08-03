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
        let caption = FfiCaptionEvent(id: "caption-1", text: "Привет", phase: .final)
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

    func testReloadPublishesSpeakers() {
        let speaker = FfiSpeaker(
            id: "speaker-1",
            meetingId: "meeting-1",
            displayName: "Алиса",
            sortIndex: 0
        )
        let core = MeetingsCoreSpy(speakers: [speaker])
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.speakers, [speaker])
    }

    func testAddSpeakerUsesRussianDefaultName() {
        let core = MeetingsCoreSpy()
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        viewModel.addSpeaker(meetingId: "meeting-1", primaryLanguage: "ru")

        XCTAssertEqual(core.lastUpsertDisplayName, "Спикер 1")
        XCTAssertEqual(core.lastUpsertSortIndex, 0)
    }

    func testAddSpeakerUsesEnglishDefaultName() {
        let core = MeetingsCoreSpy()
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        viewModel.addSpeaker(meetingId: "meeting-1", primaryLanguage: "en")

        XCTAssertEqual(core.lastUpsertDisplayName, "Speaker 1")
        XCTAssertEqual(core.lastUpsertSortIndex, 0)
    }

    func testRenameSpeakerUpdatesDisplayName() {
        let existing = FfiSpeaker(
            id: "speaker-1",
            meetingId: "meeting-1",
            displayName: "Old",
            sortIndex: 0
        )
        let core = MeetingsCoreSpy(speakers: [existing])
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        viewModel.renameSpeaker(
            meetingId: "meeting-1",
            id: "speaker-1",
            displayName: "New"
        )

        XCTAssertEqual(core.lastUpsertDisplayName, "New")
        XCTAssertEqual(core.lastUpsertId, "speaker-1")
        XCTAssertEqual(viewModel.speakers.first?.displayName, "New")
    }

    func testRemoveSpeakerSurfacesCoreError() {
        let core = MeetingsCoreSpy()
        core.deleteSpeakerError = "boom"
        let viewModel = MeetingsViewModel(core: core)

        viewModel.removeSpeaker(id: "speaker-1", meetingId: "meeting-1")

        XCTAssertEqual(viewModel.errorMessage, "boom")
    }

    func testReloadMeetingTreatsEmptyFinalDtoAsMissing() {
        let core = MeetingsCoreSpy()
        let viewModel = MeetingsViewModel(core: core)

        viewModel.reload(meetingId: "meeting-1")

        XCTAssertNil(viewModel.finalTranscript)
    }

    func testGeneratePublishesCoreErrorWithoutReloading() {
        let core = MeetingsCoreSpy()
        core.generateResult = FfiGenerateArtifactResult(
            artifact: makeArtifact(id: "", meetingId: ""),
            error: "final transcript not found"
        )
        let viewModel = MeetingsViewModel(core: core)

        viewModel.generate(meetingId: "meeting-1", kind: .brief)

        XCTAssertEqual(viewModel.errorMessage, "final transcript not found")
        XCTAssertEqual(core.listArtifactsCallCount, 0)
        XCTAssertEqual(core.listMeetingsCallCount, 0)
    }

    func testSubmitBackendRefineHappyPathImmediateSuccess() async {
        let transcript = makeTranscript(meetingId: "meeting-1")
        let local = makeArtifact(id: "local-1", meetingId: "meeting-1")
        let core = MeetingsCoreSpy(finalTranscript: transcript, artifacts: [local])
        core.submitJobResult = FfiBackendJob(
            id: "job-1",
            meetingId: "meeting-1",
            kind: "refine",
            status: "succeeded",
            error: "",
            artifactIds: ["art-b1"]
        )
        core.getArtifactResult = FfiBackendArtifact(
            id: "art-b1",
            kind: "refine",
            bodyMarkdown: "# Stub refine",
            createdAt: "2026-08-02T00:00:00Z",
            error: ""
        )
        let viewModel = MeetingsViewModel(core: core, maxPollAttempts: 20, pollDelayNanoseconds: 0)
        viewModel.reload(meetingId: "meeting-1")

        await viewModel.performBackendRefine(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.backendJobStatus, .succeeded)
        XCTAssertEqual(viewModel.backendJobId, "job-1")
        XCTAssertEqual(viewModel.backendArtifactMarkdown, "# Stub refine")
        XCTAssertEqual(viewModel.artifacts, [local])
        XCTAssertEqual(core.submitBackendJobCallCount, 1)
        XCTAssertEqual(core.getBackendArtifactCallCount, 1)
        XCTAssertEqual(core.getBackendJobCallCount, 0)
        XCTAssertNil(viewModel.errorMessage)
    }

    func testReloadMeetingPreservesBackendRefineSessionState() async {
        let transcript = makeTranscript(meetingId: "meeting-1")
        let local = makeArtifact(id: "local-1", meetingId: "meeting-1")
        let core = MeetingsCoreSpy(finalTranscript: transcript, artifacts: [local])
        core.submitJobResult = FfiBackendJob(
            id: "job-1",
            meetingId: "meeting-1",
            kind: "refine",
            status: "succeeded",
            error: "",
            artifactIds: ["art-b1"]
        )
        core.getArtifactResult = FfiBackendArtifact(
            id: "art-b1",
            kind: "refine",
            bodyMarkdown: "# Stub refine",
            createdAt: "2026-08-02T00:00:00Z",
            error: ""
        )
        let viewModel = MeetingsViewModel(core: core, maxPollAttempts: 20, pollDelayNanoseconds: 0)
        viewModel.reload(meetingId: "meeting-1")

        await viewModel.performBackendRefine(meetingId: "meeting-1")
        viewModel.reload(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.backendJobStatus, .succeeded)
        XCTAssertEqual(viewModel.backendJobId, "job-1")
        XCTAssertEqual(viewModel.backendArtifactMarkdown, "# Stub refine")
    }

    func testSubmitBackendRefineSurfacesSubmitError() async {
        let transcript = makeTranscript(meetingId: "meeting-1")
        let core = MeetingsCoreSpy(finalTranscript: transcript)
        core.submitJobResult = FfiBackendJob(
            id: "", meetingId: "", kind: "", status: "", error: "connection refused", artifactIds: []
        )
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        await viewModel.performBackendRefine(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.backendJobStatus, .failed)
        XCTAssertEqual(viewModel.errorMessage, "connection refused")
        XCTAssertEqual(core.getBackendArtifactCallCount, 0)
    }

    func testSubmitBackendRefineStopsOnFailedJobWithEmptyError() async {
        let transcript = makeTranscript(meetingId: "meeting-1")
        let core = MeetingsCoreSpy(finalTranscript: transcript)
        core.submitJobResult = FfiBackendJob(
            id: "job-1",
            meetingId: "meeting-1",
            kind: "refine",
            status: "failed",
            error: "",
            artifactIds: []
        )
        let viewModel = MeetingsViewModel(core: core, maxPollAttempts: 2, pollDelayNanoseconds: 0)
        viewModel.reload(meetingId: "meeting-1")

        await viewModel.performBackendRefine(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.backendJobStatus, .failed)
        XCTAssertEqual(viewModel.backendJobId, "job-1")
        XCTAssertEqual(viewModel.errorMessage, "Backend job failed")
        XCTAssertEqual(core.getBackendJobCallCount, 0)
        XCTAssertEqual(core.getBackendArtifactCallCount, 0)
    }

    func testSubmitBackendRefineRequiresFinalTranscript() async {
        let core = MeetingsCoreSpy()
        let viewModel = MeetingsViewModel(core: core)
        viewModel.reload(meetingId: "meeting-1")

        await viewModel.performBackendRefine(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.backendJobStatus, .failed)
        XCTAssertEqual(viewModel.errorMessage, "Нужен Final transcript")
        XCTAssertEqual(core.submitBackendJobCallCount, 0)
    }

    func testSubmitBackendRefineTimesOutWhileQueued() async {
        let transcript = makeTranscript(meetingId: "meeting-1")
        let core = MeetingsCoreSpy(finalTranscript: transcript)
        core.submitJobResult = FfiBackendJob(
            id: "job-1",
            meetingId: "meeting-1",
            kind: "refine",
            status: "queued",
            error: "",
            artifactIds: []
        )
        core.getJobResults = [
            FfiBackendJob(
                id: "job-1", meetingId: "meeting-1", kind: "refine",
                status: "queued", error: "", artifactIds: []
            ),
            FfiBackendJob(
                id: "job-1", meetingId: "meeting-1", kind: "refine",
                status: "running", error: "", artifactIds: []
            ),
        ]
        let viewModel = MeetingsViewModel(core: core, maxPollAttempts: 2, pollDelayNanoseconds: 0)
        viewModel.reload(meetingId: "meeting-1")

        await viewModel.performBackendRefine(meetingId: "meeting-1")

        XCTAssertEqual(viewModel.backendJobStatus, .failed)
        XCTAssertEqual(viewModel.errorMessage, "Backend job timeout")
        XCTAssertEqual(core.getBackendArtifactCallCount, 0)
    }

    func testGenerateRefreshesArtifactsAndMeetingBadges() {
        let generated = makeArtifact(id: "artifact-1", meetingId: "meeting-1")
        let updatedMeeting = makeMeeting(id: "meeting-1", artifactCount: 1)
        let core = MeetingsCoreSpy()
        core.generateResult = FfiGenerateArtifactResult(artifact: generated, error: "")
        core.artifactsAfterGenerate = [generated]
        core.meetingsAfterGenerate = [updatedMeeting]
        let viewModel = MeetingsViewModel(core: core)

        viewModel.generate(meetingId: "meeting-1", kind: .brief)

        XCTAssertEqual(viewModel.artifacts, [generated])
        XCTAssertEqual(viewModel.meetings, [updatedMeeting])
        XCTAssertEqual(viewModel.selectedArtifact, generated)
        XCTAssertNil(viewModel.errorMessage)
    }

    func testApplyProviderConfigUpdatesCoreBeforeGenerate() {
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
            llmBaseUrl: "http://127.0.0.1:11434"
        )
        viewModel.generate(meetingId: "meeting-1", kind: .brief)

        XCTAssertEqual(core.apiBaseUrl, "http://localhost:8080")
        XCTAssertEqual(core.apiToken, "test-token")
        XCTAssertEqual(core.lastLlmEngineCode, "ollama")
        XCTAssertEqual(core.lastLlmModelId, "gemma2")
        XCTAssertEqual(core.lastLlmBaseUrl, "http://127.0.0.1:11434")
        XCTAssertEqual(viewModel.selectedArtifact, generated)
    }

    private func makeMeeting(
        id: String,
        artifactCount: UInt64 = 0
    ) -> FfiMeetingSummary {
        FfiMeetingSummary(
            id: id,
            startedAtMs: 1_754_159_200_000,
            hasFinal: true,
            artifactCount: artifactCount
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

    private func makeArtifact(id: String, meetingId: String) -> FfiArtifact {
        FfiArtifact(
            id: id,
            meetingId: meetingId,
            kind: .brief,
            templateId: "brief.v1",
            bodyMarkdown: "# Brief",
            createdAtMs: 1_754_159_400_000
        )
    }
}

private final class MeetingsCoreSpy: MeetingsCoreProviding {
    var meetings: [FfiMeetingSummary]
    var captions: [FfiCaptionEvent]
    var finalTranscript: FfiFinalTranscript
    var artifacts: [FfiArtifact]
    var speakers: [FfiSpeaker]
    var generateResult: FfiGenerateArtifactResult
    var meetingsAfterGenerate: [FfiMeetingSummary]?
    var artifactsAfterGenerate: [FfiArtifact]?
    var deleteSpeakerError = ""
    var submitJobResult: FfiBackendJob = .init(
        id: "", meetingId: "", kind: "", status: "", error: "", artifactIds: []
    )
    var getJobResults: [FfiBackendJob] = []
    var getArtifactResult: FfiBackendArtifact = .init(
        id: "", kind: "", bodyMarkdown: "", createdAt: "", error: ""
    )
    private(set) var listMeetingsCallCount = 0
    private(set) var listArtifactsCallCount = 0
    private(set) var submitBackendJobCallCount = 0
    private(set) var getBackendJobCallCount = 0
    private(set) var getBackendArtifactCallCount = 0
    private(set) var apiBaseUrl = ""
    private(set) var apiToken = ""
    private(set) var lastLlmEngineCode = ""
    private(set) var lastLlmModelId = ""
    private(set) var lastLlmBaseUrl = ""
    private(set) var lastUpsertId: String?
    private(set) var lastUpsertDisplayName: String?
    private(set) var lastUpsertSortIndex: Int64?
    private var getJobIndex = 0

    init(
        meetings: [FfiMeetingSummary] = [],
        captions: [FfiCaptionEvent] = [],
        finalTranscript: FfiFinalTranscript = FfiFinalTranscript(
            meetingId: "",
            version: 0,
            bodyMarkdown: "",
            createdAtMs: 0
        ),
        artifacts: [FfiArtifact] = [],
        speakers: [FfiSpeaker] = []
    ) {
        self.meetings = meetings
        self.captions = captions
        self.finalTranscript = finalTranscript
        self.artifacts = artifacts
        self.speakers = speakers
        generateResult = FfiGenerateArtifactResult(
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

    func listMeetings() -> [FfiMeetingSummary] {
        listMeetingsCallCount += 1
        if let meetingsAfterGenerate {
            meetings = meetingsAfterGenerate
        }
        return meetings
    }

    func listCaptions(meetingId _: String) -> [FfiCaptionEvent] {
        captions
    }

    func getFinalTranscript(meetingId _: String) -> FfiFinalTranscript {
        finalTranscript
    }

    func listArtifacts(meetingId _: String) -> [FfiArtifact] {
        listArtifactsCallCount += 1
        if let artifactsAfterGenerate {
            artifacts = artifactsAfterGenerate
        }
        return artifacts
    }

    func listSpeakers(meetingId _: String) -> [FfiSpeaker] {
        speakers
    }

    func upsertSpeaker(
        meetingId: String,
        id: String,
        displayName: String,
        sortIndex: Int64
    ) -> String {
        lastUpsertId = id
        lastUpsertDisplayName = displayName
        lastUpsertSortIndex = sortIndex

        let savedId = id.isEmpty ? "speaker-\(speakers.count + 1)" : id
        let saved = FfiSpeaker(
            id: savedId,
            meetingId: meetingId,
            displayName: displayName,
            sortIndex: sortIndex
        )
        if let index = speakers.firstIndex(where: { $0.id == savedId }) {
            speakers[index] = saved
        } else {
            speakers.append(saved)
        }
        return ""
    }

    func deleteSpeaker(id _: String) -> String {
        deleteSpeakerError
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

    func setLlmConfig(engineCode: String, modelId: String, baseUrl: String) {
        lastLlmEngineCode = engineCode
        lastLlmModelId = modelId
        lastLlmBaseUrl = baseUrl
    }

    func submitBackendJob(meetingId _: String, kindCode _: String) -> FfiBackendJob {
        submitBackendJobCallCount += 1
        return submitJobResult
    }

    func getBackendJob(jobId _: String) -> FfiBackendJob {
        getBackendJobCallCount += 1
        guard getJobIndex < getJobResults.count else {
            return getJobResults.last ?? submitJobResult
        }
        defer { getJobIndex += 1 }
        return getJobResults[getJobIndex]
    }

    func getBackendArtifact(artifactId _: String) -> FfiBackendArtifact {
        getBackendArtifactCallCount += 1
        return getArtifactResult
    }
}
