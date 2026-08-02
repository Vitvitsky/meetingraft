import Foundation

/// Контракт источника captions для UI (Phase 2 — Rust facade).
protocol CaptionStreaming: AnyObject {
    func start(onEvent: @escaping @MainActor (CaptionLine) -> Void)
    func stop()
}

/// Скриптованный fake stream на Task.sleep.
final class FakeCaptionStream: CaptionStreaming, @unchecked Sendable {
    private let script: [CaptionLine]
    private let tickNanoseconds: UInt64
    private var task: Task<Void, Never>?

    init(script: [CaptionLine]? = nil, tickNanoseconds: UInt64 = 800_000_000) {
        self.script = script ?? Self.defaultScript
        self.tickNanoseconds = tickNanoseconds
    }

    func start(onEvent: @escaping @MainActor (CaptionLine) -> Void) {
        stop()
        let script = script
        let tick = tickNanoseconds
        task = Task { @MainActor in
            for line in script {
                if Task.isCancelled {
                    return
                }
                onEvent(line)
                try? await Task.sleep(nanoseconds: tick)
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    private static let defaultScript: [CaptionLine] = [
        .init(text: "Добро пожаловать", phase: .partial),
        .init(text: "Добро пожаловать в MeetingRaft", phase: .final),
        .init(text: "Язык сессии — русский", phase: .partial),
        .init(text: "Язык сессии — русский по умолчанию", phase: .final),
        .init(text: "English terms are fine", phase: .partial),
        .init(text: "English terms are fine in mixed meetings", phase: .final),
    ]
}
