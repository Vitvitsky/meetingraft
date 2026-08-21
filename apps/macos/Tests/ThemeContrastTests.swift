@testable import MeetingRaft
import SwiftUI
import XCTest

/// Контраст токенов по WCAG 2.1 в обеих темах.
///
/// Тест существует потому, что палитру нельзя проверить глазом на
/// Linux-машине, где она пишется, и потому что глазом её плохо видно и
/// на Маке: жёлтое `#FFD60A` на белом даёт 1.41:1, и заметно это не
/// раньше, чем на такой надписи споткнётся человек.
///
/// Пороги тирами, а не одним числом. Сплошной порог 4.5:1 покраснел бы на
/// работающей тёмной теме: третичный текст в ней 3.35:1, и это осознанно
/// — им набраны необязательные подписи. Выключенный текст не проверяется
/// вовсе: он обязан читаться выключенным, и WCAG выводит его из-под
/// требования.
///
/// **Зелёный сам по себе тут ничего не значит.** Отрицательный контроль —
/// светлая палитра, собранная переносом статусных цветов тёмной темы как
/// есть, — обязан валить пять цветов из восьми: третичный 3.26, успех
/// 2.02, предупреждение 1.41, ошибка 3.41, инфо 1.72. Числа посчитаны до
/// написания теста; сценарий подмены — `docs/mac-verification.md`.
@MainActor
final class ThemeContrastTests: XCTestCase {
    private struct Token {
        let name: String
        let color: Color
        /// Порог WCAG для этого цвета.
        let floor: Double
    }

    private var foregrounds: [Token] {
        [
            Token(name: "textPrimary", color: Theme.textPrimary, floor: 4.5),
            Token(name: "textSecondary", color: Theme.textSecondary, floor: 4.5),
            Token(name: "textTertiary", color: Theme.textTertiary, floor: 3.0),
            Token(name: "accent", color: Theme.accent, floor: 4.5),
            Token(name: "success", color: Theme.success, floor: 4.5),
            Token(name: "warning", color: Theme.warning, floor: 4.5),
            Token(name: "error", color: Theme.error, floor: 4.5),
            Token(name: "info", color: Theme.info, floor: 4.5),
        ]
    }

    private var surfaces: [(name: String, color: Color)] {
        [
            ("surfaceRoot", Theme.surfaceRoot),
            ("surface", Theme.surface),
            ("surfaceElevated", Theme.surfaceElevated),
        ]
    }

    /// Относительная яркость по WCAG 2.1.
    private func luminance(_ color: Color, in name: NSAppearance.Name) -> Double {
        var r: CGFloat = 0
        var g: CGFloat = 0
        var b: CGFloat = 0
        var a: CGFloat = 0
        NSAppearance(named: name)!.performAsCurrentDrawingAppearance {
            let native = NSColor(color).usingColorSpace(.sRGB) ?? .black
            native.getRed(&r, green: &g, blue: &b, alpha: &a)
        }
        func channel(_ value: CGFloat) -> Double {
            let x = Double(value)
            return x <= 0.03928 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    private func ratio(_ foreground: Color, on background: Color, in name: NSAppearance.Name) -> Double {
        let a = luminance(foreground, in: name)
        let b = luminance(background, in: name)
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }

    private func assertPalette(
        _ name: NSAppearance.Name,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        for token in foregrounds {
            for surface in surfaces {
                let value = ratio(token.color, on: surface.color, in: name)
                XCTAssertGreaterThanOrEqual(
                    value,
                    token.floor,
                    "\(name.rawValue): \(token.name) на \(surface.name) даёт \(String(format: "%.2f", value)):1",
                    file: file,
                    line: line
                )
            }
        }
    }

    func testDarkPaletteMeetsContrastFloors() {
        assertPalette(.darkAqua)
    }

    func testLightPaletteMeetsContrastFloors() {
        assertPalette(.aqua)
    }

    /// Прозрачные токены яркостью не судятся: у стекла и границ фон
    /// просвечивает, и число вышло бы про несуществующий цвет. Тест
    /// стоит здесь, чтобы список проверяемых не разъехался с палитрой
    /// молча — новый непрозрачный цвет обязан попасть в `foregrounds`.
    func testEveryOpaqueForegroundTokenIsAudited() {
        XCTAssertEqual(foregrounds.count, 8)
        XCTAssertEqual(surfaces.count, 3)
    }

    /// Проверка самого счётчика на величинах, которые не зависят от
    /// палитры: чёрное на белом даёт ровно 21:1, белое на белом — 1:1.
    ///
    /// Без неё зелёный тест мог бы означать, что формула всегда
    /// возвращает большое число. Прибор проверяется до того, как ему
    /// верят.
    func testRatioIsCalibratedOnKnownExtremes() {
        let black = Color(hex: 0x000000)
        let white = Color(hex: 0xFFFFFF)

        XCTAssertEqual(ratio(black, on: white, in: .aqua), 21.0, accuracy: 0.01)
        XCTAssertEqual(ratio(white, on: white, in: .aqua), 1.0, accuracy: 0.01)
        // Порядок аргументов не должен влиять: контраст симметричен.
        XCTAssertEqual(ratio(white, on: black, in: .darkAqua), 21.0, accuracy: 0.01)
    }
}
