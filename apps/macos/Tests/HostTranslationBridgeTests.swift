@testable import MeetingRaft
import XCTest

@MainActor
final class HostTranslationBridgeTests: XCTestCase {
    private struct FakeAvailability: TranslationAvailabilityChecking {
        let status: TranslationAvailabilityStatus

        func status(source _: SpeechLanguage, target _: SpeechLanguage) async
            -> TranslationAvailabilityStatus
        {
            status
        }
    }

    private final class FakeHostQueue: TranslationHostQueue {
        var requests: [FfiHostTranslationRequest] = []
        var completed: [String: String] = [:]
        var availabilityFlags: [Bool] = []
        var completionErrors: [String: String] = [:]
        private(set) var drainCount = 0

        func setHostTranslationAvailable(available: Bool) {
            availabilityFlags.append(available)
        }

        func drainHostTranslationRequests() -> [FfiHostTranslationRequest] {
            drainCount += 1
            defer { requests = [] }
            return requests
        }

        func completeHostTranslation(id: String, translatedText: String) -> String {
            completed[id] = translatedText
            return completionErrors[id] ?? ""
        }
    }

    private final class FakeSession: TranslationSessioning {
        var failingText: String?
        var prepareError: Error?
        var prepared = 0
        private(set) var translated: [String] = []

        func translate(_ text: String) async throws -> String {
            if text == failingText {
                throw NSError(domain: "test", code: 1)
            }
            translated.append(text)
            return "перевод: \(text)"
        }

        func prepareTranslation() async throws {
            prepared += 1
            if let prepareError {
                throw prepareError
            }
        }
    }

    private func request(
        id: String,
        text: String,
        source: String = "ru",
        target: String = "en"
    ) -> FfiHostTranslationRequest {
        FfiHostTranslationRequest(
            id: id,
            text: text,
            sourceCode: source,
            targetCode: target,
            phase: .final
        )
    }

