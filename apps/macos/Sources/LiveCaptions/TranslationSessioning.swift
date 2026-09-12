@preconcurrency import Translation

/// Шов для тестов: мост не должен зависеть от Apple-фреймворка напрямую.
@MainActor
protocol TranslationSessioning: AnyObject {
    func translate(_ text: String) async throws -> String
    func prepareTranslation() async throws
}

/// Настоящая сессия из `.translationTask`.
///
/// Живёт только внутри замыкания `.translationTask`: Apple бросает
/// `fatalError`, если обратиться к ней после исчезновения вью или смены
/// конфигурации. Адаптер создаётся там же и умирает вместе с замыканием.
@MainActor
final class SystemTranslationSession: TranslationSessioning {
    private let session: TranslationSession

    init(_ session: TranslationSession) {
        self.session = session
    }

    func translate(_ text: String) async throws -> String {
        try await session.translate(text).targetText
    }

    func prepareTranslation() async throws {
        try await session.prepareTranslation()
    }
}
