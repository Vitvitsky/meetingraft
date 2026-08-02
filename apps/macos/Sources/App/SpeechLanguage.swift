import Foundation

/// Язык распознавания речи (ADR-003).
enum SpeechLanguage: String, CaseIterable, Identifiable, Hashable, Sendable {
    case ru
    case en
    case es

    var id: String {
        rawValue
    }

    /// Локализованное имя для UI.
    var displayName: String {
        switch self {
        case .ru: "Русский"
        case .en: "English"
        case .es: "Español"
        }
    }
}
