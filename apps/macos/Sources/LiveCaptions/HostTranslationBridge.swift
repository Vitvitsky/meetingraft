import Foundation

/// Host bridge для backend=`apple` / `auto→apple` (ADR-008).
/// Сейчас stub: помечает текст `[apple·host]`; позже — TranslationSession.
@MainActor
final class HostTranslationBridge {
    private let core: MeetingCore
    private var task: Task<Void, Never>?

    init(core: MeetingCore) {
        self.core = core
        core.setHostTranslationAvailable(available: true)
    }

    func start() {
        stop()
        task = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                let requests = self.core.drainHostTranslationRequests()
                for request in requests {
                    let translated = Self.stubTranslate(request)
                    _ = self.core.completeHostTranslation(
                        id: request.id,
                        translatedText: translated
                    )
                }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    /// Заглушка до подключения Translation framework.
    private static func stubTranslate(_ request: FfiHostTranslationRequest) -> String {
        "[\(request.targetCode)·apple] \(request.text)"
    }
}
