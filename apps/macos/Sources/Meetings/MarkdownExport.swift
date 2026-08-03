import Foundation

enum MarkdownExportKind: String {
    case final
    case brief
    case followUp = "follow-up"
}

enum MarkdownExport {
    /// Первые 8 символов meeting_id после замены небезопасных символов на `_`.
    static func shortId(meetingId: String) -> String {
        let sanitized = meetingId.replacingOccurrences(
            of: "[^A-Za-z0-9_-]",
            with: "_",
            options: .regularExpression
        )
        return String(sanitized.prefix(8))
    }

    static func fileName(
        startedAtMs: UInt64,
        meetingId: String,
        kind: MarkdownExportKind,
        calendar: Calendar = .current,
        timeZone: TimeZone = .current
    ) -> String {
        var cal = calendar
        cal.timeZone = timeZone
        let date = Date(timeIntervalSince1970: TimeInterval(startedAtMs) / 1000)
        let components = cal.dateComponents([.year, .month, .day], from: date)
        let datePart = String(format: "%04d-%02d-%02d", components.year!, components.month!, components.day!)
        return "\(datePart)-\(shortId(meetingId: meetingId))-\(kind.rawValue).md"
    }

    /// Пишет UTF-8 markdown; создаёт directory; перезаписывает файл.
    static func write(folderURL: URL, fileName: String, body: String) throws -> URL {
        try FileManager.default.createDirectory(
            at: folderURL,
            withIntermediateDirectories: true
        )
        let fileURL = folderURL.appendingPathComponent(fileName)
        try body.write(to: fileURL, atomically: false, encoding: .utf8)
        return fileURL
    }
}
