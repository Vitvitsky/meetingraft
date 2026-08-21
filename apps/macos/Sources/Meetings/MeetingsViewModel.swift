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
///
/// `Sendable` — потому что сетевые методы вызываются с фонового потока:
/// иначе они блокируют главный на весь таймаут запроса.
protocol MeetingsCoreProviding: AnyObject, Sendable {
    func listMeetings() -> [FfiMeetingSummary]
    func renameMeeting(meetingId: String, title: String) -> String
    func deleteMeeting(meetingId: String) -> String
    func deleteMeetingAudio(meetingId: String) -> String
    func meetingAudioBytes(meetingId: String) -> UInt64
    func previewAudioSweep(olderThanMs: UInt64) -> [FfiAudioSweepEntry]
    func runAudioSweep(olderThanMs: UInt64) -> FfiAudioSweepResult
    func searchMeetings(query: String, limit: UInt32) -> [FfiSearchHit]
    func listCaptions(meetingId: String) -> [FfiCaptionEvent]
    func getFinalTranscript(meetingId: String) -> FfiFinalTranscript
    func listFinalTranscripts(meetingId: String) -> [FfiFinalTranscript]
    func getFinalTranscriptVersion(meetingId: String, version: UInt32) -> FfiFinalTranscript
    func listArtifacts(meetingId: String) -> [FfiArtifact]
    func setApiConfig(baseUrl: String, token: String)
    func setLlmConfig(engineCode: String, modelId: String, baseUrl: String, providerId: String)
    func generateArtifact(meetingId: String, kind: FfiArtifactKind) -> FfiGenerateArtifactResult
}

extension MeetingCore: MeetingsCoreProviding {}

/// Presentation model локальной истории и post-call артефактов.
@Observable
@MainActor
final class MeetingsViewModel {
    private(set) var meetings: [FfiMeetingSummary] = []
    private(set) var captions: [FfiCaptionEvent] = []
    /// Latest Final — Brief и экспорт опираются только на него.
    private(set) var finalTranscript: FfiFinalTranscript?
    /// Все версии Final (новые первыми); UI picker / Compare.
    private(set) var finalVersions: [FfiFinalTranscript] = []
    /// Выбранная версия для просмотра; `nil` → трактуем как latest.
    var selectedFinalVersion: UInt32?
    private(set) var artifacts: [FfiArtifact] = []
    private(set) var selectedArtifact: FfiArtifact?
    private(set) var errorMessage: String?
    private(set) var isGeneratingArtifact = false
    private(set) var exportStatusMessage = ""
    /// Строка поиска; пустая — показываем полный список.
    var query = "" {
        didSet {
            guard query != oldValue else { return }
            scheduleSearch()
        }
    }

    private(set) var searchHits: [FfiSearchHit] = []
    private(set) var isSearching = false
    /// Фильтр по времени; на поиск не влияет — искать логично по всему.
    var filter: MeetingsFilter = .all

    private let core: any MeetingsCoreProviding
    private var searchTask: Task<Void, Never>?
    private let searchDebounceNanoseconds: UInt64

    /// Версия, которую сейчас показывает экран Final.
    ///
    /// `nil` в `selectedFinalVersion` означает «последняя» — так его и
    /// читает тело транскрипта. Толковать тот же `nil` как «версии нет»
    /// нельзя: экран показал бы текст последней версии, а сегменты к
    /// нему не подгрузились бы, и правка с прослушиванием выглядели бы
    /// отключившимися при живых данных. Правило одно на оба чтения
    /// именно поэтому.
    var effectiveFinalVersion: UInt32? {
        selectedFinalVersion ?? finalVersions.first?.version
    }

    /// Тело выбранной версии Final (кэш списка или latest).
    var selectedFinalBody: String {
        if let version = effectiveFinalVersion,
           let match = finalVersions.first(where: { $0.version == version })
        {
            return match.bodyMarkdown
        }
        return finalTranscript?.bodyMarkdown ?? ""
    }

    init(
        core: any MeetingsCoreProviding,
        searchDebounceNanoseconds: UInt64 = 200_000_000
    ) {
        self.core = core
        self.searchDebounceNanoseconds = searchDebounceNanoseconds
    }

