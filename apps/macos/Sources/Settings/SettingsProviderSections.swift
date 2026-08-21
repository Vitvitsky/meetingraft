import SwiftUI

// MARK: - Распознавание

struct SttSettingsSection: View {
    @Bindable var model: SettingsModel
    @Environment(ProviderSettingsStore.self) private var providerStore

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Live model"),
                caption: String(localized: "Runs during the meeting; must keep up with speech.")
            ) {
                Picker("", selection: Bindable(providerStore).selectedSttModelId) {
                    ForEach(providerStore.sttModelIds) { modelId in
                        Text(modelId.displayName).tag(modelId)
                    }
                }
                .labelsHidden()
                .frame(width: 200)
            }

            engineStatus
            downloadRow

            Divider().overlay(Theme.borderSubtle)

            SettingsRow(
                title: String(localized: "Post-call model"),
                caption: String(localized: "Runs after the meeting with no latency budget, so it can be larger and slower. Downloaded on first rebuild.")
            ) {
                Text(WhisperModelId.largeV3Turbo.displayName)
                    .font(Theme.Text.body)
                    .foregroundStyle(Theme.textSecondary)
            }

            SettingsRow(
                title: String(localized: "Post-call source"),
                caption: String(localized: "Rebuild re-transcribes the stored audio instead of reusing live captions.")
            ) {
                Picker("", selection: Bindable(providerStore).postCallStt) {
                    ForEach(providerStore.postCallEngines) { engine in
                        Text(engine.pickerLabel).tag(engine)
                    }
                }
                .labelsHidden()
                .frame(width: 200)
            }
        }
    }

    private var engineStatus: some View {
        SettingsRow(
            title: String(localized: "Engine"),
            caption: model.isModelReady ? model.modelPath : String(localized: "Without a model the app emits placeholders instead of speech.")
        ) {
            StatusBadge(
                text: model.liveEngineLabel,
                kind: model.isModelReady ? .success : .warning
            )
        }
    }

    @ViewBuilder
    private var downloadRow: some View {
        let selected = providerStore.selectedSttModelId
        if selected != .auto, !model.isInstalled(selected) {
            HStack(spacing: Theme.Space.sm) {
                Button(model.isDownloading ? String(localized: "Downloading…") : String(localized: "Download")) {
                    model.download(selected, providerStore: providerStore)
                }
                .buttonStyle(.themedPrimary)
                .disabled(model.isDownloading)

                if let progress = model.downloadProgress {
                    ProgressView(value: progress).frame(width: 140)
                    Text("\(Int(progress * 100))%")
                        .font(Theme.Text.mono())
                        .foregroundStyle(Theme.textSecondary)
                }
                if let size = selected.approximateSizeMB {
                    Text("~\(size) MB")
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textTertiary)
                }
                Spacer()
            }
        }
        if !model.downloadError.isEmpty {
            Text(model.downloadError)
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.error)
                .textSelection(.enabled)
        }
    }
}

// MARK: - Перевод

struct TranslationSettingsSection: View {
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(SessionLanguageStore.self) private var languageStore

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(title: String(localized: "Live translation")) {
                Toggle("", isOn: Bindable(translationStore).enabled).labelsHidden()
            }

            if translationStore.enabled {
                SettingsRow(title: String(localized: "Target language")) {
                    Picker("", selection: Bindable(translationStore).target) {
                        ForEach(SpeechLanguage.allCases.filter { $0 != languageStore.primary }) { language in
                            Text(language.displayName).tag(language)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 160)
                }

                SettingsRow(
                    title: String(localized: "Engine"),
                    caption: String(localized: "Auto prefers the system translator when it is available.")
                ) {
                    Picker("", selection: Bindable(translationStore).backend) {
                        ForEach(translationStore.backends) { kind in
                            Text(kind.displayName).tag(kind)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 180)
                }

                if translationStore.backend == .backend || translationStore.backend == .auto {
                    SettingsRow(
                        title: String(localized: "Service URL"),
                        caption: String(localized: "Empty falls back to the built-in stub.")
                    ) {
                        TextField("", text: Bindable(translationStore).backendBaseUrl)
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 240)
                    }
                }
            }
        }
    }
}

// MARK: - AI-провайдеры

