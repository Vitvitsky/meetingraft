import Foundation
import Observation

/// Presentation model экрана live captions (demo + live STT).
@Observable
@MainActor
final class LiveCaptionsViewModel {
    private(set) var lines: [CaptionLine] = []
    private(set) var isLiveSession = false
    private let stream: CaptionStreaming
    private var livePollTask: Task<Void, Never>?

    init(stream: CaptionStreaming = RustCaptionStream()) {
        self.stream = stream
    }

    /// Scripted demo captions (без аудио).
    func startDemo() {
        stopLivePoll()
        isLiveSession = false
        lines = []
        stream.start { [weak self] line in
            self?.append(line)
        }
    }

    func stopDemo() {
        stream.stop()
    }

    /// Recording + drainLiveCaptions с того же MeetingCore.
    func startLive(capture: AudioCaptureCoordinator) async {
        stopDemo()
        stopLivePoll()
        lines = []
        await capture.startRecording()
        guard capture.isRecording else { return }
        isLiveSession = true
        livePollTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                let events = capture.drainLiveCaptions()
                for event in events {
                    let phase: CaptionPhase = switch event.phase {
                    case .partial: .partial
                    case .final: .final
                    }
                    let id = UUID(uuidString: event.id) ?? UUID()
                    append(CaptionLine(id: id, text: event.text, phase: phase))
                }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }

    func stopLive(capture: AudioCaptureCoordinator) {
        stopLivePoll()
        // Дочитать flush после stop.
        let events = capture.drainLiveCaptions()
        for event in events {
            let phase: CaptionPhase = switch event.phase {
            case .partial: .partial
            case .final: .final
            }
            let id = UUID(uuidString: event.id) ?? UUID()
            append(CaptionLine(id: id, text: event.text, phase: phase))
        }
        capture.stopRecording()
        // Ещё раз после stopRecording (Rust flush в очередь до clear).
        let after = capture.drainLiveCaptions()
        for event in after {
            let phase: CaptionPhase = switch event.phase {
            case .partial: .partial
            case .final: .final
            }
            let id = UUID(uuidString: event.id) ?? UUID()
            append(CaptionLine(id: id, text: event.text, phase: phase))
        }
        isLiveSession = false
    }

    func stopAll(capture: AudioCaptureCoordinator) {
        stopDemo()
        if isLiveSession || capture.isRecording {
            stopLive(capture: capture)
        }
    }

    private func stopLivePoll() {
        livePollTask?.cancel()
        livePollTask = nil
    }

    private func append(_ line: CaptionLine) {
        if line.phase == .final, let last = lines.last, last.phase == .partial {
            lines[lines.count - 1] = line
        } else if line.phase == .partial, let last = lines.last, last.phase == .partial {
            lines[lines.count - 1] = line
        } else {
            lines.append(line)
        }
    }
}
