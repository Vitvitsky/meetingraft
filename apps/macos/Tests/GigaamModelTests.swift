@testable import MeetingRaft
import XCTest

/// Каталог файлов русского движка и их загрузка.
///
/// Проверяется то, что расходится молча: путь, по которому ядро ищет
/// модель, формат метки, которую читает скрипт, и общий прогресс.
final class GigaamModelCatalogTests: XCTestCase {
    private var modelsDirectory: URL {
        URL(fileURLWithPath: "/tmp/mr-models", isDirectory: true)
    }

    /// Файлы кладутся туда, где их ищет ядро (`models/gigaam/`).
    ///
    /// Разойдись это с Rust — приложение скачало бы 230 МБ, а выбор
    /// движка остался бы недоступным, и связать одно с другим было бы
    /// нечем.
    func testFilesLandWhereTheCoreLooksForThem() {
        let encoder = GigaamModelCatalog.destinationURL(
            modelsDirectory: modelsDirectory,
            file: .encoder
        )

        XCTAssertTrue(encoder.path.hasSuffix("/models/gigaam/encoder.int8.onnx"), encoder.path)
    }

    /// Метка совпадает по формату с тем, что пишет
    /// `scripts/fetch-gigaam-models.sh`: `<экспорт>/<имя файла>`.
    ///
    /// Разойдись формат — скрипт счёл бы скачанный файл чужим и качал бы
    /// 225 МБ заново при каждом запуске.
    func testMarkerMatchesTheShellScriptFormat() {
        let marker = GigaamModelCatalog.markerContents(for: .encoder)

        XCTAssertEqual(marker, "\(GigaamModelCatalog.export)/encoder.int8.onnx")
        XCTAssertTrue(marker.contains("2025-12-16"), "версия экспорта потерялась: \(marker)")
    }

    /// Файл метки лежит рядом с файлом, а не подменяет его.
    func testMarkerSitsBesideTheFile() {
        let destination = GigaamModelCatalog.destinationURL(
            modelsDirectory: modelsDirectory,
            file: .tokens
        )

        let marker = GigaamModelCatalog.markerURL(for: destination)

        XCTAssertEqual(marker.path, destination.path + ".source")
    }

    /// Сумма считается из самих файлов, а не вписана числом: вписанное
    /// разошлось бы с составом комплекта молча.
    func testTotalIsTheSumOfTheFiles() {
        let expected = GigaamModelFile.allCases.reduce(Int64(0)) { $0 + $1.approximateBytes }

        XCTAssertEqual(GigaamModelCatalog.approximateTotalBytes, expected)
        // И отдельно — что это вообще похоже на четверть гигабайта, а не
        // на ноль: сумма нулей тоже равна сумме нулей.
        XCTAssertGreaterThan(GigaamModelCatalog.approximateTotalBytes, 200_000_000)
    }

    /// Пустой каталог установленным не считается.
    func testAnEmptyDirectoryIsNotInstalled() {
        let empty = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)

        XCTAssertFalse(GigaamModelCatalog.isInstalled(modelsDirectory: empty))
    }
}

/// Загрузка комплекта: четыре файла и один общий прогресс.
final class GigaamModelDownloaderTests: XCTestCase {
    /// Транспорт-двойник: ничего не качает, но пишет файл и сообщает
    /// прогресс — ровно то, на что опирается загрузчик комплекта.
    private struct FakeFiles: FileDownloading {
        let onFile: @Sendable (URL) -> Void

        func downloadFile(
            from sourceURL: URL,
            to destination: URL,
            progress: @escaping @MainActor (Double) -> Void
        ) async throws {
            onFile(sourceURL)
            try FileManager.default.createDirectory(
                at: destination.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try Data("x".utf8).write(to: destination)
            await progress(0.5)
            await progress(1)
        }
    }

