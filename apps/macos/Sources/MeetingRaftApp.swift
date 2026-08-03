import SwiftUI

/// Точка входа приложения MeetingRaft.
@main
struct MeetingRaftApp: App {
    @State private var languageStore = SessionLanguageStore()
    @State private var translationStore = TranslationSettingsStore()
    @State private var providerStore = ProviderSettingsStore()
    @State private var presenceStore = PresenceSettingsStore()
    @State private var detector = MeetingDetector()
    /// Общее состояние записи для строки меню: сама запись живёт в
    /// координаторе внутри окна, сюда попадает только флаг.
    @State private var recordingBridge = RecordingBridge()

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environment(languageStore)
                .environment(translationStore)
                .environment(providerStore)
                .environment(presenceStore)
                .environment(recordingBridge)
                .task {
                    detector.start()
                }
        }
        .commands {
            SessionCommands()
        }

        MenuBarExtra {
            MenuBarView(
                isRecording: recordingBridge.isRecording,
                detectedApp: detector.detected,
                onToggleRecording: { recordingBridge.toggle?() },
                onOpenWindow: { recordingBridge.openWindow?() }
            )
        } label: {
            Image(
                systemName: MenuBarIcon.systemImage(
                    isRecording: recordingBridge.isRecording,
                    hasDetectedMeeting: detector.detected != nil
                )
            )
        }

        Settings {
            SettingsView()
                .environment(languageStore)
                .environment(translationStore)
                .environment(providerStore)
                .environment(presenceStore)
        }
    }
}

/// Команды меню Session.
struct SessionCommands: Commands {
    @FocusedValue(\.startCaptions) private var startCaptions

    var body: some Commands {
        CommandMenu("Session") {
            Button("Start Demo Captions") {
                startCaptions?()
            }
            .keyboardShortcut("r", modifiers: [.command])
        }
    }
}
