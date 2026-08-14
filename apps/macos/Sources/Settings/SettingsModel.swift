import Foundation
import Observation

/// Состояние и операции окна настроек.
///
/// Раньше всё это жило в `SettingsView` как `@State` вместе с вёрсткой.
/// После разделения на разделы такое хранение означало бы протаскивание
/// десятка биндингов через каждый экран; заодно это чинит нарушение
/// `AGENTS.md` — во вью не место обращениям к ядру.
@Observable
@MainActor
final class SettingsModel {
    private(set) var modelPath = ""
    private(set) var modelsDirectory = ""
    private(set) var dataRoot = ""
    private(set) var localModels: [String] = []
    private(set) var downloadProgress: Double?
    private(set) var downloadError = ""
    private(set) var isDownloading = false
    private(set) var isTestingConnection = false
    private(set) var isRefreshingBackendModels = false

    // Чистка аудио (Epic 22). Предпросмотр отдельно от удаления: показ,
    // который удаляет, — худшее, что здесь можно построить.
    var audioSweepMonths = 6
    private(set) var audioSweepPreview: [FfiAudioSweepEntry] = []
    private(set) var audioSweepPreviewed = false
    private(set) var audioSweepReport = ""

    // Память на голоса (ADR-013, задача 7). Признак читается из ядра, а
    // не хранится здесь: настройки этого окна живут в памяти и умирают с
    // запуском, а биометрия обязана оставаться выключенной и после него.
    /// Собран ли движок голосов. Нет — раздела памяти на голоса не
    /// существует: запоминать было бы нечего и нечем.
    private(set) var voiceEngineAvailable = false
    private(set) var voiceMemoryEnabled = false
    private(set) var knownVoices: [FfiKnownVoice] = []
    private(set) var voiceMemoryError = ""

    private var core: MeetingCore?
    private let downloader: WhisperDownloading

    init(downloader: WhisperDownloading = WhisperModelDownloader()) {
        self.downloader = downloader
    }

    /// Движок, который реально поднимется при записи.
    var liveEngineLabel: String {
        modelPath.isEmpty
            ? String(localized: "Mock — model not installed")
            : String(localized: "Whisper, on-device")
    }

    var isModelReady: Bool {
        !modelPath.isEmpty
    }

    func isInstalled(_ modelId: WhisperModelId) -> Bool {
        guard let filename = modelId.filename else {
            return !localModels.isEmpty
        }
        return localModels.contains(filename)
    }

    /// Открыть ядро и подтянуть состояние моделей.
    func load(providerStore: ProviderSettingsStore) {
        let support = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first!
        let root = support.appendingPathComponent("meetingraft", isDirectory: true)
        dataRoot = root.path
        core = MeetingCore.withDataRoot(dataRoot: root.path)
        refreshModelPaths()
        refreshVoiceMemory()
        applySttPreference(providerStore)
        applyProviderConfig(providerStore)
    }

    func refreshVoiceMemory() {
        guard let core else { return }
        voiceEngineAvailable = core.isVoiceEngineAvailable()
        voiceMemoryEnabled = core.isVoiceMemoryEnabled()
        knownVoices = core.listKnownVoices()
    }

    /// Включить или выключить память на голоса.
    ///
    /// Выключение **забывает всех**, и человеку это сказано до нажатия, а
    /// не после: список исчезает у него на глазах, и объяснять постфактум
    /// уже поздно.
    func setVoiceMemory(enabled: Bool) {
        guard let core else { return }
        voiceMemoryError = core.setVoiceMemoryEnabled(enabled: enabled)
        refreshVoiceMemory()
    }

    func forgetVoice(id: String) {
        guard let core else { return }
        voiceMemoryError = core.forgetVoice(id: id)
        refreshVoiceMemory()
    }

    func refreshModelPaths() {
        guard let core else { return }
        modelPath = core.whisperModelPath()
        modelsDirectory = core.modelsDirectory()
        localModels = core.listLocalWhisperModels()
    }

    func applySttPreference(_ providerStore: ProviderSettingsStore) {
        core?.setPreferredWhisperModel(modelId: providerStore.selectedSttModelId.rawValue)
        refreshModelPaths()
    }

    func applyPostCallModel(_ modelId: WhisperModelId) {
        core?.setPostCallWhisperModel(modelId: modelId.rawValue)
    }

