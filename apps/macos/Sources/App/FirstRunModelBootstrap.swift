import Foundation

/// Догрузка STT-модели при первом запуске.
///
/// Раньше это делал только `SettingsView.onAppear`: пользователь, который
/// не открывал настройки, нажимал Start Live и получал Mock-движок с
/// плейсхолдерами вместо речи. Теперь загрузка привязана к старту
/// приложения, а Settings лишь показывают статус.
@MainActor
final class FirstRunModelBootstrap {
    private let downloader: WhisperDownloading
    private var isRunning = false

    init(downloader: WhisperDownloading = WhisperModelDownloader()) {
        self.downloader = downloader
    }

    /// Скачать базовую модель, если каталог моделей пуст.
    /// Повторный вызов во время загрузки ничего не делает.
    @discardableResult
    func ensureModel(core: MeetingCore) async -> Bool {
        guard !isRunning else { return false }
        guard core.listLocalWhisperModels().isEmpty else { return false }

        isRunning = true
        defer { isRunning = false }

        let directory = URL(fileURLWithPath: core.modelsDirectory(), isDirectory: true)
        do {
            _ = try await downloader.download(id: .base, modelsDirectory: directory) { _ in }
            return true
        } catch {
            // Молча деградируем в Mock: экран live captions уже показывает
            // об этом плашку, а модалка на старте приложения избыточна.
            return false
        }
    }
}
