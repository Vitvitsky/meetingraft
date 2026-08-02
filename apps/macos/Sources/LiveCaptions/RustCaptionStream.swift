import Foundation

/// CaptionStreaming поверх UniFFI `MeetingCore` (источник — Rust).
final class RustCaptionStream: CaptionStreaming, @unchecked Sendable {
    private let core = MeetingCore()
    private var task: Task<Void, Never>?

    func start(onEvent: @escaping @MainActor (CaptionLine) -> Void) {
        stop()
        core.startDemo()
        task = Task { @MainActor in
            while !Task.isCancelled {
                let events = core.drainEvents()
                for event in events {
                    let phase: CaptionPhase = switch event.phase {
                    case .partial: .partial
                    case .final: .final
                    }
                    let id = UUID(uuidString: event.id) ?? UUID()
                    onEvent(CaptionLine(id: id, text: event.text, phase: phase))
                }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
        core.stop()
    }
}
