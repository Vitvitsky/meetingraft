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
                isEditing: viewModel.editingIndex == segment.index,
                canPromote: viewModel.canPromote(index: segment.index),
                draft: $viewModel.draftText,
                onAssign: { viewModel.assignSegment(index: segment.index, to: $0) },
                onUnpin: { viewModel.unpinSegment(index: segment.index) },
                onBeginEdit: { viewModel.beginEdit(index: segment.index) },
                onCommitEdit: { viewModel.commitEdit() },
                onCancelEdit: { viewModel.cancelEdit() },
                onRevert: { viewModel.revertToOriginal(index: segment.index) },
                onPromote: { viewModel.promoteTerm(index: segment.index) }
            )
            .listRowSeparator(.hidden)
        }
    }
}

private struct FinalSegmentRow: View {
    let segment: FfiFinalSegment
    let speakers: [FfiSpeaker]
    let isEditing: Bool
    let canPromote: Bool
    @Binding var draft: String
    let onAssign: (String) -> Void
    let onUnpin: () -> Void
    let onBeginEdit: () -> Void
    let onCommitEdit: () -> Void
    let onCancelEdit: () -> Void
    let onRevert: () -> Void
    let onPromote: () -> Void
    @FocusState private var isFieldFocused: Bool
    @State private var isConfirmingPromote = false

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.sm) {
            Text(SpeakerFormat.timecode(ms: segment.startMs))
                .font(Theme.Text.mono())
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 52, alignment: .leading)

            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                speakerMenu
                if isEditing {
                    TextField("", text: $draft, axis: .vertical)
                        .textFieldStyle(.plain)
                        .font(Theme.Text.body)
                        .foregroundStyle(Theme.textPrimary)
                        .padding(Theme.Space.xs)
                        .background(
                            Theme.surfaceElevated,
                            in: RoundedRectangle(cornerRadius: Theme.Radius.sm)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.Radius.sm)
                                .stroke(Theme.accent, lineWidth: 1)
                        )
                        .focused($isFieldFocused)
                        .onAppear { isFieldFocused = true }
                        // Enter сохраняет, Esc откатывает, уход фокуса
                        // сохраняет: набранное не должно теряться от
                        // клика мимо поля.
                        .onSubmit(onCommitEdit)
                        .onExitCommand(perform: onCancelEdit)
                        .onChange(of: isFieldFocused) { _, focused in
                            if !focused, isEditing { onCommitEdit() }
                        }
                    editingBar
                } else {
                    Text(segment.text)
                        .font(Theme.Text.body)
                        .foregroundStyle(Theme.textPrimary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .contentShape(Rectangle())
                        .onTapGesture(perform: onBeginEdit)
                }
                if segment.textEdited, !isEditing {
                    Text("было: \(segment.originalText)")
                        .font(Theme.Text.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .padding(.vertical, Theme.Space.xxs)
    }

    /// Действия видны только пока строка правится: постоянный ряд кнопок
    /// на каждой реплике превратил бы транскрипт в панель управления.
    private var editingBar: some View {
        HStack(spacing: Theme.Space.sm) {
            if !segment.originalText.isEmpty {
                Button("Вернуть исходное", action: onRevert)
                    .buttonStyle(.themedSecondary)
            }
            if canPromote {
                Button("Заменять всюду") { isConfirmingPromote = true }
                    .buttonStyle(.themedSecondary)
            }
            Spacer()
        }
        .padding(.top, Theme.Space.xxs)
        .confirmationDialog(
            "Заменять всюду в этой встрече?",
            isPresented: $isConfirmingPromote,
            titleVisibility: .visible
        ) {
            Button("Заменять всюду", role: .destructive, action: onPromote)
            Button("Отмена", role: .cancel) {}
        } message: {
            Text(
                "Все совпадения в этой встрече будут заменены. "
                    + "Каждая изменённая реплика получит пометку вашей правки, "
                    + "и отменить их можно будет только по одной."
            )
        }
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
            // Две пометки различаются словом: «правка» уже занята
            // ручным назначением спикера, и одинаковые чипы рядом были
            // бы неразличимы.
            if segment.textEdited {
                Chip(text: "текст")
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
