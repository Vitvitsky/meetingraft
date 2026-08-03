import Foundation

/// Пункты боковой навигации.
enum AppDestination: String, Hashable, CaseIterable, Identifiable {
    /// Дом приложения — накопленные встречи, а не текущая запись.
    case meetings
    case liveCaptions
    case glossary

    var id: String {
        rawValue
    }

    var title: String {
        switch self {
        case .liveCaptions: "Live Captions"
        case .meetings: "Meetings"
        case .glossary: "Glossary"
        }
    }

    var systemImage: String {
        switch self {
        case .liveCaptions: "captions.bubble"
        case .meetings: "calendar"
        case .glossary: "book"
        }
    }
}
