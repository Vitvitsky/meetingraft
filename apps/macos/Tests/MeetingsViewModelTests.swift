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
    var generateResult: FfiGenerateArtifactResult
    var meetingsAfterGenerate: [FfiMeetingSummary]?
    var artifactsAfterGenerate: [FfiArtifact]?
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
        artifacts: [FfiArtifact] = []
    ) {
        self.meetings = meetings
        self.captions = captions
        self.finalTranscript = finalTranscript
        self.artifacts = artifacts
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

    func generateArtifact(
        meetingId _: String,
        kind _: FfiArtifactKind
    ) -> FfiGenerateArtifactResult {
        generateResult
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
