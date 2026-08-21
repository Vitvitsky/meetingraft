@testable import MeetingRaft
import SwiftUI
import XCTest

/// Токены — единственное место, где заданы цвета и метрика; тест ловит
/// опечатку в шестнадцатеричном литерале, которую глазом не увидеть.
///
/// Класс помечен `@MainActor`: `performAsCurrentDrawingAppearance`
/// трогает состояние отрисовки, а динамический цвет вне назначенной темы
/// значения не имеет вовсе.
@MainActor
final class ThemeTests: XCTestCase {
    private func components(
        _ color: Color,
        in name: NSAppearance.Name
    ) -> (r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) {
        var out: (r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) = (0, 0, 0, 0)
        NSAppearance(named: name)!.performAsCurrentDrawingAppearance {
            let native = NSColor(color).usingColorSpace(.sRGB) ?? .black
            native.getRed(&out.r, green: &out.g, blue: &out.b, alpha: &out.a)
        }
        return out
    }

    private func assertHex(
        _ color: Color,
        in name: NSAppearance.Name,
        equals hex: UInt32,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let got = components(color, in: name)
        XCTAssertEqual(got.r, CGFloat((hex >> 16) & 0xFF) / 255, accuracy: 0.004, file: file, line: line)
        XCTAssertEqual(got.g, CGFloat((hex >> 8) & 0xFF) / 255, accuracy: 0.004, file: file, line: line)
        XCTAssertEqual(got.b, CGFloat(hex & 0xFF) / 255, accuracy: 0.004, file: file, line: line)
    }

    func testHexInitParsesChannelsInOrder() {
        let parts = components(Color(hex: 0x4A9FD8), in: .darkAqua)

        XCTAssertEqual(parts.r, 74.0 / 255, accuracy: 0.01)
        XCTAssertEqual(parts.g, 159.0 / 255, accuracy: 0.01)
        XCTAssertEqual(parts.b, 216.0 / 255, accuracy: 0.01)
        XCTAssertEqual(parts.a, 1, accuracy: 0.01)
    }

    func testHexInitAppliesOpacity() {
        let parts = components(Color(hex: 0x000000, opacity: 0.19), in: .darkAqua)

        XCTAssertEqual(parts.a, 0.19, accuracy: 0.01)
    }

    /// Акцент переехал на системный синий Apple (Epic 23), и в светлой
    /// теме он на шаг темнее системного: `#007AFF` даёт на белом 4.02:1,
    /// то есть ниже порога для обычного текста.
    func testAccentIsSystemBlueInBothAppearances() {
        assertHex(Theme.accent, in: .darkAqua, equals: 0x0A84FF)
        assertHex(Theme.accent, in: .aqua, equals: 0x0069D9)
    }

    /// Главный признак того, что тема вообще подключена: фон окна обязан
    /// быть разным. Одинаковый означает, что провайдер не спросили.
    func testSurfaceRootDiffersBetweenAppearances() {
        assertHex(Theme.surfaceRoot, in: .darkAqua, equals: 0x0D0D0F)
        assertHex(Theme.surfaceRoot, in: .aqua, equals: 0xFFFFFF)
    }

    /// Жёлтый на белом читается как 1.41:1 — в светлой теме
    /// предупреждение оранжевое. Единственный статусный цвет, который
    /// нельзя было перенести как есть.
    func testWarningTurnsOrangeOnLight() {
        assertHex(Theme.warning, in: .darkAqua, equals: 0xFFD60A)
        assertHex(Theme.warning, in: .aqua, equals: 0xB25000)
    }

    /// Границы меняют не только яркость, но и сам цвет: белый на 6%
    /// поверх белого фона не виден вовсе.
    func testBordersInvertWithAppearance() {
        XCTAssertGreaterThan(components(Theme.borderSubtle, in: .darkAqua).r, 0.9)
        XCTAssertLessThan(components(Theme.borderSubtle, in: .aqua).r, 0.1)
    }

    /// Шкала отступов обязана расти: перепутанные значения ломают ритм
    /// всех экранов сразу.
    func testSpacingScaleIsAscending() {
        let scale = [
            Theme.Space.xxs, Theme.Space.xs, Theme.Space.sm,
            Theme.Space.md, Theme.Space.lg, Theme.Space.xl, Theme.Space.xxl,
        ]

        XCTAssertEqual(scale, scale.sorted())
        XCTAssertEqual(scale.first, 4)
        XCTAssertEqual(scale.last, 48)
    }

    func testRadiusScaleIsAscending() {
        let scale = [
            Theme.Radius.xs, Theme.Radius.sm, Theme.Radius.md,
            Theme.Radius.lg, Theme.Radius.xl,
        ]

        XCTAssertEqual(scale, scale.sorted())
    }

    func testStatusKindsUseDistinctColors() {
        let recording = components(StatusKind.recording.color, in: .darkAqua)
        let success = components(StatusKind.success.color, in: .darkAqua)

        XCTAssertNotEqual(recording.r, success.r, accuracy: 0.001)
    }
}
