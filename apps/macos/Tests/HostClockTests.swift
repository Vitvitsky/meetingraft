@testable import MeetingRaft
import XCTest

final class HostClockTests: XCTestCase {
    /// Прибор проверяется заведомо положительным случаем: настоящие часы
    /// на настоящей паузе обязаны показать ненулевой интервал.
    ///
    /// Без этой проверки нуль от сломанного перевода тиков (не тот
    /// множитель, деление раньше умножения) читался бы как «каналы
    /// начались одновременно» — то есть ровно как дефект, который эти
    /// часы устраняют.
    func testSystemClockMeasuresRealInterval() {
        let clock = HostClock.system
        let started = clock.now()
        usleep(50_000)
        let elapsed = clock.elapsedMs(from: started, to: clock.now())

        XCTAssertGreaterThanOrEqual(elapsed, 40, "пауза 50 мс не может измериться нулём")
        XCTAssertLessThan(elapsed, 5_000, "и не может измериться секундами")
    }

    /// Множитель `mach_timebase_info` обязан применяться: на Apple Silicon
    /// тик — 41.67 нс, и без него интервал ушёл бы в 42 раза.
    func testAppliesTimebaseMultiplier() {
        // 125/3 — Apple Silicon. Секунда = 24 000 000 тиков.
        let clock = HostClock(numerator: 125, denominator: 3, now: { 0 })
        XCTAssertEqual(clock.elapsedMs(from: 0, to: 24_000_000), 1_000)
        XCTAssertEqual(clock.elapsedMs(from: 1_000_000, to: 25_000_000), 1_000)

        // 1/1 — Intel, тик равен наносекунде.
        let intel = HostClock(numerator: 1, denominator: 1, now: { 0 })
        XCTAssertEqual(intel.elapsedMs(from: 0, to: 1_150_000_000), 1_150)
    }

    /// Обратный порядок даёт ноль, а не переполнение UInt64.
    func testReversedOrderIsZeroNotOverflow() {
        let clock = HostClock(numerator: 125, denominator: 3, now: { 0 })
        XCTAssertEqual(clock.elapsedMs(from: 24_000_000, to: 0), 0)
        XCTAssertEqual(clock.elapsedMs(from: 42, to: 42), 0)
    }
}
