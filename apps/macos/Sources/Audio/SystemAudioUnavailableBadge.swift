import AppKit
import SwiftUI

/// Состояние «пишется только микрофон» как actionable-плашка.
///
/// Отдельного pre-flight запроса у разрешения на системный звук нет:
/// TCC-промпт появляется при первой попытке создать tap, поэтому отказ —
/// нормальное состояние приложения, а не ошибка.
struct SystemAudioUnavailableBadge: View {
    let status: SystemAudioStatus

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "person.wave.2")
            Text("Mic only")
            if status == .denied {
                Button("Enable system audio") {
                    SystemAudioSettingsLink.open()
                }
                .buttonStyle(.link)
            }
        }
        .font(.caption)
        .foregroundStyle(.orange)
        .help(explanation)
    }

    private var explanation: String {
        switch status {
        case .denied:
            String(localized: "Only your voice is recorded. Allow System Audio Recording to capture the other participants.")
        case .unsupported:
            String(localized: "This Mac cannot capture system audio; only your voice is recorded.")
        case .noOutputDevice:
            String(localized: "No output device to capture from; only your voice is recorded.")
        case .aggregateFailed:
            String(localized: "System audio device could not be created; only your voice is recorded.")
        case .granted, .unknown:
            String(localized: "Only your voice is recorded.")
        }
    }
}

/// Переход в системные настройки приватности.
enum SystemAudioSettingsLink {
    /// Идентификатор панели менялся между релизами macOS: при промахе
    /// открываем корень настроек приватности, а не молчим.
    static let url = URL(
        string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
    )

    /// Панель микрофона — отдельная от системного звука.
    static let microphoneUrl = URL(
        string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
    )

    static let fallbackUrl = URL(string: "x-apple.systempreferences:com.apple.preference.security")

    static func open() {
        open(url)
    }

    static func openMicrophone() {
        open(microphoneUrl)
    }

    private static func open(_ pane: URL?) {
        guard let target = pane ?? fallbackUrl else { return }
        if !NSWorkspace.shared.open(target), let fallbackUrl {
            NSWorkspace.shared.open(fallbackUrl)
        }
    }
}
