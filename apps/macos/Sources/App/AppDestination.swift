import Foundation

/// Пункты боковой навигации.
enum AppDestination: String, Hashable, CaseIterable, Identifiable {
    case liveCaptions
    case meetings
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
