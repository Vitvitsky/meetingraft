import Foundation

/// Как состояние Apple-перевода выглядит в интерфейсе. Пустая колонка
/// обязана объяснять себя: молчаливый отказ хуже видимого.
extension TranslationHostState {
    /// Текст для пустой колонки перевода; `nil` — обычная заглушка.
    var columnMessage: String? {
        switch self {
        case .off, .ready:
            nil
        case .needsDownload:
            String(localized: "Apple translation needs a language download")
        case .downloading:
            String(localized: "Downloading language…")
        case .unsupported:
            String(localized: "This language pair is not supported")
        case let .failed(message):
            message
        }
    }

    /// Предлагать ли кнопку скачивания.
    var canDownload: Bool {
        switch self {
        case .needsDownload, .failed:
            true
        case .off, .ready, .downloading, .unsupported:
            false
        }
    }
}