    func testEveryFileIsFetchedAndMarked() async throws {
        let root = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let requested = Locked<[URL]>([])
        let downloader = GigaamModelDownloader(files: FakeFiles { url in requested.append(url) })

        try await downloader.download(modelsDirectory: root) { _ in }

        XCTAssertEqual(requested.value.count, GigaamModelFile.allCases.count)
        XCTAssertTrue(GigaamModelCatalog.isInstalled(modelsDirectory: root))
        // Метка обязана появиться рядом с каждым файлом: без неё скрипт
        // перекачает комплект заново.
        for file in GigaamModelFile.allCases {
            let destination = GigaamModelCatalog.destinationURL(modelsDirectory: root, file: file)
            let marker = GigaamModelCatalog.markerURL(for: destination)
            let contents = try String(contentsOf: marker, encoding: .utf8)
            XCTAssertEqual(contents, GigaamModelCatalog.markerContents(for: file))
        }
        try? FileManager.default.removeItem(at: root)
    }

    /// Прогресс идёт вперёд и доходит до единицы.
    ///
    /// Утверждение о свойстве, а не о значениях: конкретные доли зависят
    /// от размеров файлов, и вписывать их значило бы подбирать под ответ.
    func testProgressNeverGoesBackAndReachesOne() async throws {
        let root = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let seen = Locked<[Double]>([])
        let downloader = GigaamModelDownloader(files: FakeFiles { _ in })

        try await downloader.download(modelsDirectory: root) { value in
            seen.append(value)
        }

        let values = seen.value
        XCTAssertFalse(values.isEmpty, "прогресс не сообщался вовсе")
        XCTAssertEqual(values.last ?? 0, 1, accuracy: 0.001)
        for (previous, next) in zip(values, values.dropFirst()) {
            XCTAssertLessThanOrEqual(previous, next + 0.001, "прогресс поехал назад")
        }
        try? FileManager.default.removeItem(at: root)
    }
}

/// Настройка движка переживает перезапуск.
final class PostCallRecognizerPersistenceTests: XCTestCase {
    private func makeDefaults() -> UserDefaults {
        let suite = "mr-tests-\(UUID().uuidString)"
        return UserDefaults(suiteName: suite) ?? .standard
    }

    /// Выбор, сделанный в одном запуске, виден в следующем.
    ///
    /// Проверяется вторым экземпляром стора, а не чтением ключа: ключ
    /// сверялся бы сам с собой.
    func testTheChoiceSurvivesARestart() {
        let defaults = makeDefaults()
        let first = ProviderSettingsStore(defaults: defaults)

        first.postCallRecognizer = .gigaam

        let second = ProviderSettingsStore(defaults: defaults)
        XCTAssertEqual(second.postCallRecognizer, .gigaam)
    }

    /// Умолчание — правило по языку, а не движок.
    func testTheDefaultIsAutomatic() {
        let store = ProviderSettingsStore(defaults: makeDefaults())

        XCTAssertEqual(store.postCallRecognizer, .auto)
    }

    /// Мусор в настройках — это «не выбрано», а не повод упасть и не
    /// повод молча включить русский движок.
    func testAnUnknownStoredValueFallsBackToAutomatic() {
        let defaults = makeDefaults()
        defaults.set("gigaam-v3", forKey: "postCall.recognizer")

        let store = ProviderSettingsStore(defaults: defaults)

        XCTAssertEqual(store.postCallRecognizer, .auto)
    }

    /// Коды обязаны совпадать с Rust (`domain::PostCallRecognizer`):
    /// ядро разбирает именно эти строки и неизвестную отвергает.
    func testCodesMatchTheCoreContract() {
        XCTAssertEqual(
            Set(PostCallRecognizer.allCases.map(\.rawValue)),
            ["auto", "whisper", "gigaam"]
        )
    }
}

/// Потокобезопасный накопитель для двойников: колбэки прилетают из
/// произвольного контекста, а `XCTest` читает результат в конце.
private final class Locked<Value>: @unchecked Sendable {
    private var storage: Value
    private let lock = NSLock()

    init(_ value: Value) {
        storage = value
    }

    var value: Value {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }
}

private extension Locked {
    func append<Element>(_ element: Element) where Value == [Element] {
        lock.lock()
        defer { lock.unlock() }
        storage.append(element)
    }
}
