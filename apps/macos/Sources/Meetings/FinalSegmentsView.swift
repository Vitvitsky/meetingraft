import AppKit
import SwiftUI

/// Final по сегментам: имя участника, тайм-код, текст.
///
/// Переназначение живёт прямо здесь — там, где ошибка и видна. Уводить
/// его на отдельный экран значило бы заставлять человека помнить номер
/// реплики.
struct FinalSegmentsView: View {
    @Bindable var viewModel: SpeakerAttributionViewModel
    @State private var player = SegmentAudioPlayer()
    /// Какая реплика сейчас играет.
    @State private var playingIndex: UInt32?

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.sm) {
            if !viewModel.unappliedEdits.isEmpty {
                UnappliedEditsBanner(
                    edits: viewModel.unappliedEdits,
                    onPlay: { edit in
                        player.play(fragment: viewModel.audioFragment(
                            channelCode: edit.channel,
                            startMs: edit.startMs,
                            endMs: edit.endMs
                        ))
                    },
                    onCopy: { edit in
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(edit.editedText, forType: .string)
                    },
                    onDismiss: { viewModel.dismissUnapplied(id: $0.id) }
                )
                .padding(.horizontal, Theme.Space.sm)
            }
            if !viewModel.segments.isEmpty {
                EditHintBar()
            }
            List(viewModel.segments, id: \.index) { segment in
                FinalSegmentRow(
                    segment: segment,
                    speakers: viewModel.speakers,
                    isEditing: viewModel.editingIndex == segment.index,
                    canPromote: viewModel.canPromote(index: segment.index),
                    draft: $viewModel.draftText,
                    loadFragment: { viewModel.audioFragment(for: segment) },
                    audioAvailable: viewModel.audioAvailable,
                    // Играет ли **эта** реплика, а не хоть какая-то:
                    // кнопка теперь на каждой строке, и общий признак
                    // проигрывателя показал бы «Стоп» сразу на всех.
                    isPlaying: player.isPlaying && playingIndex == segment.index,
                    onPlay: {
                        playingIndex = segment.index
                        player.play(fragment: $0)
                    },
                    onStopPlayback: {
                        playingIndex = nil
                        player.stop()
                    },
                    onFragmentMissing: { viewModel.reportMissingFragment() },
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
}

/// Строка о том, что здесь можно править.
///
/// Правка по нажатию на текст ничем себя не выдавала: реплика выглядит
/// обычным абзацем, и узнать о правке можно было только случайно. Молча
/// спрятанная возможность в этом смысле не лучше молчаливого отказа —
/// человек уверен, что функции нет.
private struct EditHintBar: View {
    var body: some View {
        Label(
            "Click a line to edit its text. ▶ under the timecode plays it without opening the editor",
            systemImage: "hand.tap"
        )
        .font(Theme.Text.caption)
        .foregroundStyle(Theme.textTertiary)
        .padding(.horizontal, Theme.Space.sm)
    }
}

private struct FinalSegmentRow: View {
    let segment: FfiFinalSegment
    let speakers: [FfiSpeaker]
    let isEditing: Bool
    let canPromote: Bool
    @Binding var draft: String
    /// Звук берётся по требованию, а не на каждую перерисовку: это
    /// чтение с диска, а список перерисовывается на каждое нажатие
    /// клавиши в поле.
    let loadFragment: () -> FfiAudioFragment
    /// У встречи есть запись. Нет — кнопки прослушивания нет вовсе.
    let audioAvailable: Bool
    let isPlaying: Bool
    let onPlay: (FfiAudioFragment) -> Void
    let onStopPlayback: () -> Void
    /// Звука за этот кусок не оказалось — сказать об этом вслух.
    let onFragmentMissing: () -> Void
    let onAssign: (String) -> Void
    let onUnpin: () -> Void
    let onBeginEdit: () -> Void
    let onCommitEdit: () -> Void
    let onCancelEdit: () -> Void
    let onRevert: () -> Void
    let onPromote: () -> Void
    @FocusState private var isFieldFocused: Bool
    @State private var isConfirmingPromote = false
    /// Курсор над репликой: показываем карандаш.
    @State private var isHovered = false
    /// Звука за этот диапазон не нашлось — кнопку убираем до следующего
    /// чтения списка. Показывать её дальше значило бы держать в
    /// интерфейсе заведомо нерабочее.
    @State private var fragmentMissing = false

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.sm) {
            // Время и звук — одна колонка: обе отвечают на «где это в
            // записи», и обе не про текст.
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(SpeakerFormat.timecode(ms: segment.startMs))
                    .font(Theme.Text.mono())
                    .foregroundStyle(Theme.textTertiary)
                if audioAvailable, !fragmentMissing {
                    playButton
                }
            }
            .frame(width: 52, alignment: .leading)

            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                header
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
                            if !focused, isEditing {
                                onCommitEdit()
                            }
                        }
                    if !segment.originalText.isEmpty || canPromote {
                        editingBar
                    }
                } else {
                    // Карандаш по наведению: он и подсказывает, что
                    // реплика правится нажатием, и не шумит, пока курсор
                    // в другом месте.
                    HStack(alignment: .top, spacing: Theme.Space.xs) {
                        Text(segment.text)
                            .font(Theme.Text.body)
                            .foregroundStyle(Theme.textPrimary)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                        Image(systemName: "pencil")
                            .font(Theme.Text.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .opacity(isHovered ? 1 : 0)
                            .accessibilityHidden(true)
                    }
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onBeginEdit)
                    .onHover { isHovered = $0 }
                    .help("Click to edit the text of this line")
                }
                if segment.textEdited, !isEditing {
                    Text("was: \(segment.originalText)")
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
                Button("Restore original", action: onRevert)
                    .buttonStyle(.themedSecondary)
            }
            if canPromote {
                Button("Replace everywhere") { isConfirmingPromote = true }
                    .buttonStyle(.themedSecondary)
            }
            Spacer()
        }
        .padding(.top, Theme.Space.xxs)
        .confirmationDialog(
            "Replace everywhere in this meeting?",
            isPresented: $isConfirmingPromote,
            titleVisibility: .visible
        ) {
            Button("Replace everywhere", role: .destructive, action: onPromote)
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "Every match in this meeting will be replaced. Each changed line gets your edit mark, and undoing them is one at a time."
            )
        }
    }

    /// Шапка реплики: кто говорит и пометки.
    private var header: some View {
        HStack(spacing: Theme.Space.xs) {
            speakerMenu
            Spacer(minLength: 0)
        }
    }

    /// Прослушивание — под тайм-кодом и иконкой.
    ///
    /// Не в панели правки, потому что это разные задачи: чтобы понять,
    /// кто говорит, реплику надо послушать, не входя в правку текста и
    /// не рискуя его задеть. И не подписью — на каждой строке подпись
    /// превратила бы транскрипт в панель кнопок.
    private var playButton: some View {
        Button(action: togglePlayback) {
            Image(systemName: isPlaying ? "stop.fill" : "play.fill")
                .font(.system(size: 9))
        }
        .buttonStyle(.borderless)
        .foregroundStyle(isPlaying ? Theme.accent : Theme.textTertiary)
        .help(isPlaying ? "Stop" : "Play this line")
        .accessibilityLabel(isPlaying ? "Stop" : "Play the line")
    }

    /// Звук читается с диска по нажатию, а не при отрисовке: список
    /// перерисовывается на каждое нажатие клавиши в поле правки.
    private func togglePlayback() {
        if isPlaying {
            onStopPlayback()
            return
        }
        let fragment = loadFragment()
        guard SegmentAudioPlayer.buffer(from: fragment) != nil else {
            fragmentMissing = true
            onFragmentMissing()
            return
        }
        onPlay(fragment)
    }

    private var speakerMenu: some View {
        HStack(spacing: Theme.Space.xs) {
            Menu {
                ForEach(speakers, id: \.id) { speaker in
                    Button(speaker.displayName) { onAssign(speaker.id) }
                }
                if segment.source == .human {
                    Divider()
                    Button("Back to the channel") { onUnpin() }
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
            if segment.source == .human {
                Chip(text: String(localized: "edit"))
            }
            // Подпись слепком отличается от подписи человеком (ADR-013).
            // Одинаковые они выглядели бы одинаково достоверными, и
            // доверие к именам пришлось бы строить на вере.
            if segment.source == .voiceprint {
                Chip(text: String(localized: "voice"))
            }
            // Две пометки различаются словом: «правка» уже занята
            // ручным назначением спикера, и одинаковые чипы рядом были
            // бы неразличимы.
            if segment.textEdited {
                Chip(text: String(localized: "text"))
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
