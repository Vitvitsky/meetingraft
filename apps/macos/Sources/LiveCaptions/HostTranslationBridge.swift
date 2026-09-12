import Foundation
import Observation

/// Что видно в UI про Apple-перевод.
enum TranslationHostState: Equatable, Sendable {
    case off
    case ready
    case needsDownload
    case downloading
    case unsupported
    case failed(String)
}

/// Мост к Apple Translation (ADR-008).
///
/// Сессию не хранит: цикл живёт внутри замыкания `.translationTask`
/// (`run(session:)`) ровно столько, сколько валидна сессия. Хранить её
/// дольше нельзя — Apple бросает `fatalError` при обращении к мёртвой.
@Observable
@MainActor
final class HostTranslationBridge {
    private(set) var state: TranslationHostState = .off
    /// Есть ли живая сессия: без неё скачать пару нечем.
    private(set) var hasSession = false

    private let availability: TranslationAvailabilityChecking
    private var queue: TranslationHostQueue?
    private var downloadRequested = false
    private var targetCode = ""
    private var installed = false

    init(availability: TranslationAvailabilityChecking = SystemTranslationAvailability()) {
        self.availability = availability
    }

    /// Главное окно отдаёт свою очередь: мост создан на уровне App, чтобы
    /// его видела и Settings-сцена, а ядро живёт в окне.
    func bind(queue: TranslationHostQueue) {
        self.queue = queue
    }

    /// Кнопка «скачать» только ставит флаг: работу с сессией делает цикл,
    /// у которого сессия жива.
    func requestDownload() {
        downloadRequested = true
    }

    /// Настроить пару и спросить доступность. Вынесено из `run`, чтобы
    /// проверяться тестом без бесконечного цикла.
    func prepare(source: SpeechLanguage, target: SpeechLanguage) async {
        targetCode = target.rawValue
        let status = await availability.status(source: source, target: target)
        installed = status == .installed
        state = Self.displayState(for: status)
        queue?.setHostTranslationAvailable(available: installed)
    }

    /// Цикл живёт, пока живёт замыкание `.translationTask`.
    func run(session: TranslationSessioning, source: SpeechLanguage, target: SpeechLanguage) async {
        hasSession = true
        await prepare(source: source, target: target)
        defer {
            hasSession = false
            installed = false
            queue?.setHostTranslationAvailable(available: false)
            drainAndDropStale()
            state = .off
        }
        while !Task.isCancelled {
            if downloadRequested {
                downloadRequested = false
                await performDownload(session: session, source: source, target: target)
            }
            if installed {
                await pumpOnce(session: session)
            }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    /// Один проход: забрать запросы, перевести, вернуть. Цикл вокруг него
    /// только спит, поэтому он и тестируется.
    func pumpOnce(session: TranslationSessioning) async {
        guard let queue else { return }
        for request in queue.drainHostTranslationRequests() {
            guard request.targetCode == targetCode else {
                // Стухший запрос: сессия настроена на другую пару.
                complete(id: request.id, translatedText: "")
                continue
            }
            do {
                let translated = try await session.translate(request.text)
                if complete(id: request.id, translatedText: translated) {
                    state = .ready
                }
            } catch {
                // Реплика теряется, но очередь не растёт и следующая живёт.
                if complete(id: request.id, translatedText: "") {
                    state = .failed(error.localizedDescription)
                }
            }
        }
    }

    /// Скачать пару через живую сессию и пересчитать доступность.
    func performDownload(
        session: TranslationSessioning,
        source: SpeechLanguage,
        target: SpeechLanguage
    ) async {
        state = .downloading
        do {
            try await session.prepareTranslation()
            await prepare(source: source, target: target)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// Снять с очереди всё, что осталось от прошлой пары.
    private func drainAndDropStale() {
        guard let queue else { return }
        for request in queue.drainHostTranslationRequests() {
            // Отказ здесь тут же затёрло бы `.off` в `defer` `run`: снятие
            // стухшего запроса не требует действия от человека.
            complete(id: request.id, translatedText: "")
        }
    }

    /// Завершить запрос в ядре и не проглотить его отказ: пустая строка —
    /// успех, непустая — текст ошибки. `false` значит, что ядро отказало и
    /// `state` уже `.failed`.
    @discardableResult
    private func complete(id: String, translatedText: String) -> Bool {
        guard let queue else { return true }
        let error = queue.completeHostTranslation(id: id, translatedText: translatedText)
        guard error.isEmpty else {
            state = .failed(error)
            return false
        }
        return true
    }

    private static func displayState(for status: TranslationAvailabilityStatus) -> TranslationHostState {
        switch status {
        case .installed: .ready
        case .needsDownload: .needsDownload
        case .unsupported: .unsupported
        }
    }
}
