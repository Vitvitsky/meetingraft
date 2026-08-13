@testable import MeetingRaft
import XCTest

/// Часы, идущие по заранее заданным отметкам.
///
/// Настоящие часы для этого не годятся: тест на них мог бы утверждать
/// только «не отрицательное», а проверять надо, что цена шага — это
/// расстояние между отметками, а не что-нибудь ещё.
private final class Ticker: @unchecked Sendable {
    private let values: [UInt64]
    private var index = 0

    init(_ values: [UInt64]) {
        self.values = values
    }

    func next() -> UInt64 {
        defer { index += 1 }
        return values[min(index, values.count - 1)]
    }
}

final class CaptureStepTimerTests: XCTestCase {
    /// Тик = наносекунда: множитель 1/1.
    private func timer(_ ticks: [UInt64]) -> CaptureStepTimer {
        let ticker = Ticker(ticks)
        return CaptureStepTimer(clock: HostClock(numerator: 1, denominator: 1, now: { ticker.next() }))
    }

    /// Цена шага — расстояние от прошлой отметки, а не от начала.
    ///
    /// Без этого два шага по 5 мс дали бы 5 и 10, и дорогим оказался бы
    /// последний — всегда, независимо от того, что происходит.
    func testEachStepMeasuresFromThePreviousMark() {
        // Отметки: старт 0, затем 5 мс, 12 мс, 1012 мс.
        var timer = timer([0, 5_000_000, 12_000_000, 1_012_000_000])
        timer.step("create_tap")
        timer.step("aggregate")
        timer.step("device_start")

        XCTAssertEqual(timer.steps, [
            CaptureStartStep(name: "create_tap", elapsedMs: 5),
            CaptureStartStep(name: "aggregate", elapsedMs: 7),
            CaptureStartStep(name: "device_start", elapsedMs: 1000),
        ])
    }

    /// Шаг короче миллисекунды — ноль, и это ответ «не этот шаг», а не
    /// пропущенный замер: строка в списке всё равно есть.
    func testASubMillisecondStepIsRecordedAsZero() {
        var timer = timer([0, 300_000])
        timer.step("output_device")

        XCTAssertEqual(timer.steps.count, 1, "шаг обязан остаться в списке")
        XCTAssertEqual(timer.steps.first?.elapsedMs, 0)
    }
}
