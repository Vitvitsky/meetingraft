import SwiftUI

/// Состояние разрешений до начала записи — рядом с кнопкой «запись».
///
/// Показывается **только когда есть что сказать**. Плашка «разрешения
/// выданы» была бы украшением: человек и так нажмёт запись, и она пойдёт.
/// А вот запрет микрофона до этой правки не был виден нигде — узнать о
/// нём можно было, лишь нажав запись и получив строку ошибки.
struct CaptureReadinessBadge: View {
    let readiness: CaptureReadiness

    var body: some View {
        switch readiness {
        case .ready:
            EmptyView()
        case .microphoneWillBeAsked:
            // Не предупреждение, а объяснение: ничего чинить не нужно,
            // просто первый запуск спросит разрешение.
            label(
                icon: "questionmark.circle",
                text: String(localized: "Microphone access will be requested on the first recording"),
                tint: Theme.textTertiary
            )
        case .microphoneDenied:
            label(
                icon: "mic.slash",
                text: String(localized: "Microphone is denied — recording will not start"),
                tint: Theme.error,
                action: (String(localized: "Open Settings"), SystemAudioSettingsLink.openMicrophone)
            )
        case let .systemAudioUnavailable(status):
            // Своего текста здесь нет намеренно: причины уже описаны в
            // соседней плашке, и второй набор объяснений разошёлся бы с
            // первым.
            SystemAudioUnavailableBadge(status: status)
        }
    }

    private func label(
        icon: String,
        text: String,
        tint: Color,
        action: (title: String, run: () -> Void)? = nil
    ) -> some View {
        HStack(spacing: Theme.Space.xs) {
            Image(systemName: icon)
            Text(text)
            if let action {
                Button(action.title) { action.run() }
                    .buttonStyle(.link)
            }
        }
        .font(Theme.Text.bodySmall)
        .foregroundStyle(tint)
        .accessibilityElement(children: .combine)
    }
}
