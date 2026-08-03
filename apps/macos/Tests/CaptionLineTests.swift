@testable import MeetingRaft
import XCTest

final class CaptionLineTests: XCTestCase {
    private func event(id: String, channel: String, phase: FfiCaptionPhase) -> FfiCaptionEvent {
        FfiCaptionEvent(id: id, text: "текст", phase: phase, channel: channel)
    }

    func testMapsSystemChannelToOthers() {
        let line = CaptionLine(event: event(id: UUID().uuidString, channel: "system", phase: .final))

        XCTAssertEqual(line.speaker, .others)
        XCTAssertEqual(line.phase, .final)
    }

    func testMapsMicChannelToYou() {
        let line = CaptionLine(event: event(id: UUID().uuidString, channel: "mic", phase: .partial))

        XCTAssertEqual(line.speaker, .you)
        XCTAssertEqual(line.phase, .partial)
    }

    /// Неизвестный код канала не должен ронять ленту.
    func testUnknownChannelFallsBackToYou() {
        let line = CaptionLine(event: event(id: UUID().uuidString, channel: "", phase: .final))

        XCTAssertEqual(line.speaker, .you)
    }

    /// Невалидный uuid из Rust заменяется сгенерированным.
    func testNonUuidIdentifierIsReplaced() {
        let line = CaptionLine(event: event(id: "not-a-uuid", channel: "mic", phase: .final))

        XCTAssertEqual(line.text, "текст")
        XCTAssertNotNil(line.id)
    }
}
