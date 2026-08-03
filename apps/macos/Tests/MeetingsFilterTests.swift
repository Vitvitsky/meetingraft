@testable import MeetingRaft
import XCTest

/// Границы «сегодня» и «эта неделя» зависят от календаря; тест задаёт
/// момент явно, иначе он ломался бы раз в неделю сам по себе.
final class MeetingsFilterTests: XCTestCase {
    private var calendar: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        calendar.firstWeekday = 2
        return calendar
    }()

    /// Среда, 5 августа 2026, 12:00 UTC.
    private var now: Date {
        calendar.date(from: DateComponents(year: 2026, month: 8, day: 5, hour: 12))!
    }

    private func ms(_ date: Date) -> UInt64 {
        UInt64(date.timeIntervalSince1970 * 1000)
    }

    private func date(day: Int, hour: Int = 10) -> Date {
        calendar.date(from: DateComponents(year: 2026, month: 8, day: day, hour: hour))!
    }

    func testAllAcceptsEverything() {
        XCTAssertTrue(
            MeetingsFilter.all.matches(startedAtMs: ms(date(day: 1)), now: now, calendar: calendar)
        )
    }

    func testTodayMatchesSameDayOnly() {
        XCTAssertTrue(
            MeetingsFilter.today.matches(startedAtMs: ms(date(day: 5, hour: 8)), now: now, calendar: calendar)
        )
        XCTAssertFalse(
            MeetingsFilter.today.matches(startedAtMs: ms(date(day: 4)), now: now, calendar: calendar)
        )
    }

    /// «Эта неделя» обязана включать сегодня, иначе сегодняшняя встреча
    /// выпадала бы из всех фильтров, кроме «всех».
    func testThisWeekIncludesToday() {
        XCTAssertTrue(
            MeetingsFilter.thisWeek.matches(startedAtMs: ms(date(day: 5)), now: now, calendar: calendar)
        )
        // Понедельник той же недели.
        XCTAssertTrue(
            MeetingsFilter.thisWeek.matches(startedAtMs: ms(date(day: 3)), now: now, calendar: calendar)
        )
    }

    func testOlderIsTheComplementOfThisWeek() {
        let lastWeek = ms(date(day: 1))

        XCTAssertFalse(
            MeetingsFilter.thisWeek.matches(startedAtMs: lastWeek, now: now, calendar: calendar)
        )
        XCTAssertTrue(
            MeetingsFilter.older.matches(startedAtMs: lastWeek, now: now, calendar: calendar)
        )
    }

    /// Каждая встреча попадает ровно в один из двух непересекающихся
    /// фильтров: сумма их счётчиков обязана сходиться с общим числом.
    func testThisWeekAndOlderPartitionAllMeetings() {
        for day in 1 ... 9 {
            let stamp = ms(date(day: day))
            let inWeek = MeetingsFilter.thisWeek.matches(startedAtMs: stamp, now: now, calendar: calendar)
            let older = MeetingsFilter.older.matches(startedAtMs: stamp, now: now, calendar: calendar)

            XCTAssertNotEqual(inWeek, older, "день \(day) попал в оба или ни в один")
        }
    }

    func testTitlesAreNotEmpty() {
        for filter in MeetingsFilter.allCases {
            XCTAssertFalse(filter.title.isEmpty, "\(filter)")
        }
    }
}
