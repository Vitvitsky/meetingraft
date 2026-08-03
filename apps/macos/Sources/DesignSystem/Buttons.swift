import SwiftUI

/// Кнопки design kit (ТЗ редизайна §2.4).
///
/// Стили, а не обёртки-вью: `ButtonStyle` сохраняет родное поведение
/// кнопки — фокус, клавиатуру, доступность, — и его можно навесить на
/// любую существующую кнопку без переписывания вызова.
enum ButtonRole {
    case primary
    case secondary
    case tertiary
    case destructive

    fileprivate var foreground: Color {
        switch self {
        case .primary: Theme.textPrimary
        case .secondary: Theme.textPrimary
        case .tertiary: Theme.textSecondary
        case .destructive: Theme.textPrimary
        }
    }

    fileprivate var background: Color {
        switch self {
        case .primary: Theme.accent
        case .secondary: Theme.surfaceElevated
        case .tertiary: .clear
        case .destructive: Theme.error
        }
    }

    fileprivate var border: Color {
        switch self {
        case .primary, .destructive: .clear
        case .secondary: Theme.borderDefault
        case .tertiary: .clear
        }
    }
}

struct ThemedButtonStyle: ButtonStyle {
    let role: ButtonRole
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.Text.body)
            .foregroundStyle(isEnabled ? role.foreground : Theme.textDisabled)
            .padding(.horizontal, Theme.Space.sm)
            .padding(.vertical, Theme.Space.xs)
            .background(
                RoundedRectangle(cornerRadius: Theme.Radius.md)
                    .fill(role.background)
                    .opacity(isEnabled ? 1 : 0.4)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.md)
                    .strokeBorder(role.border, lineWidth: 1)
            )
            // Нажатие показываем прозрачностью, а не смещением: строки
            // списка от сдвига дёргаются.
            .opacity(configuration.isPressed ? 0.75 : 1)
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }
}

/// Кнопка-иконка: квадратная, без подписи.
struct IconButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .labelStyle(.iconOnly)
            .font(Theme.Text.bodyLarge)
            .foregroundStyle(isEnabled ? Theme.textSecondary : Theme.textDisabled)
            .frame(width: 28, height: 28)
            .background(
                RoundedRectangle(cornerRadius: Theme.Radius.sm)
                    .fill(configuration.isPressed ? Theme.surfaceElevated : .clear)
            )
            .contentShape(Rectangle())
    }
}

extension ButtonStyle where Self == ThemedButtonStyle {
    static var themedPrimary: ThemedButtonStyle {
        ThemedButtonStyle(role: .primary)
    }

    static var themedSecondary: ThemedButtonStyle {
        ThemedButtonStyle(role: .secondary)
    }

    static var themedTertiary: ThemedButtonStyle {
        ThemedButtonStyle(role: .tertiary)
    }

    static var themedDestructive: ThemedButtonStyle {
        ThemedButtonStyle(role: .destructive)
    }
}

extension ButtonStyle where Self == IconButtonStyle {
    static var themedIcon: IconButtonStyle {
        IconButtonStyle()
    }
}
