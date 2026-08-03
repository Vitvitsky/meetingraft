import Foundation
import Observation

/// Ошибка экспорта markdown (Swift Result требует Error, не String).
struct MarkdownExportFailure: Error, Equatable {
    let message: String
}

/// Результат экспорта markdown в папку.
struct MarkdownExportResult: Equatable {
    var writtenFileNames: [String]
    var folderPath: String
}

/// Контракт истории встреч для presentation model и тестов.
protocol MeetingsCoreProviding: AnyObject {
    func listMeetings() -> [FfiMeetingSummary]
    func listCaptions(meetingId: String) -> [FfiCaptionEvent]
    func getFinalTranscript(meetingId: String) -> FfiFinalTranscript
    func listArtifacts(meetingId: String) -> [FfiArtifact]
    func listSpeakers(meetingId: String) -> [FfiSpeaker]
    func upsertSpeaker(meetingId: String, id: String, displayName: String, sortIndex: Int64) -> String
    func deleteSpeaker(id: String) -> String
    func setApiConfig(baseUrl: String, token: String)
    func setLlmConfig(engineCode: String, modelId: String, baseUrl: String, providerId: String)
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
    private(set) var speakers: [FfiSpeaker] = []
    private(set) var selectedArtifact: FfiArtifact?
    private(set) var errorMessage: String?
    private(set) var backendJobStatus: BackendRefineStatus = .idle
    private(set) var backendJobId = ""
    private(set) var backendArtifactMarkdown = ""
    private(set) var exportStatusMessage = ""

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
        speakers = core.listSpeakers(meetingId: meetingId)
        if let selectedArtifact,
           let refreshed = artifacts.first(where: { $0.id == selectedArtifact.id })
        {
            self.selectedArtifact = refreshed
        } else {
            selectedArtifact = artifacts.first
        }
        errorMessage = nil
    }

    func addSpeaker(meetingId: String, primaryLanguage: String) {
        let number = speakers.count + 1
        let displayName = primaryLanguage == "ru"
            ? "Спикер \(number)"
            : "Speaker \(number)"
        let error = core.upsertSpeaker(
            meetingId: meetingId,
            id: "",
            displayName: displayName,
            sortIndex: Int64(speakers.count)
        )
        finishSpeakerMutation(error: error, meetingId: meetingId)
    }

    func renameSpeaker(meetingId: String, id: String, displayName: String) {
        guard let speaker = speakers.first(where: { $0.id == id }) else {
            return
        }
        let error = core.upsertSpeaker(
            meetingId: meetingId,
            id: id,
            displayName: displayName,
            sortIndex: speaker.sortIndex
        )
        finishSpeakerMutation(error: error, meetingId: meetingId)
    }

    func removeSpeaker(id: String, meetingId: String) {
        let error = core.deleteSpeaker(id: id)
        finishSpeakerMutation(error: error, meetingId: meetingId)
    }

    func applyProviderConfig(
        apiBaseUrl: String,
        apiToken: String,
        llmEngineCode: String,
        llmModelId: String,
        llmBaseUrl: String,
        llmProviderId: String
    ) {
        core.setApiConfig(baseUrl: apiBaseUrl, token: apiToken)
        core.setLlmConfig(
            engineCode: llmEngineCode,
            modelId: llmModelId,
            baseUrl: llmBaseUrl,
            providerId: llmProviderId
        )
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

    func exportMarkdown(
        meetingId: String,
        startedAtMs: UInt64,
        folderURL: URL
    ) -> Result<MarkdownExportResult, MarkdownExportFailure> {
        let transcript = core.getFinalTranscript(meetingId: meetingId)
        guard !transcript.meetingId.isEmpty else {
            let message = "Нужен Final transcript"
            exportStatusMessage = message
            return .failure(MarkdownExportFailure(message: message))
        }

        var writtenFileNames: [String] = []
        var writtenURLs: [URL] = []
        do {
            let finalName = MarkdownExport.fileName(
                startedAtMs: startedAtMs,
                meetingId: meetingId,
                kind: .final
            )
            let finalURL = try MarkdownExport.write(
                folderURL: folderURL,
                fileName: finalName,
                body: transcript.bodyMarkdown
            )
            writtenFileNames.append(finalName)
            writtenURLs.append(finalURL)

            let artifacts = core.listArtifacts(meetingId: meetingId)
            if let latestBrief = artifacts
                .filter({ $0.kind == .brief })
                .max(by: { $0.createdAtMs < $1.createdAtMs })
            {
                let briefName = MarkdownExport.fileName(
                    startedAtMs: startedAtMs,
                    meetingId: meetingId,
                    kind: .brief
                )
                let briefURL = try MarkdownExport.write(
                    folderURL: folderURL,
                    fileName: briefName,
                    body: latestBrief.bodyMarkdown
                )
                writtenFileNames.append(briefName)
                writtenURLs.append(briefURL)
            }
            if let latestFollowUp = artifacts
                .filter({ $0.kind == .followUp })
                .max(by: { $0.createdAtMs < $1.createdAtMs })
            {
                let followUpName = MarkdownExport.fileName(
                    startedAtMs: startedAtMs,
                    meetingId: meetingId,
                    kind: .followUp
                )
                let followUpURL = try MarkdownExport.write(
                    folderURL: folderURL,
                    fileName: followUpName,
                    body: latestFollowUp.bodyMarkdown
                )
                writtenFileNames.append(followUpName)
                writtenURLs.append(followUpURL)
            }

            let folderPath = folderURL.path
            let count = writtenFileNames.count
            let message = "Exported \(count) file\(count == 1 ? "" : "s") → \(folderPath)"
            exportStatusMessage = message
            return .success(
                MarkdownExportResult(
                    writtenFileNames: writtenFileNames,
                    folderPath: folderPath
                )
            )
        } catch {
            // При ошибке I/O откатываем файлы, уже записанные в этой попытке экспорта.
            for url in writtenURLs {
                try? FileManager.default.removeItem(at: url)
            }
            exportStatusMessage = error.localizedDescription
            return .failure(MarkdownExportFailure(message: error.localizedDescription))
        }
    }

    private func finishSpeakerMutation(error: String, meetingId: String) {
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        speakers = core.listSpeakers(meetingId: meetingId)
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
        if job.status == "failed" {
            backendJobStatus = .failed
            backendJobId = job.id
            errorMessage = job.error.isEmpty ? "Backend job failed" : job.error
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
