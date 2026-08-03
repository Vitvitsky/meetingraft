@testable import MeetingRaft
import XCTest

final class WhisperModelDownloaderTests: XCTestCase {
    private var tempDir: URL!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try! FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    // MARK: - Catalog

    func testWhisperModelIdFilenamesMatchRustCatalog() {
        XCTAssertNil(WhisperModelId.auto.filename)
        XCTAssertEqual(WhisperModelId.base.filename, "ggml-base.bin")
        XCTAssertEqual(WhisperModelId.small.filename, "ggml-small.bin")
        XCTAssertEqual(WhisperModelId.largeV3Turbo.filename, "ggml-large-v3-turbo.bin")
    }

    func testWhisperModelIdDownloadURLsUseHuggingFaceResolve() throws {
        XCTAssertNil(WhisperModelId.auto.downloadURL)

        let base = try XCTUnwrap(WhisperModelId.base.downloadURL)
        XCTAssertEqual(
            base.absoluteString,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
        )

        let small = try XCTUnwrap(WhisperModelId.small.downloadURL)
        XCTAssertEqual(
            small.absoluteString,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        )

        let turbo = try XCTUnwrap(WhisperModelId.largeV3Turbo.downloadURL)
        XCTAssertEqual(
            turbo.absoluteString,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
        )
    }

    func testWhisperModelIdRawValues() {
        XCTAssertEqual(WhisperModelId.auto.rawValue, "auto")
        XCTAssertEqual(WhisperModelId.base.rawValue, "base")
        XCTAssertEqual(WhisperModelId.small.rawValue, "small")
        XCTAssertEqual(WhisperModelId.largeV3Turbo.rawValue, "large-v3-turbo")
    }

    // MARK: - destinationURL

    func testDestinationURLForDownloadableIds() {
        XCTAssertEqual(
            WhisperModelDownloader.destinationURL(modelsDirectory: tempDir, id: .base)?.lastPathComponent,
            "ggml-base.bin"
        )
        XCTAssertEqual(
            WhisperModelDownloader.destinationURL(modelsDirectory: tempDir, id: .small)?.lastPathComponent,
            "ggml-small.bin"
        )
        XCTAssertEqual(
            WhisperModelDownloader.destinationURL(modelsDirectory: tempDir, id: .largeV3Turbo)?
                .lastPathComponent,
            "ggml-large-v3-turbo.bin"
        )
    }

    func testDestinationURLForAutoIsNil() {
        XCTAssertNil(WhisperModelDownloader.destinationURL(modelsDirectory: tempDir, id: .auto))
    }

    // MARK: - installDownloadedFile

    func testInstallDownloadedFileRenamesPartialToFinal() throws {
        let destination = tempDir.appendingPathComponent("ggml-base.bin")
        let partial = WhisperModelDownloader.partialURL(for: destination)
        try Data("ggml-bytes".utf8).write(to: partial)

        try WhisperModelDownloader.installDownloadedFile(tempPartial: partial, destination: destination)

        XCTAssertTrue(FileManager.default.fileExists(atPath: destination.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: partial.path))
        let contents = try String(contentsOf: destination, encoding: .utf8)
        XCTAssertEqual(contents, "ggml-bytes")
    }

    func testInstallDownloadedFileCreatesModelsDirectory() throws {
        let modelsDir = tempDir.appendingPathComponent("nested/models", isDirectory: true)
        let destination = modelsDir.appendingPathComponent("ggml-small.bin")
        let partial = tempDir.appendingPathComponent("staging.partial")
        try Data("small".utf8).write(to: partial)

        try WhisperModelDownloader.installDownloadedFile(tempPartial: partial, destination: destination)

        XCTAssertTrue(FileManager.default.fileExists(atPath: destination.path))
    }

