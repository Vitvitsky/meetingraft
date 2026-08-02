import Observation

/// Контракт истории встреч для presentation model и тестов.
protocol MeetingsCoreProviding: AnyObject {
    func listMeetings() -> [FfiMeetingSummary]
    func listCaptions(meetingId: String) -> [FfiCaptionEvent]
    func getFinalTranscript(meetingId: String) -> FfiFinalTranscript
    func listArtifacts(meetingId: String) -> [FfiArtifact]
    func generateArtifact(meetingId: String, kind: FfiArtifactKind) -> FfiGenerateArtifactResult
}

extension MeetingCore: MeetingsCoreProviding {}

/// Presentation model локальной истории и post-call артефактов.
@Observable
@MainActor
final class MeetingsViewModel {
    private(set) var meetings: [FfiMeetingSummary] = []
    private(set) var captions: [FfiCaptionEvent] = []
    private(set) var finalTranscript: FfiFinalTranscript?
    private(set) var artifacts: [FfiArtifact] = []
    private(set) var selectedArtifact: FfiArtifact?
    private(set) var errorMessage: String?

    private let core: any MeetingsCoreProviding

    init(core: any MeetingsCoreProviding) {
        self.core = core
    }

    func reload() {
        meetings = core.listMeetings()
        errorMessage = nil
    }

    func reload(meetingId: String) {
        captions = core.listCaptions(meetingId: meetingId)

        let transcript = core.getFinalTranscript(meetingId: meetingId)
        finalTranscript = transcript.meetingId.isEmpty ? nil : transcript

        artifacts = core.listArtifacts(meetingId: meetingId)
        if let selectedArtifact,
           let refreshed = artifacts.first(where: { $0.id == selectedArtifact.id })
        {
            self.selectedArtifact = refreshed
        } else {
            selectedArtifact = artifacts.first
        }
        errorMessage = nil
    }

    func generate(meetingId: String, kind: FfiArtifactKind) {
        let result = core.generateArtifact(meetingId: meetingId, kind: kind)
        guard result.error.isEmpty else {
            errorMessage = result.error
            return
        }

        artifacts = core.listArtifacts(meetingId: meetingId)
        meetings = core.listMeetings()
        selectedArtifact = artifacts.first(where: { $0.id == result.artifact.id }) ?? result.artifact
        errorMessage = nil
    }

    func selectArtifact(_ artifact: FfiArtifact) {
        selectedArtifact = artifact
    }

    func dismissError() {
        errorMessage = nil
    }
}
