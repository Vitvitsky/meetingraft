import Foundation
import Observation

/// Post-call STT / LLM providers до UniFFI ProviderConfig (design 2026-08-02).
@Observable
final class ProviderSettingsStore {
    var postCallStt: PostCallSttEngine = .localFinal {
        didSet {
            if !postCallStt.isAvailable {
                postCallStt = .localFinal
            }
        }
    }

    var llmEngine: LlmEngine = .builtinTemplates {
        didSet {
            if !llmEngine.isAvailable {
                llmEngine = .builtinTemplates
            }
        }
    }

    /// Общий backend (ADR-007 jobs).
    var apiBaseUrl: String = "http://127.0.0.1:8080"
    /// Bearer token; не коммитить секреты — default только для local docker.
    var apiToken: String = "dev-token"
    var apiConnectionMessage: String = ""
    var apiConnectionOk: Bool?
    /// Base URL локального Ollama или OpenAI-compatible сервера.
    var llmBaseUrl: String = "http://127.0.0.1:11434"
    var llmModelId: String = "gemma2"

    let postCallEngines = PostCallSttEngine.allCases
    let llmEngines = LlmEngine.allCases

    /// Подпись для баннера Artifacts.
    var artifactsPipelineCaption: String {
        switch llmEngine {
        case .builtinTemplates:
            "Генерация из Final · сейчас: builtin templates (без LLM)"
        case .ollama:
            "Генерация из Final · LLM: Ollama (\(llmModelId))"
        case .openaiCompat:
            "Генерация из Final · LLM: OpenAI-compat (\(llmModelId))"
        case .backend:
            "Генерация из Final · LLM: backend"
        }
    }
}

enum PostCallSttEngine: String, CaseIterable, Identifiable, Hashable, Sendable {
    case localFinal = "local_final"
    case backendWhisperX = "backend_whisperx"

    var id: String {
        rawValue
    }

    var isAvailable: Bool {
        switch self {
        case .localFinal: true
        case .backendWhisperX: false
        }
    }

    var displayName: String {
        switch self {
        case .localFinal: "Local final (stitch Live)"
        case .backendWhisperX: "Backend WhisperX"
        }
    }

    var pickerLabel: String {
        isAvailable ? displayName : "\(displayName) — скоро"
    }
}

enum LlmEngine: String, CaseIterable, Identifiable, Hashable, Sendable {
    case builtinTemplates = "builtin_templates"
    case ollama
    case openaiCompat = "openai_compat"
    case backend

    var id: String {
        rawValue
    }

    var isAvailable: Bool {
        switch self {
        case .builtinTemplates, .ollama, .openaiCompat, .backend: true
        }
    }

    var displayName: String {
        switch self {
        case .builtinTemplates: "Builtin templates"
        case .ollama: "Ollama"
        case .openaiCompat: "OpenAI-compatible"
        case .backend: "Backend LLM"
        }
    }

    var pickerLabel: String {
        isAvailable ? displayName : "\(displayName) — скоро"
    }

    var needsModel: Bool {
        switch self {
        case .builtinTemplates: false
        case .ollama, .openaiCompat, .backend: true
        }
    }

    var needsUrl: Bool {
        switch self {
        case .builtinTemplates, .backend: false
        case .ollama, .openaiCompat: true
        }
    }
}
