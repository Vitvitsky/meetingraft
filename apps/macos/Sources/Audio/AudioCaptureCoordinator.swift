import Foundation
import Observation

/// Координатор mic (+ system если available) → UniFFI ingest + drain live STT.
@Observable
@MainActor
final class AudioCaptureCoordinator {
    private(set) var isRecording = false
    private(set) var lastError: String?
    private(set) var systemAudioAvailable = false
    private(set) var sessionId: String?
    /// Обновляется при каждом успешном ingest — чтобы UI видел рост.
    private(set) var chunkCount: UInt64 = 0
    /// `idle` | `mock` | `whisper` после startRecording.
    private(set) var sttBackend: String = "idle"
    private(set) var captionEventCount: UInt64 = 0

    private let core: MeetingCore
    private let microphone = MicrophoneCapture()
    private let systemAudio = SystemAudioCapture()
    private var micPipeline = AudioChunkPipeline()
    private var systemPipeline = AudioChunkPipeline()
    private var startedAt: Date?

    init(core: MeetingCore) {
        self.core = core
    }

    init(dataRoot: String? = nil) {
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
        let granted = await AudioPermissions.requestMicrophone()
        guard granted else {
            lastError = "Доступ к микрофону запрещён"
            return
        }

        let id = UUID().uuidString
        let err = core.startRecording(sessionId: id)
        guard err.isEmpty else {
            lastError = err
            return
        }
        sessionId = id
        startedAt = Date()
        micPipeline.reset()
        systemPipeline.reset()
        sttBackend = core.sttBackend()
        // До start mic: иначе ранние буферы отбрасываются в ingest.
        isRecording = true

        systemAudio.prepare()
        systemAudioAvailable = systemAudio.isAvailable

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
            try? systemAudio.start { [weak self] samples in
                Task { @MainActor in
                    self?.ingest(samples: samples, channel: .system)
                }
            }
        }
    }

    func stopRecording() {
        microphone.stop()
        systemAudio.stop()
        core.stopRecording()
        isRecording = false
        startedAt = nil
        sttBackend = "idle"
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

    private func ingest(samples: [Float], channel: FfiAudioChannel) {
        guard isRecording, sessionId != nil, !samples.isEmpty else { return }
        let timestampMs = UInt64(max(0, Date().timeIntervalSince(startedAt ?? Date()) * 1000))
        let chunks: [Data] = switch channel {
        case .mic:
            micPipeline.push(samples: samples)
        case .system:
            systemPipeline.push(samples: samples)
        }
        for chunk in chunks {
            let err = core.ingestAudioChunk(
                channel: channel,
                pcm: chunk,
                sampleRate: UInt32(AudioChunkPipeline.targetSampleRate),
                timestampMs: timestampMs
            )
            if err.isEmpty {
                chunkCount += 1
            } else {
                lastError = err
            }
        }
    }
}
