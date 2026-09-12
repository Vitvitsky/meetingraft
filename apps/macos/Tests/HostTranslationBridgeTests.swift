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

        func setHostTranslationAvailable(available: Bool) {
            availabilityFlags.append(available)
        }

        func drainHostTranslationRequests() -> [FfiHostTranslationRequest] {
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
}
