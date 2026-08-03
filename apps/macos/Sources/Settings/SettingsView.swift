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
    @State private var core: MeetingCore?

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
                Text("Models dir: \(modelsDir)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                Text("Download: apps/macos/Scripts/download-stt-model.sh (Hugging Face ggml)")
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
        .onChange(of: providerStore.apiBaseUrl) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.apiToken) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.llmEngine) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.llmModelId) { _, _ in
            applyApiConfig()
        }
        .onChange(of: providerStore.llmBaseUrl) { _, _ in
            applyApiConfig()
        }
    }

    private var liveSttEngineLabel: String {
        modelPath.isEmpty ? "Mock (no ggml model)" : "Whisper (on-device)"
    }

    private func refreshModelStatus() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let root = support.appendingPathComponent("meetingraft", isDirectory: true)
        dataRoot = root.path
        let meetingCore = MeetingCore.withDataRoot(dataRoot: root.path)
        core = meetingCore
        modelPath = meetingCore.whisperModelPath()
        modelsDir = meetingCore.modelsDirectory()
        applyApiConfig()
    }

    /// Локальный core нужен для проверки API; Generate применяет эти настройки к shell core.
    private func applyApiConfig() {
        core?.setApiConfig(baseUrl: providerStore.apiBaseUrl, token: providerStore.apiToken)
        core?.setLlmConfig(
            engineCode: providerStore.llmEngine.rawValue,
            modelId: providerStore.llmModelId,
            baseUrl: providerStore.llmBaseUrl
        )
    }

    private func chooseExportFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Choose"
        if panel.runModal() == .OK, let url = panel.url {
            providerStore.exportFolderPath = url.path
        }
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
