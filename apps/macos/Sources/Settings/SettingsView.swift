import SwiftUI

/// Окно настроек: язык сессии (stub).
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore

    var body: some View {
        Form {
            Picker("Session language", selection: Bindable(languageStore).primary) {
                ForEach(languageStore.allowed) { language in
                    Text(language.displayName).tag(language)
                }
            }
            Text("Default is Russian (ADR-003).")
                .foregroundStyle(.secondary)
        }
        .padding()
        .frame(width: 360, height: 140)
    }
}
