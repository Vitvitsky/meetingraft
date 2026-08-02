import SwiftUI

/// Окно настроек: язык сессии + статус STT-модели.
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
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
        .frame(width: 480, height: 260)
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
