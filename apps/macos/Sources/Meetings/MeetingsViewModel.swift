import Observation

/// Контракт истории встреч для presentation model и тестов.
protocol MeetingsCoreProviding: AnyObject {
    func listMeetings() -> [FfiMeetingSummary]
    func listCaptions(meetingId: String) -> [FfiCaptionEvent]
    func getFinalTranscript(meetingId: String) -> FfiFinalTranscript
    func listArtifacts(meetingId: String) -> [FfiArtifact]
    func generateArtifact(meetingId: String, kind: FfiArtifactKind) -> FfiGenerateArtifactResult
    func submitBackendJob(meetingId: String, kindCode: String) -> FfiBackendJob
    func getBackendJob(jobId: String) -> FfiBackendJob
    func getBackendArtifact(artifactId: String) -> FfiBackendArtifact
}

extension MeetingCore: MeetingsCoreProviding {}

enum BackendRefineStatus: String, Equatable {
    case idle
    case submitting
    case polling
    case succeeded
    case failed
}

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
    private(set) var backendJobStatus: BackendRefineStatus = .idle
    private(set) var backendJobId = ""
    private(set) var backendArtifactMarkdown = ""

    private let core: any MeetingsCoreProviding
    private var backendRefineTask: Task<Void, Never>?
    private let maxPollAttempts: Int
    private let pollDelayNanoseconds: UInt64

    init(
        core: any MeetingsCoreProviding,
        maxPollAttempts: Int = 20,
        pollDelayNanoseconds: UInt64 = 250_000_000
    ) {
        self.core = core
        self.maxPollAttempts = maxPollAttempts
        self.pollDelayNanoseconds = pollDelayNanoseconds
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

    func submitBackendRefine(meetingId: String) {
        backendRefineTask?.cancel()
        backendRefineTask = Task { @MainActor [weak self] in
            await self?.performBackendRefine(meetingId: meetingId)
        }
    }

    func resetBackendRefineSession() {
        backendRefineTask?.cancel()
        backendRefineTask = nil
        backendJobStatus = .idle
        backendJobId = ""
        backendArtifactMarkdown = ""
    }

    func performBackendRefine(meetingId: String) async {
        guard finalTranscript != nil else {
            backendJobStatus = .failed
            errorMessage = "Нужен Final transcript"
            return
        }

        backendJobStatus = .submitting
        backendArtifactMarkdown = ""
        errorMessage = nil

        let job = core.submitBackendJob(meetingId: meetingId, kindCode: "refine")
        if !job.error.isEmpty {
            backendJobStatus = .failed
            backendJobId = job.id
            errorMessage = job.error
            return
        }

        backendJobId = job.id
        var current = job

        if current.status != "succeeded" {
            backendJobStatus = .polling
            var attempts = 0
            while attempts < maxPollAttempts {
                if Task.isCancelled {
                    return
                }
                if pollDelayNanoseconds > 0 {
                    try? await Task.sleep(nanoseconds: pollDelayNanoseconds)
                }
                if Task.isCancelled {
                    return
                }
                current = core.getBackendJob(jobId: current.id)
                if !current.error.isEmpty {
                    backendJobStatus = .failed
                    errorMessage = current.error
                    return
                }
                if current.status == "failed" {
                    backendJobStatus = .failed
                    errorMessage = current.error.isEmpty ? "Backend job failed" : current.error
                    return
                }
                if current.status == "succeeded" {
                    break
                }
                attempts += 1
            }
            if current.status != "succeeded" {
                backendJobStatus = .failed
                errorMessage = "Backend job timeout"
                return
            }
        }

        guard let artifactId = current.artifactIds.first else {
            backendJobStatus = .failed
            errorMessage = "Backend job has no artifacts"
            return
        }

        let artifact = core.getBackendArtifact(artifactId: artifactId)
        if !artifact.error.isEmpty {
            backendJobStatus = .failed
            errorMessage = artifact.error
            return
        }

        backendArtifactMarkdown = artifact.bodyMarkdown
        backendJobStatus = .succeeded
    }
}
