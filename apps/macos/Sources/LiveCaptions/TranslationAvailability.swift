import Foundation
import Translation

/// Доступность языковой пары для Apple-перевода.
enum TranslationAvailabilityStatus: Equatable, Sendable {
    /// Пара поддерживается и уже скачана.
    case installed
    /// Пара поддерживается, но требует загрузки.
    case needsDownload
    /// Пара не поддерживается вовсе.
    case unsupported
}

/// Шов для тестов: `LanguageAvailability` не должна протекать в мост.
protocol TranslationAvailabilityChecking: Sendable {
    func status(source: SpeechLanguage, target: SpeechLanguage) async
        -> TranslationAvailabilityStatus
}

/// Настоящая проверка через Translation framework.
struct SystemTranslationAvailability: TranslationAvailabilityChecking {
    func status(source: SpeechLanguage, target: SpeechLanguage) async
        -> TranslationAvailabilityStatus
    {
        let availability = LanguageAvailability()
        let status = await availability.status(
            from: Locale.Language(identifier: source.rawValue),
            to: Locale.Language(identifier: target.rawValue)
        )
        switch status {
        case .installed: return .installed
        case .supported: return .needsDownload
        case .unsupported: return .unsupported
        @unknown default: return .unsupported
        }
    }
}
