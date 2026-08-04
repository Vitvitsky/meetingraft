@testable import MeetingRaft
import XCTest

/// Глоссарий накапливается годами; ошибка в фильтре тихо прячет часть
/// словаря, и заметить это по интерфейсу почти невозможно.
final class GlossaryFilterTests: XCTestCase {
    private func term(
        id: String = UUID().uuidString,
        surface: String = "билинг",
        canonical: String = "биллинг",
        language: String = "ru",
        scope: FfiGlossaryScope = .global,
        meetingId: String = "",
        kind: FfiGlossaryKind = .replacement
    ) -> FfiGlossaryTerm {
        FfiGlossaryTerm(
            id: id,
            surface: surface,
            canonical: canonical,
            language: language,
            scope: scope,
            meetingId: meetingId,
            kind: kind
        )
    }

    func testAllAcceptsEveryTerm() {
        XCTAssertTrue(GlossaryFilter.all.matches(term(), liveSessionId: nil))
    }

    func testLanguageFilterSelectsByTag() {
        let russian = term(language: "ru")
        let english = term(language: "en")

        XCTAssertTrue(GlossaryFilter.language(.ru).matches(russian, liveSessionId: nil))
        XCTAssertFalse(GlossaryFilter.language(.ru).matches(english, liveSessionId: nil))
    }

    /// «Эта встреча» вне записи обязана быть пустой, а не показывать
    /// термины чужих встреч.
    func testMeetingScopeIsEmptyWithoutLiveSession() {
        let scoped = term(scope: .meeting, meetingId: "m1")

        XCTAssertFalse(GlossaryFilter.meeting.matches(scoped, liveSessionId: nil))
        XCTAssertFalse(GlossaryFilter.meeting.matches(scoped, liveSessionId: ""))
    }

    func testMeetingScopeMatchesOnlyCurrentSession() {
        let mine = term(scope: .meeting, meetingId: "m1")
        let other = term(scope: .meeting, meetingId: "m2")
        let global = term(scope: .global)

        XCTAssertTrue(GlossaryFilter.meeting.matches(mine, liveSessionId: "m1"))
        XCTAssertFalse(GlossaryFilter.meeting.matches(other, liveSessionId: "m1"))
        XCTAssertFalse(GlossaryFilter.meeting.matches(global, liveSessionId: "m1"))
    }

    /// Искать надо по обеим формам: пользователь чаще помнит, во что
    /// термин превращается, чем как его услышала модель.
    func testSearchMatchesBothForms() {
        let item = term(surface: "аппи", canonical: "API")

        XCTAssertTrue(item.matches(query: "аппи"))
        XCTAssertTrue(item.matches(query: "api"))
        XCTAssertTrue(item.matches(query: "AP"))
        XCTAssertFalse(item.matches(query: "биллинг"))
    }

    func testEmptyQueryMatchesEverything() {
        XCTAssertTrue(term().matches(query: ""))
        XCTAssertTrue(term().matches(query: "   "))
    }

    func testFilterIdentifiersAreUnique() {
        let ids = GlossaryFilter.allCases.map(\.id)

        XCTAssertEqual(Set(ids).count, ids.count)
        XCTAssertEqual(GlossaryFilter.allCases.count, 2 + SpeechLanguage.allCases.count)
    }
}
