import SwiftUI

/// Окно настроек: язык сессии, translation backend (ADR-008), статус STT.
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @Environment(TranslationSettingsStore.self) private var translationStore
    @State private var modelPath: String = ""
    @State private var modelsDir: String = ""

    var body: some View {
        Form {
            Picker("Session language", selection: Bindable(languageStore).primary) {
                ForEach(languageStore.allowed) { language in
                    Text(language.displayName).tag(language)
                }
            }
            Text("Default is Russian (ADR-003).")
                .foregroundStyle(.secondary)

            Section("Live translation (ADR-008)") {
                Toggle("Enable", isOn: Bindable(translationStore).enabled)
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
                    TextField("Backend base URL", text: Bindable(translationStore).backendBaseUrl)
                        .textFieldStyle(.roundedBorder)
                    Text("Skeleton: POST {base}/v1/translate (NLLB later).")
                        .foregroundStyle(.secondary)
                        .font(.caption)
                }
                Text("Apple uses host bridge (no Cocoa in Rust). Auto prefers Apple when available.")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }

            Section("Live STT (ADR-005)") {
                if modelPath.isEmpty {
                    Text("Whisper model: not found — Mock engine")
                        .foregroundStyle(.orange)
                } else {
                    Text("Whisper model: \(modelPath)")
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                Text("Models dir: \(modelsDir)")
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                Text("Download: apps/macos/Scripts/download-stt-model.sh")
                    .foregroundStyle(.secondary)
            }

            Text("Audio: mic capture active; system tap wiring follow-up (ADR-004). Chunks 100 ms @ 16 kHz.")
                .foregroundStyle(.secondary)
            Text("Glossary is available in the sidebar.")
                .foregroundStyle(.secondary)
        }
        .padding()
        .frame(width: 520, height: 420)
        .onAppear(perform: refreshModelStatus)
    }

    private func refreshModelStatus() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let root = support.appendingPathComponent("meetingraft", isDirectory: true)
        let core = MeetingCore.withDataRoot(dataRoot: root.path)
        modelPath = core.whisperModelPath()
        modelsDir = core.modelsDirectory()
    }
}
