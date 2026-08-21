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
    /// `provider_id` из backend registry (GET /v1/models); для local LLM — `"default"`.
    var llmProviderId: String = "default"
    /// Каталог моделей с backend; пусто до «Обновить» или при ошибке API.
    var backendLlmModels: [FfiLlmModelRef] = []
    /// Подпись под picker (пустой каталог / ошибка).
    var backendLlmModelsMessage: String = ""
    /// Папка экспорта markdown (Obsidian vault / Documents); tilde раскрывается при записи.
    var exportFolderPath: String = "~/Documents/MeetingRaft"
    /// On-device Whisper ggml id; `auto` — resolve по установленным файлам (ADR-005).
    var selectedSttModelId: WhisperModelId = .auto

    let postCallEngines = PostCallSttEngine.allCases
    let sttModelIds = WhisperModelId.allCases
    let llmEngines = LlmEngine.allCases

    /// Элементы Picker для backend LLM (`provider_id|model`).
    var backendLlmSelections: [BackendLlmSelection] {
        backendLlmModels.map { model in
            BackendLlmSelection(
                providerId: model.providerId,
                model: model.model,
                displayName: model.displayName
            )
        }
    }

    /// Ключ выбора в Picker; при смене обновляет `llmProviderId` + `llmModelId`.
    var selectedBackendLlmId: String {
        get { BackendLlmSelection.selectionKey(providerId: llmProviderId, model: llmModelId) }
        set {
            guard let parsed = BackendLlmSelection.parse(selectionKey: newValue) else { return }
            llmProviderId = parsed.providerId
            llmModelId = parsed.model
        }
    }

    /// Generate Brief/Follow-up: для Backend нужен непустой каталог моделей.
    var allowsArtifactGeneration: Bool {
        switch llmEngine {
        case .backend:
            !backendLlmModels.isEmpty
        case .builtinTemplates, .ollama, .openaiCompat:
            true
        }
    }

    /// Подпись, когда Generate заблокирован из‑за пустого каталога backend.
    var backendCatalogMissingHelp: String {
        String(localized: "No model catalog — Settings → Refresh, or PROVIDERS_JSON / LLM_*")
    }

    /// Подпись для баннера Artifacts.
    var artifactsPipelineCaption: String {
        switch llmEngine {
        case .builtinTemplates:
            String(localized: "Built from Final · currently: built-in templates (no LLM)")
        case .ollama:
            String(localized: "Built from Final · LLM: Ollama (\(llmModelId))")
        case .openaiCompat:
            String(localized: "Built from Final · LLM: OpenAI-compatible (\(llmModelId))")
        case .backend:
            if llmModelId.isEmpty {
                String(localized: "Built from Final · LLM: backend")
            } else {
                String(localized: "Built from Final · LLM: backend (\(llmProviderId) · \(llmModelId))")
            }
        }
    }

    /// Результат Refresh каталога: ошибка сети → сохранить cache; пустой Ok → сбросить selection.
    func applyBackendModelsCatalog(
        _ models: [FfiLlmModelRef],
        connectionError: String? = nil
    ) {
        if let connectionError, !connectionError.isEmpty {
            backendLlmModelsMessage = String(localized: "Could not refresh the catalog: \(connectionError)")
            return
        }

        backendLlmModels = models
        if models.isEmpty {
            clearBackendLlmSelection()
            backendLlmModelsMessage =
                String(localized: "No models — configure PROVIDERS_JSON / LLM_* on the backend")
            return
        }

        backendLlmModelsMessage = ""
        let currentKey = selectedBackendLlmId
        let keys = Set(
            models.map {
                BackendLlmSelection.selectionKey(providerId: $0.providerId, model: $0.model)
            }
        )
        if !keys.contains(currentKey), let first = models.first {
            llmProviderId = first.providerId
            llmModelId = first.model
        }
    }

    func clearBackendLlmSelection() {
        llmProviderId = ""
        llmModelId = ""
    }
}

/// Идентичность выбора модели backend в Settings Picker.
struct BackendLlmSelection: Hashable, Identifiable {
    var id: String {
        Self.selectionKey(providerId: providerId, model: model)
    }

    let providerId: String
    let model: String
    let displayName: String

    var pickerLabel: String {
        displayName.isEmpty ? "\(providerId) · \(model)" : displayName
    }

    static func selectionKey(providerId: String, model: String) -> String {
        "\(providerId)|\(model)"
    }

    static func parse(selectionKey: String) -> (providerId: String, model: String)? {
        guard let separator = selectionKey.firstIndex(of: "|") else { return nil }
        let providerId = String(selectionKey[..<separator])
        let model = String(selectionKey[selectionKey.index(after: separator)...])
        guard !providerId.isEmpty, !model.isEmpty else { return nil }
        return (providerId, model)
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
        isAvailable ? displayName : String(localized: "\(displayName) — coming soon")
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
        isAvailable ? displayName : String(localized: "\(displayName) — coming soon")
    }

    /// Free-text Model id (Ollama / OpenAI-compat). Backend — picker каталога.
    var needsModel: Bool {
        switch self {
        case .builtinTemplates, .backend: false
        case .ollama, .openaiCompat: true
        }
    }

    var needsBackendModelPicker: Bool {
        self == .backend
    }

    var needsUrl: Bool {
        switch self {
        case .builtinTemplates, .backend: false
        case .ollama, .openaiCompat: true
        }
    }
}
