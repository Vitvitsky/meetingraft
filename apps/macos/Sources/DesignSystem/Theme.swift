import AppKit
import SwiftUI

/// Токены оформления (ТЗ редизайна §2).
///
/// Значения заданы один раз и только здесь: экраны не имеют права
/// придумывать свои цвета и отступы, иначе визуальный язык расходится
/// быстрее, чем его успевают править.
///
/// Тем две. За каждым цветом стоит `NSColor` с провайдером: значение
/// выбирается по `NSAppearance` той вьюхи, где цвет рисуется, поэтому ни
/// одному экрану не нужно знать о теме. Светлая палитра посчитана по
/// контрасту, а не подобрана — разбор в спеке
/// `docs/superpowers/specs/2026-08-20-light-theme-and-honest-ui-design.md`.
enum Theme {
    // MARK: - Поверхности

    /// Фон окна.
    static let surfaceRoot = dynamic(light: 0xFFFFFF, dark: 0x0D0D0F)
    /// Панели и toolbar.
    static let surface = dynamic(light: 0xF5F5F7, dark: 0x141416)
    /// Карточки и приподнятые строки.
    static let surfaceElevated = dynamic(light: 0xFFFFFF, dark: 0x1C1C1F)
    /// Всплывающие панели.
    static let surfaceOverlay = dynamic(light: 0xFFFFFF, dark: 0x242428)
    /// Стекло поверх контента. В светлой теме белое и куда плотнее:
    /// чёрная вуаль на светлом фоне читается как грязь, а не как стекло.
    static let surfaceGlass = dynamic(
        light: 0xFFFFFF, dark: 0x000000,
        lightOpacity: 0.72, darkOpacity: 0.19
    )

    // MARK: - Текст

    static let textPrimary = dynamic(light: 0x1D1D1F, dark: 0xFFFFFF)
    static let textSecondary = dynamic(light: 0x6E6E73, dark: 0xA1A1A6)
    static let textTertiary = dynamic(light: 0x7C7C82, dark: 0x6E6E73)
    static let textDisabled = dynamic(light: 0xC7C7CC, dark: 0x48484D)

    // MARK: - Акцент и статусы

    /// Системный синий Apple. В светлой теме на шаг темнее системного
    /// `#007AFF`: тот даёт на белом 4.02:1, ниже порога 4.5:1 для
    /// обычного текста, а этим цветом красятся подписи в 10–13 пунктов.
    static let accent = dynamic(light: 0x0069D9, dark: 0x0A84FF)
    /// Подложка выбранного элемента.
    static let accentBackground = dynamic(
        light: 0x0069D9, dark: 0x0A84FF,
        lightOpacity: 0.10, darkOpacity: 0.09
    )
    static let success = dynamic(light: 0x1D7A32, dark: 0x30D158)
    /// В светлой теме оранжевое: жёлтое на белом даёт 1.41:1.
    static let warning = dynamic(light: 0xB25000, dark: 0xFFD60A)
    static let error = dynamic(light: 0xD70015, dark: 0xFF453A)
    static let info = dynamic(light: 0x0071A4, dark: 0x64D2FF)

    // MARK: - Границы

    static let borderSubtle = dynamic(
        light: 0x000000, dark: 0xFFFFFF, lightOpacity: 0.06, darkOpacity: 0.06
    )
    static let borderDefault = dynamic(
        light: 0x000000, dark: 0xFFFFFF, lightOpacity: 0.10, darkOpacity: 0.10
    )
    static let borderStrong = dynamic(
        light: 0x000000, dark: 0xFFFFFF, lightOpacity: 0.14, darkOpacity: 0.14
    )

    // MARK: - Разрешение по теме

    /// Цвет, знающий обе темы.
    ///
    /// `NSColor` собирается из компонент напрямую, а не через
    /// `NSColor(Color(hex:))`: провайдер зовётся при отрисовке, и путь
    /// через SwiftUI протаскивал бы туда лишний слой преобразования.
    private static func dynamic(
        light: UInt32,
        dark: UInt32,
        lightOpacity: Double = 1,
        darkOpacity: Double = 1
    ) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return srgb(isDark ? dark : light, isDark ? darkOpacity : lightOpacity)
        })
    }

    private static func srgb(_ hex: UInt32, _ opacity: Double) -> NSColor {
        NSColor(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: CGFloat(opacity)
        )
    }

    // MARK: - Типографика

    /// Системный SF Pro, и это решение, а не временная замена.
    /// Предписание ТЗ §2.2 брать Inter отменено 2026-08-20: человеку
    /// понравился ровно системный шрифт, а Inter увёл бы от него и добавил
    /// файл в бандл.
    enum Text {
        static let caption = Font.system(size: 10, weight: .regular)
        static let bodySmall = Font.system(size: 12, weight: .regular)
        static let body = Font.system(size: 13, weight: .regular)
        static let bodyLarge = Font.system(size: 15, weight: .regular)
        static let title = Font.system(size: 17, weight: .semibold)
        static let headline = Font.system(size: 20, weight: .semibold)
        static let large = Font.system(size: 28, weight: .semibold)
        static let extraLarge = Font.system(size: 34, weight: .bold)

        /// Моноширинный: латентность, идентификаторы, тайм-коды.
        static func mono(size: CGFloat = 12, weight: Font.Weight = .regular) -> Font {
            .system(size: size, weight: weight, design: .monospaced)
        }
    }

    // MARK: - Метрика

    enum Space {
        static let xxs: CGFloat = 4
        static let xs: CGFloat = 8
        static let sm: CGFloat = 12
        static let md: CGFloat = 16
        static let lg: CGFloat = 24
        static let xl: CGFloat = 32
        static let xxl: CGFloat = 48
    }

    enum Radius {
        static let xs: CGFloat = 3
        static let sm: CGFloat = 6
        static let md: CGFloat = 8
        static let lg: CGFloat = 12
        static let xl: CGFloat = 16
        /// Пилюля: берётся половина высоты элемента.
        static let pill: CGFloat = 999
    }
}

extension Color {
    /// Цвет из `0xRRGGBB`.
    ///
    /// Токены записаны шестнадцатеричными литералами, как в макете, —
    /// так расхождение с дизайном видно глазом, без пересчёта в доли.
    init(hex: UInt32, opacity: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
        )
    }
}
