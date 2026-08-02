import XCTest
@testable import MeetingRaft

@MainActor
final class FakeCaptionStreamTests: XCTestCase {
    func testEmitsPartialThenFinalForFirstSegment() async throws {
        let stream = FakeCaptionStream(
            script: [
                .init(text: "Привет", phase: .partial),
                .init(text: "Привет, команда", phase: .final),
            ],
            tickNanoseconds: 1_000_000
        )
        var received: [CaptionLine] = []
        let done = expectation(description: "two events")
        done.expectedFulfillmentCount = 2

        stream.start { line in
            received.append(line)
            done.fulfill()
        }

        await fulfillment(of: [done], timeout: 2.0)
        stream.stop()

        XCTAssertEqual(received.map(\.phase), [.partial, .final])
        XCTAssertEqual(received.map(\.text), ["Привет", "Привет, команда"])
    }
}
