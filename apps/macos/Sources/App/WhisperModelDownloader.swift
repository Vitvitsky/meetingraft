import Foundation

enum WhisperModelDownloaderError: Error, Equatable {
    /// `auto` и другие id без файла на диске.
    case notDownloadable
    /// HTTP или транспорт URLSession.
    case downloadFailed(statusCode: Int?)
}

/// Скачивание ggml Whisper с Hugging Face в каталог `models/`.
protocol WhisperDownloading: Sendable {
    func download(
        id: WhisperModelId,
        modelsDirectory: URL,
        progress: @escaping @MainActor (Double) -> Void
    ) async throws -> URL
}

struct WhisperModelDownloader: WhisperDownloading {
    /// `(sourceURL, partialDestination, onProgress)` — для unit-тестов без live HF.
    private let downloadTransport: @Sendable (URL, URL, @Sendable @escaping (Double) -> Void) async throws -> Void
    private let session: URLSession

    init(
        session: URLSession = .shared,
        downloadTransport: (
            @Sendable (URL, URL, @Sendable @escaping (Double) -> Void) async throws -> Void
        )? = nil
    ) {
        self.session = session
        self.downloadTransport = downloadTransport ?? { sourceURL, partialURL, onProgress in
            try await Self.defaultDownloadTransport(
                session: session,
                from: sourceURL,
                to: partialURL,
                onProgress: onProgress
            )
        }
    }

    /// Абсолютный путь финального файла; `nil` для `auto`.
    static func destinationURL(modelsDirectory: URL, id: WhisperModelId) -> URL? {
        guard let filename = id.filename else { return nil }
        return modelsDirectory.appendingPathComponent(filename, isDirectory: false)
    }

    /// Промежуточный файл рядом с финальным (`ggml-base.bin.partial`).
    static func partialURL(for destination: URL) -> URL {
        URL(fileURLWithPath: destination.path + ".partial")
    }

    /// Перенос `.partial` → финальный файл (идемпотентно создаёт каталог).
    static func installDownloadedFile(tempPartial: URL, destination: URL) throws {
        let fileManager = FileManager.default
        let directory = destination.deletingLastPathComponent()
        if !fileManager.fileExists(atPath: directory.path) {
            try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        if fileManager.fileExists(atPath: destination.path) {
            try fileManager.removeItem(at: destination)
        }
        try fileManager.moveItem(at: tempPartial, to: destination)
    }

    func download(
        id: WhisperModelId,
        modelsDirectory: URL,
        progress: @escaping @MainActor (Double) -> Void
    ) async throws -> URL {
        guard let destination = Self.destinationURL(modelsDirectory: modelsDirectory, id: id),
              let sourceURL = id.downloadURL
        else {
            throw WhisperModelDownloaderError.notDownloadable
        }

        let fileManager = FileManager.default
        if fileManager.fileExists(atPath: destination.path) {
            return destination
        }

        await progress(0)

        let partial = Self.partialURL(for: destination)
        if fileManager.fileExists(atPath: partial.path) {
            try fileManager.removeItem(at: partial)
        }

        do {
            // Модели весят сотни мегабайт: без честного прогресса загрузка
            // выглядит зависшей, и её отменяют, не дождавшись.
            try await downloadTransport(sourceURL, partial) { fraction in
                Task { @MainActor in progress(fraction) }
            }
        } catch let error as WhisperModelDownloaderError {
            try? fileManager.removeItem(at: partial)
            throw error
        } catch {
            try? fileManager.removeItem(at: partial)
            throw WhisperModelDownloaderError.downloadFailed(statusCode: nil)
        }

        do {
            try Self.installDownloadedFile(tempPartial: partial, destination: destination)
        } catch {
            try? fileManager.removeItem(at: partial)
            throw error
        }

        await progress(1)
        return destination
    }

    private static func defaultDownloadTransport(
        session: URLSession,
        from sourceURL: URL,
        to partialURL: URL,
        onProgress: @Sendable @escaping (Double) -> Void
    ) async throws {
        let tempFile: URL = try await withCheckedThrowingContinuation { continuation in
            let box = ContinuationBox(continuation)
            let task = session.downloadTask(with: sourceURL) { url, response, error in
                if let error {
                    box.fail(error)
                    return
                }
                if let http = response as? HTTPURLResponse,
                   !(200 ... 299).contains(http.statusCode)
                {
                    box.fail(WhisperModelDownloaderError.downloadFailed(statusCode: http.statusCode))
                    return
                }
                guard let url else {
                    box.fail(WhisperModelDownloaderError.downloadFailed(statusCode: nil))
                    return
                }
                // Временный файл живёт только внутри этого колбэка, поэтому
                // переносим его сразу, а не после возврата из continuation.
                let staged = URL(fileURLWithPath: NSTemporaryDirectory())
                    .appendingPathComponent(UUID().uuidString)
                do {
                    try FileManager.default.moveItem(at: url, to: staged)
                    box.succeed(staged)
                } catch {
                    box.fail(error)
                }
            }
            // `progress` у задачи обновляется по мере получения данных —
            // это единственный источник настоящего прогресса у URLSession.
            let observation = task.progress.observe(\.fractionCompleted) { progress, _ in
                onProgress(progress.fractionCompleted)
            }
            box.retain(observation)
            task.resume()
        }
        defer { try? FileManager.default.removeItem(at: tempFile) }

        let directory = partialURL.deletingLastPathComponent()
        if !FileManager.default.fileExists(atPath: directory.path) {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        if FileManager.default.fileExists(atPath: partialURL.path) {
            try FileManager.default.removeItem(at: partialURL)
        }
        try FileManager.default.moveItem(at: tempFile, to: partialURL)
    }
}

/// Одноразовое возобновление continuation из колбэка URLSession.
///
/// Колбэк может прийти один раз, но компилятор этого не знает, а двойное
/// возобновление — падение. Заодно держит наблюдателя прогресса живым до
/// конца загрузки.
private final class ContinuationBox: @unchecked Sendable {
    private var continuation: CheckedContinuation<URL, Error>?
    private var observation: NSKeyValueObservation?
    private let lock = NSLock()

    init(_ continuation: CheckedContinuation<URL, Error>) {
        self.continuation = continuation
    }

    func retain(_ observation: NSKeyValueObservation) {
        lock.lock()
        defer { lock.unlock() }
        self.observation = observation
    }

    func succeed(_ url: URL) {
        finish { $0.resume(returning: url) }
    }

    func fail(_ error: Error) {
        finish { $0.resume(throwing: error) }
    }

    private func finish(_ body: (CheckedContinuation<URL, Error>) -> Void) {
        lock.lock()
        let pending = continuation
        continuation = nil
        observation?.invalidate()
        observation = nil
        lock.unlock()
        if let pending {
            body(pending)
        }
    }
}
