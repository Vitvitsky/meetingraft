import SwiftUI

/// Чип: язык, движок, короткая метка (ТЗ редизайна §2.4).
struct Chip: View {
    let text: String
    var isSelected = false
    var tint: Color = Theme.textSecondary

    var body: some View {
        Text(text)
            .font(Theme.Text.bodySmall)
            .foregroundStyle(isSelected ? Theme.accent : tint)
            .padding(.horizontal, Theme.Space.xs)
            .padding(.vertical, Theme.Space.xxs)
            .background(
                Capsule().fill(isSelected ? Theme.accentBackground : Theme.surfaceElevated)
            )
            .overlay(
                Capsule().strokeBorder(isSelected ? Theme.accent : Theme.borderSubtle, lineWidth: 1)
            )
    }
}

/// Что означает бейдж; от этого зависит цвет.
enum StatusKind {
    case recording
    case success
    case warning
    case failure
    case neutral

    var color: Color {
        switch self {
        case .recording, .failure: Theme.error
        case .success: Theme.success
        case .warning: Theme.warning
        case .neutral: Theme.textSecondary
        }
    }
}

/// Статусный бейдж: REC, Final, ошибка.
///
/// Цвет всегда сопровождается текстом: полагаться на один цвет нельзя —
/// его не различает часть пользователей и не передаёт VoiceOver.
struct StatusBadge: View {
    let text: String
    var kind: StatusKind = .neutral
    /// Точка вместо иконки — для состояния записи.
    var showsDot = false

    var body: some View {
        HStack(spacing: Theme.Space.xxs) {
            if showsDot {
                Circle()
                    .fill(Theme.textPrimary)
                    .frame(width: 6, height: 6)
            }
            Text(text)
                .font(Theme.Text.caption.weight(.semibold))
        }
        .foregroundStyle(kind == .neutral ? kind.color : Theme.textPrimary)
        .padding(.horizontal, Theme.Space.xs)
        .padding(.vertical, Theme.Space.xxs)
        .background(
            Capsule().fill(kind == .neutral ? Theme.surfaceElevated : kind.color)
        )
        .accessibilityElement(children: .combine)
        .accessibilityLabel(text)
    }
}

/// Строка настроек: подпись, контрол, пояснение под ними.
///
/// Пояснение — часть строки, а не отдельный `Text` рядом: иначе оно
/// теряет связь с контролом и в VoiceOver читается оторванно.
struct SettingsRow<Control: View>: View {
    let title: String
    var caption: String?
    @ViewBuilder let control: () -> Control

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            HStack(alignment: .firstTextBaseline, spacing: Theme.Space.sm) {
                Text(title)
                    .font(Theme.Text.body)
                    .foregroundStyle(Theme.textPrimary)
                Spacer(minLength: Theme.Space.sm)
                control()
            }
            if let caption, !caption.isEmpty {
                Text(caption)
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.vertical, Theme.Space.xs)
        .accessibilityElement(children: .contain)
    }
}

/// Карточка: приподнятая поверхность с рамкой.
struct Card<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        content()
            .padding(Theme.Space.md)
            .background(
                RoundedRectangle(cornerRadius: Theme.Radius.lg)
                    .fill(Theme.surfaceElevated)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.lg)
                    .strokeBorder(Theme.borderSubtle, lineWidth: 1)
            )
    }
}

/// Полоса происхождения данных внизу панели.
struct ProvenanceBar: View {
    let text: String

    var body: some View {
        Text(text)
            .font(Theme.Text.bodySmall)
            .foregroundStyle(Theme.textTertiary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Theme.Space.md)
            .padding(.vertical, Theme.Space.xs)
            .background(Theme.surface)
            .overlay(alignment: .top) {
                Rectangle()
                    .fill(Theme.borderSubtle)
                    .frame(height: 1)
            }
    }
}
