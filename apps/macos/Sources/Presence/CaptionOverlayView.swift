import SwiftUI

/// Накладка с субтитрами поверх остальных приложений (ТЗ редизайна §4.6).
///
/// Задача — не мешать. Ни рамки окна, ни заголовка: полупрозрачная
/// плашка, которую видно поверх Zoom, и которую можно утащить мышью за
/// любое место.
struct CaptionOverlayView: View {
    let lines: [CaptionLine]
    let isRecording: Bool
    let showsSpeaker: Bool
    let opacity: Double
    let onStop: () -> Void

    @State private var isHovering = false
    @State private var pulses = false

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.sm) {
            recordingDot
            captions
            Spacer(minLength: 0)
            // Управление появляется только под курсором: постоянная
            // кнопка поверх чужого окна — это шум.
            if isHovering {
                Button(action: onStop) {
                    Image(systemName: "stop.fill")
                }
                .buttonStyle(.themedIcon)
                .help(String(localized: "Stop recording"))
            }
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.vertical, Theme.Space.sm)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.lg)
                .fill(Theme.surfaceRoot.opacity(opacity))
        )
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.lg)
                .strokeBorder(Theme.borderDefault, lineWidth: 1)
        )
        .onHover { isHovering = $0 }
        .onAppear { pulses = true }
    }

    /// Пульсирующая точка: единственный признак записи, когда главное
    /// окно свёрнуто, поэтому она должна быть заметна периферийным
    /// зрением, но не мигать резко.
    private var recordingDot: some View {
        Circle()
            .fill(isRecording ? Theme.error : Theme.textTertiary)
            .frame(width: 10, height: 10)
            .scaleEffect(isRecording && pulses ? 1 : 0.65)
            .opacity(isRecording && pulses ? 1 : 0.45)
            .animation(
                isRecording
                    ? .easeInOut(duration: 0.9).repeatForever(autoreverses: true)
                    : .default,
                value: pulses
            )
            .padding(.top, Theme.Space.xxs)
            .accessibilityLabel(
                isRecording
                    ? String(localized: "Recording")
                    : String(localized: "Not recording")
            )
    }

    private var captions: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            if lines.isEmpty {
                Text("Listening…")
                    .font(Theme.Text.bodyLarge)
                    .foregroundStyle(Theme.textTertiary)
            } else {
                ForEach(lines) { line in
                    VStack(alignment: .leading, spacing: 0) {
                        if showsSpeaker {
                            Text(line.speaker.label)
                                .font(Theme.Text.caption.weight(.semibold))
                                .foregroundStyle(
                                    line.speaker == .you ? Theme.accent : Theme.info
                                )
                        }
                        Text(line.text)
                            .font(Theme.Text.bodyLarge)
                            .foregroundStyle(
                                line.phase == .partial ? Theme.textSecondary : Theme.textPrimary
                            )
                            .lineLimit(2)
                    }
                }
            }
        }
    }
}