    func testInstallDownloadedFileReplacesExistingDestination() throws {
        let destination = tempDir.appendingPathComponent("ggml-base.bin")
        try Data("old".utf8).write(to: destination)
        let partial = WhisperModelDownloader.partialURL(for: destination)
        try Data("new".utf8).write(to: partial)

        try WhisperModelDownloader.installDownloadedFile(tempPartial: partial, destination: destination)

        let contents = try String(contentsOf: destination, encoding: .utf8)
        XCTAssertEqual(contents, "new")
    }

    // MARK: - download (без live HF)

    func testDownloadSkipsWhenDestinationAlreadyExists() async throws {
        let destination = try XCTUnwrap(
            WhisperModelDownloader.destinationURL(modelsDirectory: tempDir, id: .base)
        )
        try Data("existing".utf8).write(to: destination)

        final class DownloadTracker: @unchecked Sendable {
            var invoked = false
        }
        let tracker = DownloadTracker()
        let downloader = WhisperModelDownloader(downloadTransport: { _, _, _ in
            tracker.invoked = true
        })

        let result = try await downloader.download(id: .base, modelsDirectory: tempDir) { _ in }

        XCTAssertEqual(result, destination)
        XCTAssertFalse(tracker.invoked)
    }

    func testDownloadWritesFileViaInjectedTransport() async throws {
        let payload = Data("downloaded-model".utf8)
        let downloader = WhisperModelDownloader(downloadTransport: { _, partialURL, _ in
            try payload.write(to: partialURL)
        })

        let result = try await downloader.download(id: .base, modelsDirectory: tempDir) { _ in }

        XCTAssertEqual(result.lastPathComponent, "ggml-base.bin")
        let contents = try Data(contentsOf: result)
        XCTAssertEqual(contents, payload)
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: WhisperModelDownloader.partialURL(for: result).path
            )
        )
    }

    func testDownloadReportsProgress() async throws {
        final class ProgressTracker: @unchecked Sendable {
            var fractions: [Double] = []
        }
        let tracker = ProgressTracker()
        let downloader = WhisperModelDownloader(downloadTransport: { _, partialURL, _ in
            try Data("x".utf8).write(to: partialURL)
        })

        _ = try await downloader.download(id: .base, modelsDirectory: tempDir) { fraction in
            tracker.fractions.append(fraction)
        }

        XCTAssertEqual(tracker.fractions.first, 0)
        XCTAssertEqual(tracker.fractions.last, 1)
    }

    func testDownloadAutoThrowsNotDownloadable() async {
        let downloader = WhisperModelDownloader()

        do {
            _ = try await downloader.download(id: .auto, modelsDirectory: tempDir) { _ in }
            XCTFail("Expected notDownloadable")
        } catch WhisperModelDownloaderError.notDownloadable {
            // ok
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testDownloadPreservesHTTPStatusOnTransportFailure() async {
        let downloader = WhisperModelDownloader(downloadTransport: { _, _, _ in
            throw WhisperModelDownloaderError.downloadFailed(statusCode: 404)
        })

        do {
            _ = try await downloader.download(id: .base, modelsDirectory: tempDir) { _ in }
            XCTFail("Expected downloadFailed")
        } catch WhisperModelDownloaderError.downloadFailed(statusCode: 404) {
            // ok
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    /// Прогресс обязан доходить до UI: без него загрузка на сотни
    /// мегабайт выглядит зависшей.
    func testProgressFromTransportReachesCaller() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        let downloader = WhisperModelDownloader(downloadTransport: { _, partialURL, onProgress in
            onProgress(0.25)
            onProgress(0.75)
            try Data("ggml".utf8).write(to: partialURL)
        })
        let seen = ProgressSink()

        _ = try await downloader.download(id: .small, modelsDirectory: directory) { value in
            seen.append(value)
        }

        let values = seen.values
        XCTAssertTrue(values.contains(0.25), "\(values)")
        XCTAssertTrue(values.contains(0.75), "\(values)")
        try? FileManager.default.removeItem(at: directory)
    }
}

/// Собирает значения прогресса с MainActor.
@MainActor
private final class ProgressSink {
    private(set) var values: [Double] = []

    nonisolated init() {}

    func append(_ value: Double) {
        values.append(value)
    }
}
