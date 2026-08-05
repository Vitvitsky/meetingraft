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
