import Foundation
import Observation

/// Координатор mic (+ system если available) → UniFFI ingest + drain live STT.
@Observable
@MainActor
final class AudioCaptureCoordinator {
    private(set) var isRecording = false
    private(set) var lastError: String?
    private(set) var systemAudioAvailable = false
    /// Почему системный звук недоступен — для actionable-состояния в UI.
    private(set) var systemAudioStatus: SystemAudioStatus = .unknown
    private(set) var sessionId: String?
    /// Обновляется при каждом успешном ingest — чтобы UI видел рост.
    private(set) var chunkCount: UInt64 = 0
    /// `idle` | `mock` | `whisper` после startRecording.
    private(set) var sttBackend: String = "idle"
    private(set) var captionEventCount: UInt64 = 0
    /// Сглаженный уровень микрофона, 0…1 — для индикатора на экране.
    ///
    /// Считается из тех же сэмплов, что уходят в распознавание, поэтому
    /// показывает реальный вход, а не анимацию.
    private(set) var inputLevel: Double = 0

    private let core: MeetingCore
    private let microphone: any AudioTapping
    private let systemAudio: any AudioTapping
    private var micPipeline = AudioChunkPipeline()
    private var systemPipeline = AudioChunkPipeline()

    /// Запрос разрешения вынесен в зависимость: в тестовом бандле
    /// системный промпт недоступен и подвесил бы тест.
    private let requestMicrophonePermission: @Sendable () async -> Bool

    init(
        core: MeetingCore,
        microphone: any AudioTapping = MicrophoneCapture(),
        systemAudio: any AudioTapping = SystemAudioCapture(),
        requestMicrophonePermission: @escaping @Sendable () async -> Bool = {
            await AudioPermissions.requestMicrophone()
        }
    ) {
        self.core = core
        self.microphone = microphone
        self.systemAudio = systemAudio
        self.requestMicrophonePermission = requestMicrophonePermission
    }

    init(dataRoot: String? = nil) {
        microphone = MicrophoneCapture()
        systemAudio = SystemAudioCapture()
        requestMicrophonePermission = { await AudioPermissions.requestMicrophone() }
        if let dataRoot {
            core = MeetingCore.withDataRoot(dataRoot: dataRoot)
        } else {
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            let root = support.appendingPathComponent("meetingraft", isDirectory: true)
            try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            core = MeetingCore.withDataRoot(dataRoot: root.path)
        }
    }

    /// Путь к ggml-модели (пусто если нет).
    var whisperModelPath: String {
        core.whisperModelPath()
    }

    var modelsDirectory: String {
        core.modelsDirectory()
    }

    /// Старт recording: permission → Rust session → taps.
    func startRecording() async {
        lastError = nil
        chunkCount = 0
        captionEventCount = 0
        sttBackend = "idle"
        let granted = await requestMicrophonePermission()
        guard granted else {
            lastError = "Доступ к микрофону запрещён"
            return
        }

        let id = UUID().uuidString
        let err = core.startRecording(sessionId: id, title: MeetingTitle.forNewMeeting())
        guard err.isEmpty else {
            lastError = err
            return
        }
        sessionId = id
        micPipeline.reset()
        systemPipeline.reset()
        sttBackend = core.sttBackend()
        // До start mic: иначе ранние буферы отбрасываются в ingest.
        isRecording = true

        systemAudio.prepare()
        systemAudioAvailable = systemAudio.isAvailable
        systemAudioStatus = (systemAudio as? SystemAudioCapture)?.status ?? .unknown
        // Пока tap не запущен, микшер не должен ждать системный канал.
        core.setSystemAudioExpected(expected: false)

        do {
            try microphone.start { [weak self] samples in
                Task { @MainActor in
                    self?.ingest(samples: samples, channel: .mic)
                }
            }
        } catch {
            lastError = "Не удалось запустить микрофон: \(error.localizedDescription)"
            microphone.stop()
            core.stopRecording()
            sessionId = nil
            isRecording = false
            sttBackend = "idle"
            return
        }

        if systemAudioAvailable {
            do {
                try systemAudio.start { [weak self] samples in
                    Task { @MainActor in
                        self?.ingest(samples: samples, channel: .system)
                    }
                }
                core.setSystemAudioExpected(expected: true)
            } catch {
                systemAudioAvailable = false
                systemAudioStatus = (systemAudio as? SystemAudioCapture)?.status ?? .unsupported
            }
        }
    }

    func stopRecording() {
        microphone.stop()
        systemAudio.stop()
        core.stopRecording()
        core.setSystemAudioExpected(expected: false)
        isRecording = false
        sttBackend = "idle"
        inputLevel = 0
    }

    /// Live STT events с того же MeetingCore, что и ingest.
    func drainLiveCaptions() -> [FfiCaptionEvent] {
        let events = core.drainLiveCaptions()
        if let sessionId {
            captionEventCount = core.captionEventCount(sessionId: sessionId)
        }
        return events
    }

    /// Сбросить сообщение об ошибке (UI alert).
    func clearError() {
        lastError = nil
    }

    /// Насколько быстро индикатор следует за звуком. Ниже — плавнее, но
    /// заметно отстаёт от речи.
    private static let levelSmoothing = 0.3
    /// Опорный уровень: обычная речь около этого значения RMS.
    private static let levelReference = 0.15

    private func updateLevel(samples: [Float]) {
        let sum = samples.reduce(0.0) { $0 + Double($1) * Double($1) }
        let rms = (sum / Double(samples.count)).squareRoot()
        let normalized = min(1, rms / Self.levelReference)
        inputLevel += (normalized - inputLevel) * Self.levelSmoothing
    }

    private func ingest(samples: [Float], channel: FfiAudioChannel) {
        guard isRecording, sessionId != nil, !samples.isEmpty else { return }
        if channel == .mic {
            updateLevel(samples: samples)
        }
        let chunks: [AudioChunk] = switch channel {
        case .mic:
            micPipeline.push(samples: samples)
        case .system:
            systemPipeline.push(samples: samples)
        }
        for chunk in chunks {
            let err = core.ingestAudioChunk(
                channel: channel,
                pcm: chunk.data,
                sampleRate: UInt32(AudioChunkPipeline.targetSampleRate),
                timestampMs: chunk.timestampMs()
            )
            if err.isEmpty {
                chunkCount += 1
            } else {
                lastError = err
            }
        }
    }
}
