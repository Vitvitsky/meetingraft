@testable import MeetingRaft
import XCTest

@MainActor
final class RustCaptionStreamSmokeTests: XCTestCase {
    func testRustStreamEmitsRussianPartial() async {
        let core = MeetingCore()
        let stream = RustCaptionStream(core: core)
        var received: [CaptionLine] = []
        let done = expectation(description: "at least one event")

        stream.start { line in
            received.append(line)
            done.fulfill()
        }

        await fulfillment(of: [done], timeout: 3.0)
        stream.stop()

        XCTAssertFalse(received.isEmpty)
        XCTAssertEqual(received[0].phase, .partial)
        XCTAssertTrue(received[0].text.contains("Добро"))
    }

    func testEnglishSessionLanguageEmitsEnglishDemo() async {
        let core = MeetingCore()
        XCTAssertTrue(core.setSessionLanguage(primaryCode: "en").isEmpty)
        let stream = RustCaptionStream(core: core)
        var received: [CaptionLine] = []
        let done = expectation(description: "english event")

        stream.start { line in
            received.append(line)
            done.fulfill()
        }

        await fulfillment(of: [done], timeout: 3.0)
        stream.stop()

        XCTAssertFalse(received.isEmpty)
        XCTAssertEqual(received[0].text, "Welcome")
    }
}
