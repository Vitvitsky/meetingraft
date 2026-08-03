@testable import MeetingRaft
import XCTest

final class MarkdownExportTests: XCTestCase {
    func testShortIdTakesFirst8SafeChars() {
        XCTAssertEqual(MarkdownExport.shortId(meetingId: "abcdef12-zzzz"), "abcdef12")
        XCTAssertEqual(MarkdownExport.shortId(meetingId: "ab/cd"), "ab_cd")
    }

    func testFileNameUsesDateShortIdAndKind() throws {
        let utc = try XCTUnwrap(TimeZone(secondsFromGMT: 0))
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = utc
        // 2026-08-03T00:00:00Z
        let ms: UInt64 = 1_785_715_200_000
        let name = MarkdownExport.fileName(
            startedAtMs: ms,
            meetingId: "a1b2c3d4xxxx",
            kind: .brief,
            calendar: calendar,
            timeZone: utc
        )
        XCTAssertEqual(name, "2026-08-03-a1b2c3d4-brief.md")
    }

    func testWriteCreatesAndOverwrites() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mr-export-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let url1 = try MarkdownExport.write(folderURL: dir, fileName: "x-final.md", body: "v1")
        XCTAssertEqual(try String(contentsOf: url1, encoding: .utf8), "v1")
        _ = try MarkdownExport.write(folderURL: dir, fileName: "x-final.md", body: "v2")
        XCTAssertEqual(try String(contentsOf: url1, encoding: .utf8), "v2")
    }
}