struct LlmSettingsSection: View {
    @Bindable var model: SettingsModel
    @Environment(ProviderSettingsStore.self) private var providerStore

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Engine"),
                caption: String(localized: "Used for briefs, follow-ups and transcript polish.")
            ) {
                Picker("", selection: Bindable(providerStore).llmEngine) {
                    ForEach(providerStore.llmEngines) { engine in
                        Text(engine.pickerLabel).tag(engine)
                    }
                }
                .labelsHidden()
                .frame(width: 220)
            }

            if providerStore.llmEngine.needsModel {
                SettingsRow(title: String(localized: "Model")) {
                    TextField("", text: Bindable(providerStore).llmModelId)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 240)
                }
            }

            if providerStore.llmEngine.needsUrl {
                SettingsRow(
                    title: String(localized: "Service URL"),
                    caption: String(localized: "Ollama: /api/chat · OpenAI-compatible: /v1/chat/completions")
                ) {
                    TextField("", text: Bindable(providerStore).llmBaseUrl)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 240)
                }
            }

            if providerStore.llmEngine.needsBackendModelPicker {
                backendCatalog
            }
        }
    }

    @ViewBuilder
    private var backendCatalog: some View {
        if providerStore.backendLlmModels.isEmpty {
            Text(providerStore.backendLlmModelsMessage.isEmpty
                ? String(localized: "No models offered by the backend yet.")
                : providerStore.backendLlmModelsMessage)
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textTertiary)
        } else {
            SettingsRow(title: String(localized: "Model")) {
                Picker("", selection: Bindable(providerStore).selectedBackendLlmId) {
                    ForEach(providerStore.backendLlmSelections) { selection in
                        Text(selection.pickerLabel).tag(selection.id)
                    }
                }
                .labelsHidden()
                .frame(width: 240)
            }
        }
        Button(
            model.isRefreshingBackendModels
                ? String(localized: "Refreshing…")
                : String(localized: "Refresh catalog")
        ) {
            Task { await model.refreshBackendLlmModels(providerStore) }
        }
        .buttonStyle(.themedSecondary)
        .disabled(model.isRefreshingBackendModels)
    }
}

// MARK: - Backend

struct BackendSettingsSection: View {
    @Bindable var model: SettingsModel
    @Environment(ProviderSettingsStore.self) private var providerStore

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Base URL"),
                caption: String(localized: "Optional. Only post-call jobs use it; live captions never leave the device.")
            ) {
                TextField("", text: Bindable(providerStore).apiBaseUrl)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 240)
            }

            SettingsRow(title: String(localized: "Token")) {
                SecureField("", text: Bindable(providerStore).apiToken)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 240)
            }

            HStack(spacing: Theme.Space.sm) {
                Button(
                    model.isTestingConnection
                        ? String(localized: "Testing…")
                        : String(localized: "Test connection")
                ) {
                    Task { await model.testApiConnection(providerStore) }
                }
                .buttonStyle(.themedSecondary)
                .disabled(model.isTestingConnection)

                if let ok = providerStore.apiConnectionOk {
                    StatusBadge(
                        text: ok ? String(localized: "OK") : String(localized: "Failed"),
                        kind: ok ? .success : .failure
                    )
                }
                Spacer()
            }

            if !providerStore.apiConnectionMessage.isEmpty {
                Text(providerStore.apiConnectionMessage)
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(
                        providerStore.apiConnectionOk == true ? Theme.textTertiary : Theme.error
                    )
                    .textSelection(.enabled)
            }
        }
    }
}

// MARK: - Данные

/// Чистка старых записей (Epic 22).
///
/// Транскрипт нужен всегда, запись полугодовой давности — почти никогда.
/// Автоматики здесь нет и не будет: молча терять то, что человек может
/// считать своим, нельзя. Предпросмотр — отдельная кнопка, и он только
/// считает.
struct AudioRetentionSection: View {
    @Bindable var model: SettingsModel
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Delete audio older than"),
                caption: String(
                    localized: "Transcripts, edits and artifacts stay. The audio cannot be restored."
                )
            ) {
                HStack(spacing: Theme.Space.xs) {
                    Picker("", selection: $model.audioSweepMonths) {
                        ForEach([3, 6, 12, 24], id: \.self) { months in
                            Text("\(months)").tag(months)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 70)
                    Text(String(localized: "months"))
                        .font(Theme.Text.caption)
                        .foregroundStyle(.secondary)
                    Button(String(localized: "Preview")) { model.previewAudioSweep() }
                        .buttonStyle(.themedSecondary)
                }
            }

            if model.audioSweepPreviewed {
                Divider().overlay(Theme.borderSubtle)
                preview
            }

            if !model.audioSweepReport.isEmpty {
                Text(model.audioSweepReport)
                    .font(Theme.Text.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .confirmationDialog(
            String(localized: "Delete these recordings?"),
            isPresented: $confirming,
            titleVisibility: .visible
        ) {
            Button(String(localized: "Delete"), role: .destructive) { model.runAudioSweep() }
            Button(String(localized: "Cancel"), role: .cancel) {}
        } message: {
            Text(
                String(
                    localized:
                    "The audio cannot be restored. Transcripts, edits and artifacts are untouched."
                )
            )
        }
    }

    @ViewBuilder
    private var preview: some View {
        if model.audioSweepPreview.isEmpty {
            Text(String(localized: "Nothing to delete."))
                .font(Theme.Text.caption)
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                ForEach(model.audioSweepPreview, id: \.meetingId) { entry in
                    HStack {
                        Text(entry.title.isEmpty ? String(localized: "Untitled") : entry.title)
                            .font(Theme.Text.caption)
                        Spacer()
                        Text(Self.size(entry.bytes))
                            .font(Theme.Text.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                HStack {
                    Text(
                        String(
                            localized: "\(model.audioSweepPreview.count) recordings, "
                        ) + Self.size(model.audioSweepTotalBytes)
                    )
                    .font(Theme.Text.caption)
                    Spacer()
                    Button(String(localized: "Delete")) { confirming = true }
                        .buttonStyle(.link)
                }
            }
        }
    }

    private static func size(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }
}

struct DataSettingsSection: View {
    @Bindable var model: SettingsModel
    @Environment(ProviderSettingsStore.self) private var providerStore

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Export folder"),
                caption: String(localized: "Markdown for finals, briefs and follow-ups.")
            ) {
                HStack(spacing: Theme.Space.xs) {
                    TextField("", text: Bindable(providerStore).exportFolderPath)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 200)
                    Button(String(localized: "Choose…")) {
                        if let url = DirectoryPicker.chooseDirectory(prompt: "Choose") {
                            providerStore.exportFolderPath = url.path
                        }
                    }
                    .buttonStyle(.themedSecondary)
                }
            }

            Divider().overlay(Theme.borderSubtle)

            pathRow(title: String(localized: "Application data"), path: model.dataRoot)
            pathRow(title: String(localized: "Speech models"), path: model.modelsDirectory)

            if !model.localModels.isEmpty {
                SettingsRow(title: String(localized: "Installed models")) {
                    Text(model.localModels.joined(separator: ", "))
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textSecondary)
                }
            }

            Text("Audio, captions and transcripts stay on this Mac. Deleting a meeting removes all of it.")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)

            Divider().overlay(Theme.borderSubtle)

            // Без движка голосов запоминать нечего и нечем: раздела не
            // существует, а не показывается пустым.
            if model.voiceEngineAvailable {
                VoiceMemorySection(model: model)
            }
        }
    }

    private func pathRow(title: String, path: String) -> some View {
        SettingsRow(title: title) {
            Text(path)
                .font(Theme.Text.mono(size: 11))
                .foregroundStyle(Theme.textSecondary)
                .textSelection(.enabled)
                .lineLimit(2)
                .frame(maxWidth: 280, alignment: .trailing)
        }
    }
}

