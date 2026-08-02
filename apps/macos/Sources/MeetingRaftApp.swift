import SwiftUI

/// Точка входа приложения MeetingRaft.
@main
struct MeetingRaftApp: App {
    @State private var languageStore = SessionLanguageStore()

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environment(languageStore)
        }
        .commands {
            SessionCommands()
        }

        Settings {
            SettingsView()
                .environment(languageStore)
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
