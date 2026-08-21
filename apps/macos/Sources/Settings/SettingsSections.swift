import SwiftUI

/// Разделы настроек (ТЗ редизайна §3.2).
///
/// Один бесконечный `Form` заменён списком: в нём нельзя было найти
/// нужное, а заголовки ссылались на номера ADR — внутренние документы,
/// о существовании которых пользователь не знает.
enum SettingsSection: String, CaseIterable, Identifiable, Hashable {
    case general
    case audio
    case sttEngine
    case translation
    case llm
    case backend
    case data

    var id: String {
        rawValue
    }

    var title: String {
        switch self {
        case .general: String(localized: "General")
        case .audio: String(localized: "Audio")
        case .sttEngine: String(localized: "Speech recognition")
        case .translation: String(localized: "Translation")
        case .llm: String(localized: "AI providers")
        case .backend: String(localized: "Backend")
        case .data: String(localized: "Data & storage")
        }
    }

    var systemImage: String {
        switch self {
        case .general: "gearshape"
        case .audio: "waveform"
        case .sttEngine: "text.bubble"
        case .translation: "character.bubble"
        case .llm: "sparkles"
        case .backend: "server.rack"
        case .data: "internaldrive"
        }
    }
}

// MARK: - General

struct GeneralSettingsSection: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @Environment(PresenceSettingsStore.self) private var presenceStore
    @Environment(AppearanceSettingsStore.self) private var appearanceStore

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            SettingsRow(
                title: String(localized: "Session language"),
                caption: String(localized: "Primary recognition language. Russian by default.")
            ) {
                Picker("", selection: Bindable(languageStore).primary) {
                    ForEach(languageStore.allowed) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
            }

            Divider().overlay(Theme.borderSubtle)

            SettingsRow(
                title: String(localized: "Appearance"),
                caption: String(localized: "System follows the macOS setting.")
            ) {
                Picker("", selection: Bindable(appearanceStore).preference) {
                    ForEach(AppearancePreference.allCases) { option in
                        Text(option.title).tag(option)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
            }

            Divider().overlay(Theme.borderSubtle)

            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Text("Recognized languages")
                    .font(Theme.Text.body)
                HStack(spacing: Theme.Space.xs) {
                    ForEach(SpeechLanguage.allCases) { language in
                        Chip(
                            text: language.rawValue.uppercased(),
                            isSelected: language == languageStore.primary
                        )
                    }
                }
            }

            Divider().overlay(Theme.borderSubtle)

            Text("While recording")
                .font(Theme.Text.title)

            SettingsRow(
                title: String(localized: "Captions over other apps"),
                caption: String(localized: "A floating strip stays visible above Zoom or Meet, including full screen.")
            ) {
                Toggle("", isOn: Bindable(presenceStore).showsOverlay)
                    .labelsHidden()
            }

            SettingsRow(
                title: String(localized: "Hide the main window"),
                caption: String(localized: "Only while the floating strip is on — otherwise nothing would show that recording is running.")
            ) {
                Toggle("", isOn: Bindable(presenceStore).minimizesMainWindow)
                    .labelsHidden()
                    .disabled(!presenceStore.showsOverlay)
            }

            SettingsRow(title: String(localized: "Strip opacity")) {
                Slider(value: Bindable(presenceStore).overlayOpacity, in: 0.2 ... 1)
                    .frame(width: 160)
                    .disabled(!presenceStore.showsOverlay)
            }

            Divider().overlay(Theme.borderSubtle)

            SettingsRow(title: String(localized: "Version")) {
                Text(Self.appVersion)
                    .font(Theme.Text.mono())
                    .foregroundStyle(Theme.textSecondary)
            }
        }
    }

    private static var appVersion: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "—"
        let build = info?["CFBundleVersion"] as? String ?? "—"
        return "\(short) (\(build))"
    }
}

// MARK: - Audio

struct AudioSettingsSection: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.md) {
            Card {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text("Capture sources")
                        .font(Theme.Text.title)
                    sourceRow(
                        title: String(localized: "Microphone"),
                        caption: String(localized: "Your own voice."),
                        kind: .success,
                        status: String(localized: "Active")
                    )
                    sourceRow(
                        title: String(localized: "System audio"),
                        caption: String(localized: "Everyone else in the call, captured without a driver."),
                        kind: .success,
                        status: String(localized: "Active")
                    )
                }
            }

            Text("Both streams stay separate end to end: post-call knows exactly who spoke, live estimates it by loudness.")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)

            SettingsRow(
                title: String(localized: "Chunk"),
                caption: String(localized: "Feeding the recognizer; not user-adjustable.")
            ) {
                Text("100 ms · 16 kHz")
                    .font(Theme.Text.mono())
                    .foregroundStyle(Theme.textSecondary)
            }
        }
    }

    private func sourceRow(
        title: String,
        caption: String,
        kind: StatusKind,
        status: String
    ) -> some View {
        SettingsRow(title: title, caption: caption) {
            StatusBadge(text: status, kind: kind)
        }
    }
}
