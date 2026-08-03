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
    /// `(sourceURL, partialDestination)` — для unit-тестов без live HF.
    private let downloadTransport: @Sendable (URL, URL) async throws -> Void
    private let session: URLSession

    init(
        session: URLSession = .shared,
        downloadTransport: (@Sendable (URL, URL) async throws -> Void)? = nil
    ) {
        self.session = session
        self.downloadTransport = downloadTransport ?? { sourceURL, partialURL in
            try await Self.defaultDownloadTransport(
                session: session,
                from: sourceURL,
                to: partialURL
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
            try await downloadTransport(sourceURL, partial)
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
        to partialURL: URL
    ) async throws {
        let (tempFile, response) = try await session.download(from: sourceURL)
        defer { try? FileManager.default.removeItem(at: tempFile) }

        if let http = response as? HTTPURLResponse,
           !(200 ... 299).contains(http.statusCode)
        {
            throw WhisperModelDownloaderError.downloadFailed(statusCode: http.statusCode)
        }

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
