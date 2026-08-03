import AppKit
import SwiftUI

/// Окно настроек: язык + карта Providers (STT / translation / LLM / backend / paths).
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(ProviderSettingsStore.self) private var providerStore
    @State private var modelPath: String = ""
    @State private var modelsDir: String = ""
    @State private var dataRoot: String = ""
    @State private var localModels: [String] = []
    @State private var downloadProgress: Double?
    @State private var downloadError: String = ""
    @State private var isDownloading = false
    @State private var core: MeetingCore?
    private let modelDownloader: WhisperDownloading = WhisperModelDownloader()

    var body: some View {
        Form {
            Section("Session") {
                Picker("Language", selection: Bindable(languageStore).primary) {
                    ForEach(languageStore.allowed) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                Text("Primary recognition language (ADR-003). Default: Russian.")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }

            Section("Backend API (ADR-007)") {
                TextField("API base URL", text: Bindable(providerStore).apiBaseUrl)
                    .textFieldStyle(.roundedBorder)
                SecureField("Bearer token", text: Bindable(providerStore).apiToken)
                    .textFieldStyle(.roundedBorder)
                Text("docker compose up → http://127.0.0.1:8080 · token default `dev-token`")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("POST /v1/jobs · GET /v1/jobs/{id} · GET /v1/artifacts/{id}")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack {
                    Button("Test API") {
                        testApiConnection()
                    }
                    if let ok = providerStore.apiConnectionOk {
                        Text(ok ? "OK" : "Fail")
                            .foregroundStyle(ok ? Color.green : Color.red)
                    }
                }
                if !providerStore.apiConnectionMessage.isEmpty {
                    Text(providerStore.apiConnectionMessage)
                        .font(.caption)
                        .foregroundStyle(
                            providerStore.apiConnectionOk == true ? Color.secondary : Color.red
                        )
                        .textSelection(.enabled)
                }
            }

            Section("Live STT (ADR-005)") {
                Picker("Model", selection: Bindable(providerStore).selectedSttModelId) {
                    ForEach(providerStore.sttModelIds) { modelId in
                        Text(modelId.displayName).tag(modelId)
                    }
                }
                LabeledContent("Engine") {
                    Text(liveSttEngineLabel)
                        .foregroundStyle(modelPath.isEmpty ? .orange : .primary)
                }
                if modelPath.isEmpty {
                    Text("Status: missing model → Mock")
                        .foregroundStyle(.orange)
                } else {
                    Text("Status: configured")
                        .foregroundStyle(.secondary)
                    Text(modelPath)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                if !localModels.isEmpty {
                    Text("Installed: \(localModels.joined(separator: ", "))")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if providerStore.selectedSttModelId != .auto, !isSelectedModelInstalled {
                    HStack {
                        Button(isDownloading ? "Downloading…" : "Download") {
                            startDownload(providerStore.selectedSttModelId)
                        }
                        .disabled(isDownloading)
                        if let progress = downloadProgress {
                            ProgressView(value: progress)
                                .frame(width: 120)
                        }
                    }
                }
                if !downloadError.isEmpty {
                    Text(downloadError)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                }
                Text("Models dir: \(modelsDir)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                Text("First run downloads ggml-base.bin automatically at app launch.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Post-call STT") {
                Picker("Engine", selection: Bindable(providerStore).postCallStt) {
                    ForEach(providerStore.postCallEngines) { engine in
                        Text(engine.pickerLabel).tag(engine)
                    }
                }
                Text("local_final: Final = Live finals + glossary. WhisperX → Backend API /v1/jobs — скоро.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Translation (ADR-008)") {
                Toggle("Enable live translation", isOn: Bindable(translationStore).enabled)
                Picker("Target", selection: Bindable(translationStore).target) {
                    ForEach(SpeechLanguage.allCases.filter { $0 != languageStore.primary }) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                Picker("Backend", selection: Bindable(translationStore).backend) {
                    ForEach(translationStore.backends) { kind in
                        Text(kind.displayName).tag(kind)
                    }
                }
                if translationStore.backend == .backend || translationStore.backend == .auto {
                    TextField("Translate base URL", text: Bindable(translationStore).backendBaseUrl)
                        .textFieldStyle(.roundedBorder)
                    Text("POST {base}/v1/translate (NLLB later). Empty → auto falls back to stub.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text("Apple: host bridge in Swift. Auto prefers Apple when host is registered.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("LLM (Brief / Follow-up)") {
                Picker("Engine", selection: Bindable(providerStore).llmEngine) {
                    ForEach(providerStore.llmEngines) { engine in
                        Text(engine.pickerLabel).tag(engine)
                    }
                }
                Text("Builtin templates локально; Ollama / OpenAI-compatible — локальный HTTP; Backend — jobs.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if providerStore.llmEngine.needsModel {
                    TextField("Model id", text: Bindable(providerStore).llmModelId)
                        .textFieldStyle(.roundedBorder)
                }
                if providerStore.llmEngine.needsBackendModelPicker {
                    if providerStore.backendLlmModels.isEmpty {
                        Text(
                            providerStore.backendLlmModelsMessage.isEmpty
                                ? "Нет моделей — настройте PROVIDERS_JSON / LLM_* на backend"
                                : providerStore.backendLlmModelsMessage
                        )
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    } else {
                        Picker("Model", selection: Bindable(providerStore).selectedBackendLlmId) {
                            ForEach(providerStore.backendLlmSelections) { selection in
                                Text(selection.pickerLabel).tag(selection.id)
                            }
                        }
                    }
                    Button("Обновить") {
                        refreshBackendLlmModels()
                    }
                }
                if providerStore.llmEngine.needsUrl {
                    TextField("LLM base URL", text: Bindable(providerStore).llmBaseUrl)
                        .textFieldStyle(.roundedBorder)
                    Text("Ollama: /api/chat · OpenAI-compatible: /v1/chat/completions")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Section("Export") {
                TextField("Export folder", text: Bindable(providerStore).exportFolderPath)
                    .textFieldStyle(.roundedBorder)
                HStack {
                    Button("Choose…") {
                        chooseExportFolder()
                    }
                    Spacer()
                }
                Text("Markdown export (Final, Brief, Follow-up) → Obsidian vault or Documents.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Data roots") {
                Text("App Support: \(dataRoot)")
                    .font(.caption)
                    .textSelection(.enabled)
                Text("SQLite / models живут под этим каталогом.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                Text("Audio: mic (+ system if available), 100 ms @ 16 kHz (ADR-004). Glossary — в sidebar.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
        .frame(minWidth: 560, minHeight: 620)
        .onAppear(perform: refreshModelStatus)
        .onChange(of: providerStore.selectedSttModelId) { _, _ in
            applySttPreference()
            refreshModelPaths()
        }
        .onChange(of: providerStore.apiBaseUrl) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.apiToken) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.llmEngine) { _, engine in
            applyApiConfig()
            if engine.needsBackendModelPicker {
                refreshBackendLlmModels()
            }
        }
        .onChange(of: providerStore.llmModelId) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.llmProviderId) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.llmBaseUrl) { _, _ in
            applyApiConfig()
        }
    }

    private var liveSttEngineLabel: String {
        modelPath.isEmpty ? "Mock (no ggml model)" : "Whisper (on-device)"
    }

    private var isSelectedModelInstalled: Bool {
        guard let filename = providerStore.selectedSttModelId.filename else {
            return !localModels.isEmpty
        }
        return localModels.contains(filename)
    }

    private func refreshModelStatus() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let root = support.appendingPathComponent("meetingraft", isDirectory: true)
        dataRoot = root.path
        let meetingCore = MeetingCore.withDataRoot(dataRoot: root.path)
        core = meetingCore
        refreshModelPaths()
        applySttPreference()
        applyApiConfig()
    }

    private func refreshModelPaths() {
        guard let core else { return }
        modelPath = core.whisperModelPath()
        modelsDir = core.modelsDirectory()
        localModels = core.listLocalWhisperModels()
    }

    private func applySttPreference() {
        core?.setPreferredWhisperModel(modelId: providerStore.selectedSttModelId.rawValue)
        refreshModelPaths()
    }

    private func startDownload(_ modelId: WhisperModelId) {
        guard let core else { return }
        let modelsDirectory = URL(fileURLWithPath: core.modelsDirectory(), isDirectory: true)
        isDownloading = true
        downloadError = ""
        downloadProgress = nil
        Task {
            do {
                _ = try await modelDownloader.download(
                    id: modelId,
                    modelsDirectory: modelsDirectory
                ) { fraction in
                    downloadProgress = fraction
                }
                refreshModelPaths()
                applySttPreference()
            } catch {
                downloadError = downloadErrorMessage(for: error)
            }
            isDownloading = false
            downloadProgress = nil
        }
    }

    private func downloadErrorMessage(for error: Error) -> String {
        switch error {
        case WhisperModelDownloaderError.notDownloadable:
            "Модель не скачивается (auto)."
        case let WhisperModelDownloaderError.downloadFailed(statusCode):
            if let statusCode {
                "Ошибка загрузки: HTTP \(statusCode)."
            } else {
                "Ошибка загрузки модели с Hugging Face."
            }
        default:
            "Ошибка загрузки: \(error.localizedDescription)"
        }
    }

    /// Локальный core нужен для проверки API; Generate применяет эти настройки к shell core.
    private func applyApiConfig() {
        core?.setApiConfig(baseUrl: providerStore.apiBaseUrl, token: providerStore.apiToken)
        core?.setLlmConfig(
            engineCode: providerStore.llmEngine.rawValue,
            modelId: providerStore.llmModelId,
            baseUrl: providerStore.llmBaseUrl,
            providerId: providerStore.llmProviderId
        )
    }

    private func refreshBackendLlmModels() {
        applyApiConfig()
        guard let core else { return }
        let models = core.listBackendLlmModels()
        // FFI maps sync Err → []; отличаем сбой от Ok([]) через health probe.
        if models.isEmpty {
            let connectionError = core.testApiConnection()
            if !connectionError.isEmpty {
                providerStore.applyBackendModelsCatalog([], connectionError: connectionError)
                return
            }
        }
        providerStore.applyBackendModelsCatalog(models)
    }

    private func chooseExportFolder() {
        guard let url = DirectoryPicker.chooseDirectory(prompt: "Choose") else { return }
        providerStore.exportFolderPath = url.path
    }

    private func testApiConnection() {
        applyApiConfig()
        guard let core else { return }
        let error = core.testApiConnection()
        if error.isEmpty {
            providerStore.apiConnectionOk = true
            providerStore.apiConnectionMessage = "GET /health OK"
        } else {
            providerStore.apiConnectionOk = false
            providerStore.apiConnectionMessage = error
        }
    }
}
