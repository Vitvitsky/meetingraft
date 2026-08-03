import Foundation

/// Область просмотра глоссария (ТЗ редизайна §4.5).
///
/// Логика отбора вынесена из вью: глоссарий — то место, где термины
/// накапливаются годами, и ошибка в фильтре тихо прячет часть словаря.
enum GlossaryFilter: Identifiable, Hashable, CaseIterable {
    case all
    case language(SpeechLanguage)
    case meeting

    static var allCases: [GlossaryFilter] {
        [.all] + SpeechLanguage.allCases.map(GlossaryFilter.language) + [.meeting]
    }

    var id: String {
        switch self {
        case .all: "all"
        case let .language(language): "lang-\(language.rawValue)"
        case .meeting: "meeting"
        }
    }

    var title: String {
        switch self {
        case .all: String(localized: "All")
        case let .language(language): language.rawValue.uppercased()
        case .meeting: String(localized: "This meeting")
        }
    }

    /// Подходит ли термин под фильтр.
    ///
    /// `liveSessionId` передаётся явно: «эта встреча» имеет смысл только
    /// во время записи, и вне её фильтр обязан быть пустым, а не показывать
    /// термины чужих встреч.
    func matches(_ term: FfiGlossaryTerm, liveSessionId: String?) -> Bool {
        switch self {
        case .all:
            return true
        case let .language(language):
            return term.language == language.rawValue
        case .meeting:
            guard let liveSessionId, !liveSessionId.isEmpty else { return false }
            return term.scope == .meeting && term.meetingId == liveSessionId
        }
    }
}

extension FfiGlossaryTerm {
    /// Совпадение с поисковым запросом по обеим формам термина.
    ///
    /// Искать только по исходной форме недостаточно: пользователь чаще
    /// помнит, во что термин превращается, а не как его услышала модель.
    func matches(query: String) -> Bool {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !needle.isEmpty else { return true }
        return surface.lowercased().contains(needle)
            || canonical.lowercased().contains(needle)
    }
}
