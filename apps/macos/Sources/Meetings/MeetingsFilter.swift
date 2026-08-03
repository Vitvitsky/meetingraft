import Foundation

/// Фильтр списка встреч по времени (ТЗ редизайна §4.2).
///
/// Логика вынесена из вью: границы «сегодня» и «эта неделя» зависят от
/// календаря и часового пояса, и такое место обязано быть проверяемым.
enum MeetingsFilter: String, CaseIterable, Identifiable, Hashable {
    case all
    case today
    case thisWeek
    case older

    var id: String {
        rawValue
    }

    var title: String {
        switch self {
        case .all: String(localized: "All")
        case .today: String(localized: "Today")
        case .thisWeek: String(localized: "This week")
        case .older: String(localized: "Older")
        }
    }

    /// Подходит ли встреча под фильтр.
    ///
    /// `calendar` и `now` передаются явно: иначе тест зависел бы от
    /// текущей даты машины и ломался бы раз в неделю.
    func matches(startedAtMs: UInt64, now: Date, calendar: Calendar = .current) -> Bool {
        let started = Date(timeIntervalSince1970: TimeInterval(startedAtMs) / 1000)
        switch self {
        case .all:
            return true
        case .today:
            return calendar.isDate(started, inSameDayAs: now)
        case .thisWeek:
            // «Эта неделя» включает сегодня: иначе встреча исчезала бы из
            // обоих фильтров, кроме «всех».
            return calendar.isDate(started, equalTo: now, toGranularity: .weekOfYear)
        case .older:
            return !calendar.isDate(started, equalTo: now, toGranularity: .weekOfYear)
        }
    }
}
