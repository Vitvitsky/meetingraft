import SwiftUI

/// Точка входа приложения MeetingRaft.
@main
struct MeetingRaftApp: App {
    @State private var languageStore = SessionLanguageStore()
    @State private var translationStore = TranslationSettingsStore()
    @State private var providerStore = ProviderSettingsStore()
    @State private var presenceStore = PresenceSettingsStore()

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environment(languageStore)
                .environment(translationStore)
                .environment(providerStore)
                .environment(presenceStore)
        }
        .commands {
            SessionCommands()
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
