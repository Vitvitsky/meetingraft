import SwiftUI

/// Экран живых субтитров (ТЗ редизайна §4.1).
///
/// Порядок сверху вниз: шапка → субтитры по центру → управление →
/// индикатор входа → строка состояния.
///
/// Главное отличие от прежней версии: содержимое экрана — речь, а не
/// журнал событий. Счётчики чанков и caption-событий были дебажной
/// телеметрией на первом плане; полный лог живёт в истории встречи.
struct LiveCaptionsView: View {
    @Bindable var viewModel: LiveCaptionsViewModel
    @Bindable var capture: AudioCaptureCoordinator
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(ProviderSettingsStore.self) private var providerStore
    let primaryLanguage: SpeechLanguage

    private var isLive: Bool {
        capture.isRecording || viewModel.isLiveSession
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider().overlay(Theme.borderSubtle)
            captionStage
            Divider().overlay(Theme.borderSubtle)
            // Нижняя часть неприкосновенна: у неё приоритет раскладки,
            // поэтому длинная реплика ужимает ленту субтитров, а не
            // выдавливает управление за край окна.
            VStack(spacing: 0) {
                controls
                levelMeter
                statusBar
            }
            .layoutPriority(1)
        }
        .background(Theme.surfaceRoot)
        .navigationTitle("Live Captions")
        .onAppear {
            viewModel.applyTranslationSettings(translationStore)
        }
        .onDisappear {
            viewModel.stopAll(capture: capture)
        }
        .onChange(of: translationStore.enabled) { _, _ in
            viewModel.applyTranslationSettings(translationStore)
        }
        .onChange(of: translationStore.target) { _, _ in
            viewModel.applyTranslationSettings(translationStore)
        }
    }

    // MARK: - Шапка

    private var header: some View {
        HStack(spacing: Theme.Space.sm) {
            if isLive {
                StatusBadge(text: "REC", kind: .recording, showsDot: true)
            }
            Chip(text: primaryLanguage.rawValue.uppercased(), isSelected: true)
            TimelineView(.periodic(from: .now, by: 1)) { _ in
                Text(elapsedText)
                    .font(Theme.Text.mono(size: 13))
                    .foregroundStyle(Theme.textSecondary)
            }
            Spacer()
            if !capture.systemAudioAvailable, isLive {
                SystemAudioUnavailableBadge(status: capture.systemAudioStatus)
            }
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.vertical, Theme.Space.sm)
        .background(Theme.surface)
    }

    /// Длительность сессии; вне записи — прочерк, а не ноль: ноль
    /// читается как «идёт, но ничего не пишется».
    private var elapsedText: String {
        guard let started = viewModel.sessionStartedAt else { return "—:—" }
        let seconds = max(0, Int(Date().timeIntervalSince(started)))
        return String(format: "%02d:%02d", seconds / 60, seconds % 60)
    }

    // MARK: - Субтитры

    private var captionStage: some View {
        HStack(spacing: 0) {
            stage(lines: viewModel.recentLines(), placeholder: placeholderText)
            if translationStore.enabled {
                Divider().overlay(Theme.borderSubtle)
                stage(
                    lines: Array(viewModel.translationLines.suffix(3)),
                    placeholder: String(localized: "Translation appears here")
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// Лента прокручивается внутри себя и прижата к низу: свежая реплика
    /// всегда видна, а высота блока не зависит от длины текста.
    private func stage(lines: [CaptionLine], placeholder: String) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Space.sm) {
                if lines.isEmpty {
                    Text(placeholder)
                        .font(Theme.Text.bodyLarge)
                        .foregroundStyle(Theme.textTertiary)
                } else {
                    ForEach(lines) { line in
                        captionLine(line)
                    }
                }
            }
            .frame(maxWidth: 740, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(Theme.Space.lg)
        }
        .defaultScrollAnchor(.bottom)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func captionLine(_ line: CaptionLine) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            // Подпись говорящего осмысленна только когда пишутся оба
            // канала; в монологе она лишний шум.
            if capture.systemAudioAvailable {
                Text(line.speaker.label)
                    .font(Theme.Text.caption.weight(.semibold))
                    .foregroundStyle(line.speaker == .you ? Theme.accent : Theme.info)
            }
            Text(line.text)
                .font(Theme.Text.large)
                .foregroundStyle(line.phase == .partial ? Theme.textSecondary : Theme.textPrimary)
                // Без предела строк одна длинная реплика требовала всю
                // высоту окна и выдавливала кнопки.
                .lineLimit(4)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            capture.systemAudioAvailable
                ? "\(line.speaker.label): \(line.text)"
                : line.text
        )
    }

    private var placeholderText: String {
        isLive
            ? String(localized: "Listening…")
            : String(localized: "Press Start to capture the meeting")
    }

    // MARK: - Управление

    private var controls: some View {
        HStack(spacing: Theme.Space.sm) {
            if isLive {
                Button(String(localized: "Stop recording")) {
                    viewModel.stopLive(capture: capture)
                }
                .buttonStyle(.themedDestructive)
            } else {
                Button(String(localized: "Start recording")) {
                    Task {
                        await viewModel.startLive(
                            capture: capture,
                            translation: translationStore,
                            stt: providerStore
                        )
                    }
                }
                .buttonStyle(.themedPrimary)
            }

            Toggle(String(localized: "Translate"), isOn: Bindable(translationStore).enabled)
                .toggleStyle(.switch)
                .font(Theme.Text.body)
                .foregroundStyle(Theme.textSecondary)

            if translationStore.enabled {
                Picker("", selection: Bindable(translationStore).target) {
                    ForEach(SpeechLanguage.allCases.filter { $0 != primaryLanguage }) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                .labelsHidden()
                .frame(width: 140)
            }
            Spacer()

            if let error = capture.lastError {
                Text(error)
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(Theme.error)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.vertical, Theme.Space.sm)
    }

    // MARK: - Индикатор входа

    private var levelMeter: some View {
        HStack(spacing: Theme.Space.sm) {
            Text("Microphone")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textTertiary)
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(Theme.surfaceElevated)
                    Capsule()
                        .fill(capture.inputLevel > 0.9 ? Theme.warning : Theme.accent)
                        .frame(width: geometry.size.width * capture.inputLevel)
                }
            }
            .frame(height: 6)
            .animation(.linear(duration: 0.1), value: capture.inputLevel)
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.bottom, Theme.Space.sm)
        .opacity(isLive ? 1 : 0.35)
    }

    // MARK: - Строка состояния

    private var statusBar: some View {
        HStack {
            Text(engineDescription)
                .font(Theme.Text.mono(size: 11))
                .foregroundStyle(Theme.textTertiary)
            Spacer()
            if translationStore.enabled, !viewModel.effectiveTranslationBackend.isEmpty {
                Text(viewModel.effectiveTranslationBackend)
                    .font(Theme.Text.mono(size: 11))
                    .foregroundStyle(Theme.textTertiary)
            }
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.vertical, Theme.Space.xs)
        .background(Theme.surface)
    }

    /// Латентность здесь намеренно не показывается: в продукте она не
    /// измеряется, а рисовать правдоподобное число — врать.
    private var engineDescription: String {
        switch capture.sttBackend {
        case "whisper": "Whisper · on-device"
        case "mock": String(localized: "Placeholder engine — no model installed")
        default: String(localized: "Idle")
        }
    }
}