    func reload() {
        meetings = core.listMeetings()
        errorMessage = nil
    }

    /// Переименовать встречу; список обновляется только при успехе.
    func rename(meetingId: String, title: String) {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let error = core.renameMeeting(meetingId: meetingId, title: trimmed)
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        reload()
    }

    /// Удалить запись встречи, оставив транскрипт и всё остальное.
    ///
    /// Отдельно от `delete`: транскрипт нужен всегда, а запись
    /// полугодовой давности — почти никогда (Epic 22).
    func deleteAudio(meetingId: String) {
        let error = core.deleteMeetingAudio(meetingId: meetingId)
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        reload()
    }

    /// Сколько занимает запись на диске; 0 — записи нет.
    func audioBytes(meetingId: String) -> UInt64 {
        core.meetingAudioBytes(meetingId: meetingId)
    }

    /// Удалить встречу со всеми материалами.
    func delete(meetingId: String) {
        let error = core.deleteMeeting(meetingId: meetingId)
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        if !query.isEmpty {
            searchHits.removeAll { $0.meetingId == meetingId }
        }
        reload()
    }

    /// Дебаунс: пользователь печатает быстрее, чем имеет смысл искать.
    private func scheduleSearch() {
        searchTask?.cancel()
        let text = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            searchHits = []
            isSearching = false
            return
        }
        isSearching = true
        searchTask = Task { @MainActor [weak self] in
            guard let self else { return }
            try? await Task.sleep(nanoseconds: searchDebounceNanoseconds)
            guard !Task.isCancelled else { return }
            searchHits = core.searchMeetings(query: text, limit: 50)
            isSearching = false
        }
    }

    /// Встречи под текущим фильтром.
    func filteredMeetings(now: Date = Date()) -> [FfiMeetingSummary] {
        meetings.filter { filter.matches(startedAtMs: $0.startedAtMs, now: now) }
    }

    /// Сколько встреч попадает в фильтр — число рядом с его названием.
    func count(for filter: MeetingsFilter, now: Date = Date()) -> Int {
        meetings.count { filter.matches(startedAtMs: $0.startedAtMs, now: now) }
    }

    /// Название для показа: пустое поле заменяется датой встречи.
    func displayTitle(for meeting: FfiMeetingSummary) -> String {
        let trimmed = meeting.title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.isEmpty else { return trimmed }
        return MeetingTitle.fallback(startedAtMs: meeting.startedAtMs)
    }

    /// Длительность завершённой встречи; `nil` пока идёт запись.
    func duration(for meeting: FfiMeetingSummary) -> Duration? {
        guard meeting.endedAtMs > meeting.startedAtMs else { return nil }
        return .milliseconds(meeting.endedAtMs - meeting.startedAtMs)
    }

    func reload(meetingId: String) {
        captions = core.listCaptions(meetingId: meetingId)

        let transcript = core.getFinalTranscript(meetingId: meetingId)
        finalTranscript = transcript.meetingId.isEmpty ? nil : transcript
        finalVersions = core.listFinalTranscripts(meetingId: meetingId)
        selectedFinalVersion = finalTranscript?.version

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

    /// Join raw live caption finals — колонка Compare, без line-diff.
    func liveFinalsText(from captions: [FfiCaptionEvent]) -> String {
        captions.filter { $0.phase == .final }.map(\.text).joined(separator: "\n\n")
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

    /// LLM отвечает до минуты, поэтому вызов уходит с главного потока.
    /// Ожидание видно по `isGeneratingArtifact` — без этого кнопка молчит.
    func generate(meetingId: String, kind: FfiArtifactKind) async {
        // Ссылка для фонового потока: свойство изолировано `@MainActor`,
        // а замыкание — нет, поэтому читаем его здесь.
        let backgroundCore = core
        isGeneratingArtifact = true
        defer { isGeneratingArtifact = false }
        let result = await offMainThread {
            backgroundCore.generateArtifact(meetingId: meetingId, kind: kind)
        }
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
            let message = String(localized: "A Final transcript is required")
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
}