    /// Apple-путь обязан отдать перевод сессии, а не метку заглушки.
    func testPumpOnceCompletesTranslation() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "Добро пожаловать")]
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: FakeSession())

        XCTAssertEqual(queue.completed["1"], "перевод: Добро пожаловать")
        XCTAssertEqual(bridge.state, .ready)
    }

    /// Отрицательный контроль: старая заглушка не должна вернуться.
    func testPumpOnceIsNotTheStub() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "Добро пожаловать")]
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: FakeSession())

        XCTAssertNotEqual(queue.completed["1"], "[en·apple] Добро пожаловать")
    }

    /// Упавшая реплика завершается пустой: очередь не растёт, следующая живёт.
    func testPumpOnceOnErrorCompletesEmptyAndContinues() async {
        let queue = FakeHostQueue()
        queue.requests = [
            request(id: "1", text: "раз"),
            request(id: "2", text: "два"),
        ]
        let session = FakeSession()
        session.failingText = "раз"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: session)

        XCTAssertEqual(queue.completed["1"], "")
        XCTAssertEqual(queue.completed["2"], "перевод: два")
    }

    /// Ошибка видна в состоянии, а не проглатывается.
    func testPumpOnceOnErrorPublishesFailure() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "раз")]
        let session = FakeSession()
        session.failingText = "раз"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: session)

        guard case .failed = bridge.state else {
            return XCTFail("ожидалось состояние failed, получено \(bridge.state)")
        }
    }

    /// Отказ ядра при завершении не глотается: он виден в состоянии, а не
    /// подменяется `.ready`.
    func testPumpOnceSurfacesCompletionError() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "Добро пожаловать")]
        queue.completionErrors["1"] = "unknown host translation id: 1"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: FakeSession())

        XCTAssertEqual(bridge.state, .failed("unknown host translation id: 1"))
    }

    /// Отказ ядра на стухшем запросе тоже виден: снятие с очереди — не
    /// повод проглотить ошибку.
    func testPumpOnceStaleDropSurfacesCompletionError() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "hola", target: "es")]
        queue.completionErrors["1"] = "unknown host translation id: 1"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: FakeSession())

        XCTAssertEqual(bridge.state, .failed("unknown host translation id: 1"))
    }

    /// Запрос от другой пары не переводим: сессия настроена не на неё.
    func testPumpOnceDropsRequestForAnotherPair() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "hola", target: "es")]
        let session = FakeSession()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: session)

        XCTAssertEqual(queue.completed["1"], "")
        XCTAssertTrue(session.translated.isEmpty)
    }

    /// Установленная пара разрешает Apple в ядре.
    func testPrepareInstalledRegistersHostAvailable() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)

        await bridge.prepare(source: .ru, target: .en)

        XCTAssertEqual(queue.availabilityFlags.last, true)
        XCTAssertEqual(bridge.state, .ready)
    }

    /// Нескачанная пара запрещает Apple: auto уйдёт на stub/backend.
    func testPrepareNeedsDownloadDisablesHost() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .needsDownload))
        bridge.bind(queue: queue)

        await bridge.prepare(source: .ru, target: .en)

        XCTAssertEqual(queue.availabilityFlags.last, false)
        XCTAssertEqual(bridge.state, .needsDownload)
    }

    func testPrepareUnsupportedDisablesHost() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .unsupported))
        bridge.bind(queue: queue)

        await bridge.prepare(source: .ru, target: .en)

        XCTAssertEqual(queue.availabilityFlags.last, false)
        XCTAssertEqual(bridge.state, .unsupported)
    }

    /// Скачивание идёт через сессию и пересчитывает доступность.
    func testPerformDownloadPreparesAndRechecks() async {
        let queue = FakeHostQueue()
        let session = FakeSession()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.performDownload(session: session, source: .ru, target: .en)

        XCTAssertEqual(session.prepared, 1)
        XCTAssertEqual(bridge.state, .ready)
    }

    /// Отказ `prepareTranslation` виден в состоянии и не оставляет
    /// `.downloading`.
    func testPerformDownloadFailurePublishesFailure() async {
        let queue = FakeHostQueue()
        let session = FakeSession()
        session.prepareError = NSError(domain: "test", code: 2)
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.performDownload(session: session, source: .ru, target: .en)

        guard case .failed = bridge.state else {
            return XCTFail("ожидалось состояние failed, получено \(bridge.state)")
        }
        XCTAssertNotEqual(bridge.state, .downloading)
    }

    /// `bind` после `prepare` обязан донести уже известную доступность:
    /// `.onAppear` может прийти позже, чем цикл `run` начнёт `prepare`.
    func testBindPushesKnownAvailability() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))

        await bridge.prepare(source: .ru, target: .en)
        bridge.bind(queue: queue)

        XCTAssertEqual(queue.availabilityFlags.last, true)
    }

    /// Недоступная пара не копит очередь: снятые запросы завершаются
    /// пустыми молча, и видимое `.needsDownload` не подменяется ошибкой
    /// ядра, даже если оно её вернуло.
    func testDropPendingRequestsIsSilent() async {
        let queue = FakeHostQueue()
        queue.requests = [
            request(id: "1", text: "раз"),
            request(id: "2", text: "два"),
        ]
        queue.completionErrors["1"] = "unknown host translation id: 1"
        queue.completionErrors["2"] = "unknown host translation id: 2"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .needsDownload))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        bridge.dropPendingRequests()

        XCTAssertEqual(queue.completed["1"], "")
        XCTAssertEqual(queue.completed["2"], "")
        XCTAssertTrue(queue.requests.isEmpty)
        XCTAssertEqual(bridge.state, .needsDownload)
    }

    /// Цикл `run` на непригодной паре дренит очередь каждый проход, а по
    /// отмене — снимает доступность, дренит остаток и уходит в `.off`.
    func testRunDrainsQueueWhilePairUnavailableAndResetsOnCancel() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "раз")]
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .needsDownload))
        bridge.bind(queue: queue)

        let task = Task { @MainActor in
            await bridge.run(session: FakeSession(), source: .ru, target: .en)
        }
        let deadline = Date().addingTimeInterval(1)
        while queue.drainCount == 0, Date() < deadline {
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        // Дренаж обязан случиться до отмены: иначе очередь копится весь
        // цикл и разом всплывает на teardown.
        XCTAssertGreaterThanOrEqual(queue.drainCount, 1)
        XCTAssertTrue(queue.requests.isEmpty)
        // Пока цикл жив, сессия есть: иначе кнопка скачивания нечем работает.
        XCTAssertTrue(bridge.hasSession)

        task.cancel()
        await task.value

        XCTAssertFalse(bridge.hasSession)
        XCTAssertEqual(queue.completed["1"], "")
        XCTAssertEqual(queue.availabilityFlags.last, false)
        XCTAssertEqual(bridge.state, .off)
    }
}
