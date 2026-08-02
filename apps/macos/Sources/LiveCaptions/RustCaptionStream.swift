import Foundation

/// CaptionStreaming поверх UniFFI `MeetingCore` (источник — Rust).
final class RustCaptionStream: CaptionStreaming, @unchecked Sendable {
    private let core: MeetingCore
    private var task: Task<Void, Never>?

    init(core: MeetingCore = MeetingCore()) {
        self.core = core
    }

    func start(
        onEvent: @escaping @MainActor (CaptionLine) -> Void,
        onTranslation: (@MainActor (CaptionLine) -> Void)? = nil
    ) {
        stop()
        core.startDemo()
        task = Task { @MainActor in
            while !Task.isCancelled {
                let events = core.drainEvents()
                for event in events {
                    onEvent(Self.map(event))
                }
                if let onTranslation {
                    for event in core.drainLiveTranslations() {
                        onTranslation(Self.map(event))
                    }
                }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }

    func start(onEvent: @escaping @MainActor (CaptionLine) -> Void) {
        start(onEvent: onEvent, onTranslation: nil)
    }

    func stop() {
        task?.cancel()
        task = nil
        core.stop()
    }

    private static func map(_ event: FfiCaptionEvent) -> CaptionLine {
        let phase: CaptionPhase = switch event.phase {
        case .partial: .partial
        case .final: .final
        }
        let id = UUID(uuidString: event.id) ?? UUID()
        return CaptionLine(id: id, text: event.text, phase: phase)
    }
}
