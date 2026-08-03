import SwiftUI

/// Токены оформления (ТЗ редизайна §2).
///
/// Значения заданы один раз и только здесь: экраны не имеют права
/// придумывать свои цвета и отступы, иначе визуальный язык расходится
/// быстрее, чем его успевают править.
///
/// Тема пока одна — тёмная. Светлую палитру ТЗ выносит за скобки (§8),
/// поэтому переключателя нет, а окно принудительно тёмное: смешение
/// системной светлой темы с этими токенами дало бы нечитаемый контраст.
enum Theme {
    // MARK: - Поверхности

    /// Фон окна.
    static let surfaceRoot = Color(hex: 0x0D0D0F)
    /// Панели и toolbar.
    static let surface = Color(hex: 0x141416)
    /// Карточки и приподнятые строки.
    static let surfaceElevated = Color(hex: 0x1C1C1F)
    /// Всплывающие панели.
    static let surfaceOverlay = Color(hex: 0x242428)
    /// Стекло поверх контента.
    static let surfaceGlass = Color(hex: 0x000000, opacity: 0.19)

    // MARK: - Текст

    static let textPrimary = Color(hex: 0xFFFFFF)
    static let textSecondary = Color(hex: 0xA1A1A6)
    static let textTertiary = Color(hex: 0x6E6E73)
    static let textDisabled = Color(hex: 0x48484D)

    // MARK: - Акцент и статусы

    static let accent = Color(hex: 0x4A9FD8)
    /// Подложка выбранного элемента.
    static let accentBackground = Color(hex: 0x4A9FD8, opacity: 0.09)
    static let success = Color(hex: 0x30D158)
    static let warning = Color(hex: 0xFFD60A)
    static let error = Color(hex: 0xFF453A)
    static let info = Color(hex: 0x64D2FF)

    // MARK: - Границы

    static let borderSubtle = Color.white.opacity(0.06)
    static let borderDefault = Color.white.opacity(0.10)
    static let borderStrong = Color.white.opacity(0.14)

    // MARK: - Типографика

    /// Шрифты не бандлятся: Inter и IBM Plex Mono заменены системными
    /// (ТЗ §9). Решение сменить их — отдельное, и менять придётся только
    /// здесь.
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
