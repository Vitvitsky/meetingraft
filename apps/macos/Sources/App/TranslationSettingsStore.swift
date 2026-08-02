import Foundation
import Observation

/// Выбор translation backend (ADR-008); primary язык — в SessionLanguageStore.
@Observable
final class TranslationSettingsStore {
    var enabled = false
    var target: SpeechLanguage = .en
    /// `auto` | `stub` | `apple` | `backend` | `local_llm` | `off`
    var backend: TranslationBackendKind = .auto
    var backendBaseUrl: String = ""

    let backends: [TranslationBackendKind] = [
        .auto, .apple, .backend, .localLlm, .stub, .off,
    ]
}

enum TranslationBackendKind: String, CaseIterable, Identifiable, Hashable, Sendable {
    case auto
    case apple
    case backend
    case localLlm = "local_llm"
    case stub
    case off

    var id: String {
        rawValue
    }

    var displayName: String {
        switch self {
        case .auto: "Auto"
        case .apple: "Apple (host)"
        case .backend: "Backend (NLLB)"
        case .localLlm: "Local LLM"
        case .stub: "Stub (demo)"
        case .off: "Off"
        }
    }
}
