import SwiftUI

/// Вкладка **Speakers**: кто говорил на встрече и сколько.
///
/// Ручных меток без связи с транскриптом здесь больше нет: атрибуция
/// идёт по каналам захвата (ADR-012), а экран показывает её результат и
/// даёт назначить имя всему каналу разом.
struct SpeakersPanelView: View {
    @Bindable var viewModel: SpeakerAttributionViewModel
    let primaryLanguage: String

    var body: some View {
        VStack(spacing: 0) {
            channelBar

            Divider()

            List {
                ForEach(viewModel.rows) { row in
                    SpeakerStatRow(
                        row: row,
                        onRename: { viewModel.rename(id: row.id, displayName: $0) },
                        onDelete: { viewModel.remove(id: row.id) }
                    )
                    .listRowSeparator(.hidden)
                }
            }
            .overlay {
                if viewModel.rows.isEmpty {
                    ContentUnavailableView(
                        "Участников нет",
                        systemImage: "person.3",
                        description: Text(
                            "Спикеры появятся после пересбора Final: "
                                + "имена привязываются к дорожкам записи."
                        )
                    )
                }
            }
        }
    }

    /// Назначение по каналу — основная операция: один выбор подписывает
    /// весь транскрипт.
    private var channelBar: some View {
        HStack(spacing: Theme.Space.sm) {
            if viewModel.canAssign {
                channelMenu(code: "mic")
                channelMenu(code: "system")
            } else {
                Text("Назначение по дорожкам доступно после пересбора Final")
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(Theme.textTertiary)
            }
            Spacer()
            Button("Add", systemImage: "person.badge.plus") {
                viewModel.addSpeaker(primaryLanguage: primaryLanguage)
            }
        }
        .padding(Theme.Space.md)
    }

    private func channelMenu(code: String) -> some View {
        let assigned = viewModel.channelSpeakerName(code)
        return Menu {
            ForEach(viewModel.speakers, id: \.id) { speaker in
                Button(speaker.displayName) {
                    viewModel.assignChannel(code, to: speaker.id)
                }
            }
        } label: {
            HStack(spacing: Theme.Space.xxs) {
                Text(SpeakerFormat.channelLabel(code))
                    .foregroundStyle(Theme.textSecondary)
                Text(assigned.isEmpty ? "—" : assigned)
                    .foregroundStyle(Theme.textPrimary)
            }
            .font(Theme.Text.bodySmall)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .disabled(viewModel.speakers.isEmpty)
        .help("Подписать все реплики дорожки \(SpeakerFormat.channelLabel(code))")
    }
}

/// Строка участника: имя, дорожка, доля речи, число реплик.
private struct SpeakerStatRow: View {
    let row: SpeakerRowModel
    let onRename: (String) -> Void
    let onDelete: () -> Void

    @State private var displayName: String

    init(
        row: SpeakerRowModel,
        onRename: @escaping (String) -> Void,
        onDelete: @escaping () -> Void
    ) {
        self.row = row
        self.onRename = onRename
        self.onDelete = onDelete
        _displayName = State(initialValue: row.displayName)
    }

    var body: some View {
        HStack(spacing: Theme.Space.sm) {
            avatar

            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                TextField("Имя участника", text: $displayName)
                    .textFieldStyle(.plain)
                    .font(Theme.Text.body)
                    .onSubmit { onRename(displayName) }
                HStack(spacing: Theme.Space.xs) {
                    if !row.channelCode.isEmpty {
                        Chip(text: SpeakerFormat.channelLabel(row.channelCode))
                    }
                    Text(SpeakerFormat.shareText(row.share))
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textSecondary)
                    Text(SpeakerFormat.durationText(ms: row.speakingMs))
                        .font(Theme.Text.mono())
                        .foregroundStyle(Theme.textTertiary)
                    Text(SpeakerFormat.segmentCountText(row.segmentCount))
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textTertiary)
                }
            }

            Spacer(minLength: Theme.Space.sm)

            Button("Delete", systemImage: "trash", role: .destructive) {
                onDelete()
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .help("Удалить участника и снять его со всех реплик")
        }
        .padding(.vertical, Theme.Space.xxs)
        .onChange(of: row.displayName) { _, newValue in
            displayName = newValue
        }
    }

    private var avatar: some View {
        Text(SpeakerFormat.initial(row.label))
            .font(Theme.Text.body.weight(.semibold))
            .foregroundStyle(Theme.textPrimary)
            .frame(width: 28, height: 28)
            .background(Circle().fill(Theme.surfaceOverlay))
    }
}
