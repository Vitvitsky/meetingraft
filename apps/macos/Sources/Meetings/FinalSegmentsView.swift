import SwiftUI

/// Final по сегментам: имя участника, тайм-код, текст.
///
/// Переназначение живёт прямо здесь — там, где ошибка и видна. Уводить
/// его на отдельный экран значило бы заставлять человека помнить номер
/// реплики.
struct FinalSegmentsView: View {
    @Bindable var viewModel: SpeakerAttributionViewModel

    var body: some View {
        List(viewModel.segments, id: \.index) { segment in
            FinalSegmentRow(
                segment: segment,
                speakers: viewModel.speakers,
                onAssign: { viewModel.assignSegment(index: segment.index, to: $0) },
                onUnpin: { viewModel.unpinSegment(index: segment.index) }
            )
            .listRowSeparator(.hidden)
        }
    }
}

private struct FinalSegmentRow: View {
    let segment: FfiFinalSegment
    let speakers: [FfiSpeaker]
    let onAssign: (String) -> Void
    let onUnpin: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.sm) {
            Text(SpeakerFormat.timecode(ms: segment.startMs))
                .font(Theme.Text.mono())
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 52, alignment: .leading)

            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                speakerMenu
                Text(segment.text)
                    .font(Theme.Text.body)
                    .foregroundStyle(Theme.textPrimary)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.vertical, Theme.Space.xxs)
    }

    private var speakerMenu: some View {
        HStack(spacing: Theme.Space.xs) {
            Menu {
                ForEach(speakers, id: \.id) { speaker in
                    Button(speaker.displayName) { onAssign(speaker.id) }
                }
                if segment.speakerPinned {
                    Divider()
                    Button("Вернуть под дорожку") { onUnpin() }
                }
            } label: {
                Text(speakerLabel)
                    .font(Theme.Text.bodySmall.weight(.semibold))
                    .foregroundStyle(speakerColor)
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .disabled(speakers.isEmpty)

            // Правку помечаем: иначе непонятно, почему смена имени
            // дорожки эту реплику не задела.
            if segment.speakerPinned {
                Chip(text: "правка")
            }
        }
    }

    private var speakerLabel: String {
        segment.speakerName.isEmpty
            ? SpeakerFormat.channelLabel(segment.channel)
            : segment.speakerName
    }

    /// Не назначенная реплика подписана дорожкой и приглушена: это не
    /// имя, а признание, что имени нет.
    private var speakerColor: Color {
        segment.speakerName.isEmpty ? Theme.textTertiary : Theme.accent
    }
}