    /// Локальное ядро нужно для проверки API; запись применяет те же
    /// настройки к своему экземпляру.
    func applyProviderConfig(_ providerStore: ProviderSettingsStore) {
        core?.setApiConfig(baseUrl: providerStore.apiBaseUrl, token: providerStore.apiToken)
        core?.setLlmConfig(
            engineCode: providerStore.llmEngine.rawValue,
            modelId: providerStore.llmModelId,
            baseUrl: providerStore.llmBaseUrl,
            providerId: providerStore.llmProviderId
        )
    }

    func download(_ modelId: WhisperModelId, providerStore: ProviderSettingsStore) {
        guard let core, !isDownloading else { return }
        let directory = URL(fileURLWithPath: core.modelsDirectory(), isDirectory: true)
        isDownloading = true
        downloadError = ""
        downloadProgress = 0
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await downloader.download(id: modelId, modelsDirectory: directory) { value in
                    self.downloadProgress = value
                }
                refreshModelPaths()
                applySttPreference(providerStore)
            } catch {
                downloadError = Self.message(for: error)
            }
            isDownloading = false
            downloadProgress = nil
        }
    }

    var audioSweepTotalBytes: UInt64 {
        audioSweepPreview.reduce(0) { $0 + $1.bytes }
    }

    /// Что уйдёт при чистке. Ничего не удаляет.
    func previewAudioSweep() {
        guard let core else { return }
        audioSweepPreview = core.previewAudioSweep(olderThanMs: sweepThresholdMs())
        audioSweepPreviewed = true
        audioSweepReport = ""
    }

    /// Удалить то, что показал предпросмотр.
    func runAudioSweep() {
        guard let core else { return }
        let result = core.runAudioSweep(olderThanMs: sweepThresholdMs())
        let freed = ByteCountFormatter.string(
            fromByteCount: Int64(result.freedBytes),
            countStyle: .file
        )
        var report = String(
            localized: "Удалено записей: \(result.deletedCount), освобождено \(freed)"
        )
        if !result.skipped.isEmpty {
            // Пропуски называются вслух: молчание сделало бы число
            // удалённых враньём.
            report += "\n" + String(localized: "Пропущено: ") + result.skipped.joined(separator: ", ")
        }
        audioSweepReport = report
        previewAudioSweep()
    }

    /// Порог в абсолютном времени. Календарь живёт здесь, а не в ядре.
    private func sweepThresholdMs() -> UInt64 {
        let cutoff = Calendar.current.date(
            byAdding: .month,
            value: -audioSweepMonths,
            to: Date()
        ) ?? Date()
        return UInt64(max(0, cutoff.timeIntervalSince1970 * 1000))
    }

    /// Сеть блокирует поток на весь таймаут, поэтому запрос уходит с
    /// главного. Пока он идёт, интерфейс больше не замирает — значит
    /// ожидание надо показать явно, иначе кнопка выглядит мёртвой.
    func testApiConnection(_ providerStore: ProviderSettingsStore) async {
        applyProviderConfig(providerStore)
        guard let core else { return }
        isTestingConnection = true
        defer { isTestingConnection = false }
        let error = await offMainThread { core.testApiConnection() }
        providerStore.apiConnectionOk = error.isEmpty
        providerStore.apiConnectionMessage = error.isEmpty ? "GET /health OK" : error
    }

    func refreshBackendLlmModels(_ providerStore: ProviderSettingsStore) async {
        applyProviderConfig(providerStore)
        guard let core else { return }
        isRefreshingBackendModels = true
        defer { isRefreshingBackendModels = false }
        let models = await offMainThread { core.listBackendLlmModels() }
        // FFI отображает синхронную ошибку в пустой список, поэтому сбой
        // от честного «моделей нет» отличаем отдельной проверкой здоровья.
        if models.isEmpty {
            let connectionError = await offMainThread { core.testApiConnection() }
            if !connectionError.isEmpty {
                providerStore.applyBackendModelsCatalog([], connectionError: connectionError)
                return
            }
        }
        providerStore.applyBackendModelsCatalog(models)
    }

    static func message(for error: Error) -> String {
        switch error {
        case WhisperModelDownloaderError.notDownloadable:
            String(localized: "This option has no file to download.")
        case let WhisperModelDownloaderError.downloadFailed(statusCode):
            if let statusCode {
                String(localized: "Download failed: HTTP \(statusCode).")
            } else {
                String(localized: "Download from Hugging Face failed.")
            }
        default:
            String(localized: "Download failed: \(error.localizedDescription)")
        }
    }
}