/// Память на голоса: кого приложение узнаёт между встречами (ADR-013).
///
/// Живёт в разделе «Данные», а не рядом со спикерами, и это не вопрос
/// вёрстки. Слепок между встречами — единственное в программе, что
/// переживает удаление записи, из которой посчитано; человек ищет такое
/// там, где написано, что хранится на диске.
///
/// Текст прямой: «по голосу узнаём». Мягкая формулировка вроде
/// «улучшение распознавания участников» скрыла бы ровно то, ради чего
/// признак и выключен по умолчанию.
struct VoiceMemorySection: View {
    @Bindable var model: SettingsModel

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Remember voices between meetings"),
                // Одним литералом, а не склейкой: `String(localized:)`
                // принимает `String.LocalizationValue`, и та строится
                // только из литерала. Склеенное `+` — уже `String`, и
                // компилятор отказывается молча его принять.
                caption: String(localized: "The app stores a voiceprint on this Mac for everyone you name and recognises them in later recordings. The print stays even if the recording is deleted.")
            ) {
                Toggle(
                    "",
                    isOn: Binding(
                        get: { model.voiceMemoryEnabled },
                        set: { model.setVoiceMemory(enabled: $0) }
                    )
                )
                .labelsHidden()
            }

            if model.voiceMemoryEnabled {
                Text("Turning this off forgets everyone: the voiceprints go with it.")
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if !model.knownVoices.isEmpty {
                ForEach(model.knownVoices, id: \.id) { voice in
                    knownVoiceRow(voice)
                }
            } else if model.voiceMemoryEnabled {
                // Пусто — это состояние, а не ошибка: голоса запоминаются
                // по одному, кнопкой на вкладке Speakers.
                Text("Nobody is remembered yet. A voice is remembered from the button next to a participant.")
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if !model.voiceMemoryError.isEmpty {
                Text(model.voiceMemoryError)
                    .font(Theme.Text.bodySmall)
                    .foregroundStyle(Theme.error)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func knownVoiceRow(_ voice: FfiKnownVoice) -> some View {
        HStack(spacing: Theme.Space.sm) {
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(voice.displayName)
                    .font(Theme.Text.body)
                    .foregroundStyle(Theme.textPrimary)
                HStack(spacing: Theme.Space.xs) {
                    Text(SpeakerFormat.knownVoiceText(voice))
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textTertiary)
                    if !voice.modelMatches {
                        // Не поломка: сравнивать с векторами другой модели
                        // нельзя, и голос просто не участвует.
                        Chip(text: "другая модель", tint: Theme.warning)
                    }
                }
            }

            Spacer(minLength: Theme.Space.sm)

            Button(String(localized: "Forget"), role: .destructive) {
                model.forgetVoice(id: voice.id)
            }
            .buttonStyle(.themedSecondary)
            .help("Delete this person's voiceprint")
        }
        .padding(.vertical, Theme.Space.xxs)
    }
}
