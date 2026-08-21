import Foundation

/// Название встречи по умолчанию.
///
/// Формат даты локале-зависим, поэтому генерация живёт в презентационном
/// слое, а не в Rust (`AGENTS.md`: форматирование не уезжает в ядро).
/// Ядро хранит название как есть и допускает пустую строку.
enum MeetingTitle {
    /// Название для новой записи: «Meeting Aug 3, 14:30».
    ///
    /// **Не локализуется намеренно.** Это название уходит в базу при
    /// старте записи (`AudioCaptureCoordinator`), то есть становится
    /// данными. Локализованное, оно вморозило бы язык интерфейса в
    /// момент записи: сменил язык системы — и список встреч стал
    /// двуязычным, причём старые названия уже не переписать. То же
    /// основание, по которому не локализуется имя спикера по умолчанию.
    ///
    /// Формат даты локале-зависим и остаётся таким: он читается, а не
    /// хранится смыслом.
    static func forNewMeeting(startedAt: Date = Date()) -> String {
        let stamp = startedAt.formatted(date: .abbreviated, time: .shortened)
        return "Meeting \(stamp)"
    }

    /// Подстановка для встреч, записанных до появления названий.
    static func fallback(startedAtMs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(startedAtMs) / 1000)
        return forNewMeeting(startedAt: date)
    }
}
