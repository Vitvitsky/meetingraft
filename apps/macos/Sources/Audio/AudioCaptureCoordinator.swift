import Foundation
import Observation

/// Координатор mic (+ system если available) → UniFFI ingest.
@Observable
@MainActor
final class AudioCaptureCoordinator {
    private(set) var isRecording = false
    private(set) var lastError: String?
    private(set) var systemAudioAvailable = false
    private(set) var sessionId: String?

    private let core: MeetingCore
    private let microphone = MicrophoneCapture()
    private let systemAudio = SystemAudioCapture()
    private var micPipeline = AudioChunkPipeline()
    private var systemPipeline = AudioChunkPipeline()
    private var startedAt: Date?

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

    /// Старт recording: permission → Rust session → taps.
    func startRecording() async {
        lastError = nil
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
            core.stopRecording()
            sessionId = nil
            return
        }

        if systemAudioAvailable {
            try? systemAudio.start { [weak self] samples in
                Task { @MainActor in
                    self?.ingest(samples: samples, channel: .system)
                }
            }
        }

        isRecording = true
    }

    func stopRecording() {
        microphone.stop()
        systemAudio.stop()
        core.stopRecording()
        isRecording = false
        startedAt = nil
    }

    /// Сбросить сообщение об ошибке (UI alert).
    func clearError() {
        lastError = nil
    }

    func manifestChunkCount() -> UInt64 {
        guard let sessionId else { return 0 }
        return core.manifestChunkCount(sessionId: sessionId)
    }

    private func ingest(samples: [Float], channel: FfiAudioChannel) {
        guard isRecording else { return }
        let timestampMs = UInt64(max(0, (startedAt ?? Date()).timeIntervalSinceNow * -1000))
        var chunks: [Data] = []
        switch channel {
        case .mic:
            chunks = micPipeline.push(samples: samples)
        case .system:
            chunks = systemPipeline.push(samples: samples)
        }
        for chunk in chunks {
            let err = core.ingestAudioChunk(
                channel: channel,
                pcm: chunk,
                sampleRate: UInt32(AudioChunkPipeline.targetSampleRate),
                timestampMs: timestampMs
            )
            if !err.isEmpty {
                lastError = err
            }
        }
    }
}
