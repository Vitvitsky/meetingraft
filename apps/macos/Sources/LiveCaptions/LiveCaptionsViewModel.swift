import Foundation
import Observation

/// Presentation model экрана live captions (demo + live STT + optional translation).
@Observable
@MainActor
final class LiveCaptionsViewModel {
    private(set) var lines: [CaptionLine] = []
    private(set) var translationLines: [CaptionLine] = []
    private(set) var isLiveSession = false
    /// Начало live-сессии — для таймера в шапке.
    private(set) var sessionStartedAt: Date?
    private(set) var effectiveTranslationBackend: String = "off"
    /// Почему перевод не включился, хотя переключатель включён.
    private(set) var translationIssue = ""

    private let core: MeetingCore
    private let stream: RustCaptionStream
    private let hostBridge: HostTranslationBridge
    private var livePollTask: Task<Void, Never>?

    init(core: MeetingCore) {
        self.core = core
        stream = RustCaptionStream(core: core)
        hostBridge = HostTranslationBridge(core: core)
        hostBridge.start()
    }

    /// Прокинуть primary из Settings / toolbar в Rust STT/demo.
    func applySessionLanguage(_ language: SpeechLanguage) {
        let error = core.setSessionLanguage(primaryCode: language.rawValue)
        if !error.isEmpty {
            assertionFailure(error)
        }
    }

    /// ADR-008: enabled/target/backend → MeetingCore.
    func applyTranslationSettings(_ store: TranslationSettingsStore) {
        let backendError = core.setTranslationBackend(
            kindCode: store.backend.rawValue,
            baseUrl: store.backendBaseUrl
        )
        if !backendError.isEmpty {
            assertionFailure(backendError)
        }
        let enabled = store.enabled && store.backend != .off
        let liveError = core.setLiveTranslation(
            enabled: enabled,
            targetCode: store.target.rawValue
        )
        // Отказ ядра нельзя проглатывать: переключатель остаётся
        // включённым, перевода нет, и причина (чаще всего target равен
        // языку сессии) не видна нигде.
        translationIssue = enabled ? liveError : ""
        effectiveTranslationBackend = core.effectiveTranslationBackend()
    }

    /// Scripted demo captions (без аудио).
    func startDemo(translation: TranslationSettingsStore) {
        stopLivePoll()
        isLiveSession = false
        lines = []
        translationLines = []
        applyTranslationSettings(translation)
        stream.start(
            onEvent: { [weak self] line in
                self?.appendCaption(line)
            },
            onTranslation: { [weak self] line in
                self?.appendTranslation(line)
            }
        )
    }

    func stopDemo() {
        stream.stop()
    }

    /// ADR-005: выбранная ggml-модель → MeetingCore перед записью.
    func applySttModelPreference(_ store: ProviderSettingsStore) {
        core.setPreferredWhisperModel(modelId: store.selectedSttModelId.rawValue)
    }

    /// Recording + drainLiveCaptions с того же MeetingCore.
    func startLive(
        capture: AudioCaptureCoordinator,
        translation: TranslationSettingsStore,
        stt: ProviderSettingsStore
    ) async {
        stopDemo()
        stopLivePoll()
        lines = []
        translationLines = []
        applyTranslationSettings(translation)
        applySttModelPreference(stt)
        await capture.startRecording()
        guard capture.isRecording else { return }
        isLiveSession = true
        sessionStartedAt = Date()
        livePollTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                ingestLiveEvents(capture.drainLiveCaptions(), intoCaptions: true)
                ingestLiveEvents(core.drainLiveTranslations(), intoCaptions: false)
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }

    func stopLive(capture: AudioCaptureCoordinator) {
        stopLivePoll()
        ingestLiveEvents(capture.drainLiveCaptions(), intoCaptions: true)
        ingestLiveEvents(core.drainLiveTranslations(), intoCaptions: false)
        capture.stopRecording()
        ingestLiveEvents(capture.drainLiveCaptions(), intoCaptions: true)
        ingestLiveEvents(core.drainLiveTranslations(), intoCaptions: false)
        isLiveSession = false
        sessionStartedAt = nil
    }

    func stopAll(capture: AudioCaptureCoordinator) {
        stopDemo()
        if isLiveSession || capture.isRecording {
            stopLive(capture: capture)
        }
    }

    #if DEBUG
        /// Наполнение ленты в тестах без аудио и без Rust-потока.
        func ingestForTesting(text: String, phase: CaptionPhase) {
            appendCaption(CaptionLine(text: text, phase: phase))
        }
    #endif

    /// Последние строки для центрального блока: экран показывает речь,
    /// а не журнал событий. Полный лог живёт в истории встречи.
    func recentLines(limit: Int = 3) -> [CaptionLine] {
        Array(lines.suffix(limit))
    }

    private func stopLivePoll() {
        livePollTask?.cancel()
        livePollTask = nil
    }

    private func ingestLiveEvents(_ events: [FfiCaptionEvent], intoCaptions: Bool) {
        for event in events {
            let line = CaptionLine(event: event)
            if intoCaptions {
                appendCaption(line)
            } else {
                appendTranslation(line)
            }
        }
    }

    private func appendCaption(_ line: CaptionLine) {
        append(line, to: &lines)
    }

    private func appendTranslation(_ line: CaptionLine) {
        append(line, to: &translationLines)
    }

    private func append(_ line: CaptionLine, to lines: inout [CaptionLine]) {
        if line.phase == .final, let last = lines.last, last.phase == .partial {
            lines[lines.count - 1] = line
        } else if line.phase == .partial, let last = lines.last, last.phase == .partial {
            lines[lines.count - 1] = line
        } else {
            lines.append(line)
        }
    }
}
