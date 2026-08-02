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
    private(set) var listMeetingsCallCount = 0
    private(set) var listArtifactsCallCount = 0

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
}
