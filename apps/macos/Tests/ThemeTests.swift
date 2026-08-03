@testable import MeetingRaft
import SwiftUI
import XCTest

/// Токены — единственное место, где заданы цвета и метрика; тест ловит
/// опечатку в шестнадцатеричном литерале, которую глазом не увидеть.
final class ThemeTests: XCTestCase {
    private func components(_ color: Color) -> (r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) {
        let native = NSColor(color).usingColorSpace(.sRGB) ?? .black
        var r: CGFloat = 0
        var g: CGFloat = 0
        var b: CGFloat = 0
        var a: CGFloat = 0
        native.getRed(&r, green: &g, blue: &b, alpha: &a)
        return (r, g, b, a)
    }

    func testHexInitParsesChannelsInOrder() {
        let parts = components(Color(hex: 0x4A9FD8))

        XCTAssertEqual(parts.r, 74.0 / 255, accuracy: 0.01)
        XCTAssertEqual(parts.g, 159.0 / 255, accuracy: 0.01)
        XCTAssertEqual(parts.b, 216.0 / 255, accuracy: 0.01)
        XCTAssertEqual(parts.a, 1, accuracy: 0.01)
    }

    func testHexInitAppliesOpacity() {
        let parts = components(Color(hex: 0x000000, opacity: 0.19))

        XCTAssertEqual(parts.a, 0.19, accuracy: 0.01)
    }

    func testAccentMatchesSpec() {
        let accent = components(Theme.accent)
        let expected = components(Color(hex: 0x4A9FD8))

        XCTAssertEqual(accent.r, expected.r, accuracy: 0.001)
        XCTAssertEqual(accent.g, expected.g, accuracy: 0.001)
        XCTAssertEqual(accent.b, expected.b, accuracy: 0.001)
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
        let recording = components(StatusKind.recording.color)
        let success = components(StatusKind.success.color)

        XCTAssertNotEqual(recording.r, success.r, accuracy: 0.001)
    }
}
