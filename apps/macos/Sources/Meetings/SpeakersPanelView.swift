import SwiftUI

/// Вкладка **Speakers**: кто говорил на встрече и сколько.
///
/// Ручных меток без связи с транскриптом здесь больше нет: атрибуция
/// идёт по каналам захвата (ADR-012), а экран показывает её результат и
/// даёт назначить имя всему каналу разом.
struct SpeakersPanelView: View {
    @Bindable var viewModel: SpeakerAttributionViewModel
    let primaryLanguage: String
    @State private var player = SegmentAudioPlayer()
    /// Какая неопознанная реплика сейчас играет.
    @State private var playingIndex: UInt32?

    var body: some View {
        VStack(spacing: 0) {
            channelBar

            Divider()

            // Панель показывается, только если движок голосов вообще
            // собран и есть что разносить. Без движка кнопка отказывала бы
            // всегда — заглушка в чистом виде; у версии без сегментов
            // разносить нечего.
            if viewModel.voiceEngineAvailable, viewModel.hasSegments {
                VoicePrintBar(viewModel: viewModel)
            }

            List {
                ForEach(viewModel.rows) { row in
                    SpeakerStatRow(
                        row: row,
                        printFor: { viewModel.voicePrints.first { $0.speakerId == row.id } },
                        canRemember: viewModel.canRememberVoice(speakerId: row.id),
                        onRemember: { viewModel.rememberVoice(speakerId: row.id) },
                        onRename: { viewModel.rename(id: row.id, displayName: $0) },
                        onDelete: { viewModel.remove(id: row.id) }
                    )
                    .listRowSeparator(.hidden)
                }

                if !viewModel.unidentifiedSegments.isEmpty {
                    unidentifiedSection
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

    /// Реплики без имени — здесь же, а не на отдельном экране.
    ///
    /// Они и есть работа: человек подписывает несколько, пересчёт разносит
    /// остальные, неразнесённые снова оказываются тут. Уводить этот
    /// список в другое место значило бы разрывать цикл пополам.
    private var unidentifiedSection: some View {
        Section {
            ForEach(viewModel.unidentifiedSegments, id: \.index) { segment in
                UnidentifiedReplyRow(
                    segment: segment,
                    speakers: viewModel.speakers,
                    audioAvailable: viewModel.audioAvailable,
                    isPlaying: player.isPlaying && playingIndex == segment.index,
                    loadFragment: { viewModel.audioFragment(for: segment) },
                    onPlay: {
                        playingIndex = segment.index
                        player.play(fragment: $0)
                    },
                    onStopPlayback: {
                        playingIndex = nil
                        player.stop()
                    },
                    onFragmentMissing: { viewModel.reportMissingFragment() },
                    onAssign: { viewModel.assignSegment(index: segment.index, to: $0) }
                )
                .listRowSeparator(.hidden)
            }
        } header: {
            Text("Unnamed · \(viewModel.unidentifiedSegments.count)")
                .font(Theme.Text.bodySmall.weight(.semibold))
                .foregroundStyle(Theme.textSecondary)
        } footer: {
            Text(
                "Подпишите несколько — по ним складываются слепки, "
                    + "и пересчёт разнесёт похожие."
            )
            .font(Theme.Text.caption)
            .foregroundStyle(Theme.textTertiary)
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
                Text("Assigning by channel becomes available after Final is rebuilt")
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
        .help("Label every line on the \(SpeakerFormat.channelLabel(code)) channel")
    }
}

/// Пересчёт подписей по слепкам голоса (ADR-013).
///
/// Кнопка называет свою цену **до** нажатия: из скольки ручных подписей
/// сложатся слепки и скольким репликам это может дать имя. Кнопка,
/// молчащая о том, что сделает, ничем не лучше молчаливого отказа —
/// человек нажимает вслепую и не может сказать, сработало ли.
private struct VoicePrintBar: View {
    let viewModel: SpeakerAttributionViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            HStack(spacing: Theme.Space.sm) {
                Button("Split by voice", systemImage: "waveform.badge.person") {
                    viewModel.recomputeVoicePrints()
                }
                .disabled(!viewModel.canRecomputeVoicePrints)
                .help(
                    "Сложить слепки по подписанным вручную репликам "
                        + "и разнести по ним остальные"
                )

                Text(costText)
                    .font(Theme.Text.caption)
                    .foregroundStyle(Theme.textTertiary)

                Spacer()
            }

            if viewModel.voicePrintsNeedRecompute {
                // Не поломка и не повод выбросить слепки: сравнивать их с
                // векторами другой модели нельзя, и это всё.
                Label(
                    "Модель голосов сменилась — слепки надо пересчитать, "
                        + "сравнивать со старыми нечего",
                    systemImage: "exclamationmark.triangle"
                )
                .font(Theme.Text.caption)
                .foregroundStyle(Theme.warning)
            }

            if let pass = viewModel.lastPass, pass.error.isEmpty {
                Text(SpeakerFormat.passSummary(pass))
                    .font(Theme.Text.caption)
                    .foregroundStyle(Theme.textSecondary)
            }
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.vertical, Theme.Space.sm)
    }

    /// Что произойдёт по нажатию — либо почему нажимать нечего.
    private var costText: String {
        guard viewModel.canRecomputeVoicePrints else {
            return "Подпишите хотя бы одну реплику вручную — слепки складываются по ним"
        }
        let labelled = viewModel.humanLabelledCount
        let candidates = viewModel.unidentifiedSegments.count
        return "слепки по \(labelled) подписанным · без имени \(candidates)"
    }
}

/// Реплика, оставшаяся без имени: послушать и подписать, не уходя с
/// экрана.
private struct UnidentifiedReplyRow: View {
    let segment: FfiFinalSegment
    let speakers: [FfiSpeaker]
    let audioAvailable: Bool
    let isPlaying: Bool
    let loadFragment: () -> FfiAudioFragment
    let onPlay: (FfiAudioFragment) -> Void
    let onStopPlayback: () -> Void
    let onFragmentMissing: () -> Void
    let onAssign: (String) -> Void

    /// Звука за этот кусок не нашлось — кнопка гаснет после попытки, а не
    /// молча.
    @State private var fragmentMissing = false

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.sm) {
            VStack(spacing: Theme.Space.xxs) {
                Text(SpeakerFormat.timecode(ms: segment.startMs))
                    .font(Theme.Text.mono())
                    .foregroundStyle(Theme.textTertiary)
                if audioAvailable, !fragmentMissing {
                    Button(action: toggle) {
                        Image(systemName: isPlaying ? "stop.fill" : "play.fill")
                            .font(.system(size: 9))
                    }
                    .buttonStyle(.borderless)
                    .foregroundStyle(isPlaying ? Theme.accent : Theme.textTertiary)
                    .help("Play the line — a name is usually recognised by ear")
                    .accessibilityLabel(isPlaying ? "Остановить" : "Прослушать реплику")
                }
            }
            .frame(width: 56)

            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(segment.text)
                    .font(Theme.Text.body)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(3)
                HStack(spacing: Theme.Space.xs) {
                    Chip(text: SpeakerFormat.channelLabel(segment.channel))
                    Text(SpeakerFormat.durationText(ms: durationMs))
                        .font(Theme.Text.mono())
                        .foregroundStyle(Theme.textTertiary)
                }
            }

            Spacer(minLength: Theme.Space.sm)

            Menu("Подписать") {
                ForEach(speakers, id: \.id) { speaker in
                    Button(speaker.displayName) { onAssign(speaker.id) }
                }
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .disabled(speakers.isEmpty)
        }
        .padding(.vertical, Theme.Space.xxs)
    }

    /// Длительность реплики. Вычитание идёт с проверкой: `UInt64` при
    /// перевёрнутых границах ушёл бы в огромное число, а не в минус.
    private var durationMs: UInt64 {
        segment.endMs > segment.startMs ? segment.endMs - segment.startMs : 0
    }

    private func toggle() {
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
}

/// Строка участника: имя, дорожка, доля речи, число реплик.
private struct SpeakerStatRow: View {
    let row: SpeakerRowModel
    /// Слепок этого участника, если он сложен.
    let printFor: () -> FfiVoicePrint?
    /// Можно ли запомнить его голос между встречами.
    let canRemember: Bool
    let onRemember: () -> Void
    let onRename: (String) -> Void
    let onDelete: () -> Void

    @State private var displayName: String
    @FocusState private var isFieldFocused: Bool
    /// Курсор над полем имени: показываем рамку.
    @State private var isHovered = false

    init(
        row: SpeakerRowModel,
        printFor: @escaping () -> FfiVoicePrint?,
        canRemember: Bool,
        onRemember: @escaping () -> Void,
        onRename: @escaping (String) -> Void,
        onDelete: @escaping () -> Void
    ) {
        self.row = row
        self.printFor = printFor
        self.canRemember = canRemember
        self.onRemember = onRemember
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
                    // Поле имени выглядит подписью, и что его правят,
                    // ниоткуда не следует. Рамка по наведению говорит об
                    // этом до нажатия, а не после.
                    .padding(.horizontal, Theme.Space.xxs)
                    .overlay(
                        RoundedRectangle(cornerRadius: Theme.Radius.sm)
                            .stroke(
                                isFieldFocused ? Theme.accent : Theme.textTertiary,
                                lineWidth: 1
                            )
                            .opacity(isFieldFocused || isHovered ? 1 : 0)
                    )
                    .onHover { isHovered = $0 }
                    .help("Participant name — click to edit")
                    .focused($isFieldFocused)
                    // Enter сохраняет, уход фокуса — тоже, как и при
                    // правке реплики. Без второго набранное имя не
                    // сохранялось вовсе: переключение вкладки уносило его
                    // молча, и возврат показывал прежнее «Спикер 3».
                    .onSubmit(commit)
                    .onChange(of: isFieldFocused) { _, focused in
                        if !focused {
                            commit()
                        }
                    }
                    // Вкладку могут переключить, не трогая фокус: строка
                    // тогда исчезает без onChange, и сохранить надо здесь.
                    .onDisappear(perform: commit)
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
                    // Из чего сложен слепок — не украшение: слепок на
                    // четырёх секундах и слепок на четырёх минутах
                    // подписывают по-разному, и знать это надо до того,
                    // как поверить результату.
                    if let print = printFor() {
                        Chip(text: SpeakerFormat.voicePrintText(print))
                    }
                }
            }

            Spacer(minLength: Theme.Space.sm)

            // Кнопка появляется, только когда запоминать есть что и
            // память включена: показывать её иначе значило бы предлагать
            // действие, которое откажет.
            if canRemember {
                Button("Remember", systemImage: "person.crop.circle.badge.checkmark") {
                    onRemember()
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help("Remember this voice: the app will recognise this person in later meetings")
            }

            Button("Delete", systemImage: "trash", role: .destructive) {
                onDelete()
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .help("Delete the participant and unassign every line")
        }
        .padding(.vertical, Theme.Space.xxs)
        .onChange(of: row.displayName) { _, newValue in
            displayName = newValue
        }
    }

    /// Сохранить набранное, если есть что сохранять.
    ///
    /// Пустое поле возвращается к прежнему имени: строка без подписи при
    /// живом участнике выглядела бы как сбой атрибуции.
    private func commit() {
        guard let name = row.nameToCommit(draft: displayName) else {
            displayName = row.displayName
            return
        }
        onRename(name)
    }

    private var avatar: some View {
        Text(SpeakerFormat.initial(row.label))
            .font(Theme.Text.body.weight(.semibold))
            .foregroundStyle(Theme.textPrimary)
            .frame(width: 28, height: 28)
            .background(Circle().fill(Theme.surfaceOverlay))
    }
}
