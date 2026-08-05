import SwiftUI

/// Правки, не легшие ни на одну версию после пересбора.
///
/// Стоит над списком и появляется только когда есть что показать:
/// постоянный раздел был бы пустым почти всегда. Прятать это в конец
/// списка нельзя — сообщение означает «часть вашей ручной работы
/// отвалилась», и оно должно попадаться на глаза само.
struct UnappliedEditsBanner: View {
    let edits: [FfiSegmentEdit]
    let onPlay: (FfiSegmentEdit) -> Void
    let onCopy: (FfiSegmentEdit) -> Void
    let onDismiss: (FfiSegmentEdit) -> Void

    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Button {
                isExpanded.toggle()
            } label: {
                HStack(spacing: Theme.Space.xs) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(Theme.warning)
                    Text(title)
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textPrimary)
                    Spacer()
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            .buttonStyle(.plain)

            if isExpanded {
                ForEach(edits, id: \.id) { edit in
                    card(edit)
                }
            }
        }
        .padding(Theme.Space.sm)
        .background(
            Theme.warning.opacity(0.10),
            in: RoundedRectangle(cornerRadius: Theme.Radius.md)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.md)
                .stroke(Theme.warning.opacity(0.45), lineWidth: 1)
        )
    }

    private var title: String {
        edits.count == 1
            ? "1 правка не легла на текущую версию"
            : "\(edits.count) правок не легли на текущую версию"
    }

    private func card(_ edit: FfiSegmentEdit) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            HStack(spacing: Theme.Space.xs) {
                Text(SpeakerFormat.timecode(ms: edit.startMs))
                    .font(Theme.Text.mono())
                Text(SpeakerFormat.channelLabel(edit.channel))
                    .font(Theme.Text.caption)
            }
            .foregroundStyle(Theme.textTertiary)

            Text("было: \(edit.originalText)")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textTertiary)
            Text("стало: \(edit.editedText)")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textPrimary)

            HStack(spacing: Theme.Space.xs) {
                Button("▶ Прослушать") { onPlay(edit) }
                    .buttonStyle(.themedSecondary)
                // Перенести правку на место нельзя: `originalText` служит
                // и поиском при пересборе, и признаком возврата к
                // исходному. Поэтому копируем текст, а правится нужный
                // сегмент обычным путём.
                Button("Скопировать текст") { onCopy(edit) }
                    .buttonStyle(.themedSecondary)
                Button("Удалить правку") { onDismiss(edit) }
                    .buttonStyle(.themedSecondary)
                Spacer()
            }
            .padding(.top, Theme.Space.xxs)
        }
        .padding(.vertical, Theme.Space.xs)
    }
}
