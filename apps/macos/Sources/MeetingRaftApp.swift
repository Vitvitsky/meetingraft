import SwiftUI

/// Точка входа приложения MeetingRaft.
@main
struct MeetingRaftApp: App {
    @State private var languageStore = SessionLanguageStore()
    @State private var translationStore = TranslationSettingsStore()

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environment(languageStore)
                .environment(translationStore)
        }
        .commands {
            SessionCommands()
        }

        Settings {
            SettingsView()
                .environment(languageStore)
                .environment(translationStore)
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
