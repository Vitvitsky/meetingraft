import Foundation

/// Скачивание русского движка GigaAM: четыре файла, ~230 МБ.
///
/// Отдельно от `WhisperModelDownloader`, потому что задача другая —
/// **комплект**, а не файл: прогресс общий, а недокачанный комплект
/// работать не будет. Сам транспорт при этом чужой (`FileDownloading`):
/// вторая реализация докачки и `.partial` была бы второй правдой об
/// одном и том же.
protocol GigaamDownloading: Sendable {
    func download(
        modelsDirectory: URL,
        progress: @escaping @MainActor (Double) -> Void
    ) async throws
}

struct GigaamModelDownloader: GigaamDownloading {
    private let files: FileDownloading

    init(files: FileDownloading = WhisperModelDownloader()) {
        self.files = files
    }

    func download(
        modelsDirectory: URL,
        progress: @escaping @MainActor (Double) -> Void
    ) async throws {
        let total = Double(GigaamModelCatalog.approximateTotalBytes)
        var completedBytes: Double = 0

        for file in GigaamModelFile.allCases {
            guard let sourceURL = GigaamModelCatalog.sourceURL(for: file) else {
                throw WhisperModelDownloaderError.notDownloadable
            }
            let destination = GigaamModelCatalog.destinationURL(
                modelsDirectory: modelsDirectory,
                file: file
            )
            let share = Double(file.approximateBytes)
            // Прогресс общий, а не «файл 2 из 4»: три файла из четырёх
            // весят меньше процента, и полоса по числу файлов
            // простаивала бы на энкодере, а потом прыгала до конца.
            let base = completedBytes
            try await files.downloadFile(from: sourceURL, to: destination) { fraction in
                progress((base + share * fraction) / total)
            }
            completedBytes += share

            // Метка пишется **после** файла и тем же форматом, что у
            // `scripts/fetch-gigaam-models.sh`. Без неё скрипт считает
            // файл неизвестным и качает 225 МБ заново; порядок важен,
            // потому что метка при недокачанном файле соврала бы.
            try? GigaamModelCatalog.markerContents(for: file).write(
                to: GigaamModelCatalog.markerURL(for: destination),
                atomically: true,
                encoding: .utf8
            )
        }

        await progress(1)
    }
}
