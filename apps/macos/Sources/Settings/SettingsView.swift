import SwiftUI

/// Окно настроек: язык + карта Providers (STT / translation / LLM / paths).
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(ProviderSettingsStore.self) private var providerStore
    @State private var modelPath: String = ""
    @State private var modelsDir: String = ""
    @State private var dataRoot: String = ""

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
                Text("local_final: Final = Live finals + glossary. WhisperX jobs — позже (ADR-007).")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if providerStore.postCallStt == .backendWhisperX || !PostCallSttEngine.backendWhisperX.isAvailable {
                    TextField("API base URL", text: Bindable(providerStore).apiBaseUrl)
                        .textFieldStyle(.roundedBorder)
                        .disabled(true)
                    Text("POST {apiBase}/v1/jobs — скоро")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
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
                    Text("POST {base}/v1/translate (NLLB on backend later). Empty → auto falls back to stub.")
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
                Text("Сейчас артефакты всегда из Final через builtin templates.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if providerStore.llmEngine.needsUrl || !LlmEngine.ollama.isAvailable {
                    TextField("LLM base URL", text: Bindable(providerStore).llmBaseUrl)
                        .textFieldStyle(.roundedBorder)
                        .disabled(!providerStore.llmEngine.isAvailable || !providerStore.llmEngine.needsUrl)
                    TextField("Model id", text: Bindable(providerStore).llmModelId)
                        .textFieldStyle(.roundedBorder)
                        .disabled(!providerStore.llmEngine.isAvailable)
                    Text("Ollama: http://127.0.0.1:11434 · OpenAI-compat: {base}/v1 · Backend: {apiBase}/v1 — скоро")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
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
        .frame(minWidth: 560, minHeight: 560)
        .onAppear(perform: refreshModelStatus)
    }

    private var liveSttEngineLabel: String {
        modelPath.isEmpty ? "Mock (no ggml model)" : "Whisper (on-device)"
    }

    private func refreshModelStatus() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let root = support.appendingPathComponent("meetingraft", isDirectory: true)
        dataRoot = root.path
        let core = MeetingCore.withDataRoot(dataRoot: root.path)
        modelPath = core.whisperModelPath()
        modelsDir = core.modelsDirectory()
    }
}
