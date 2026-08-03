@testable import MeetingRaft
import XCTest

@MainActor
final class ProviderSettingsStoreTests: XCTestCase {
    func testDefaultsAreLocalFinalAndBuiltinTemplates() {
        let store = ProviderSettingsStore()
        XCTAssertEqual(store.postCallStt, .localFinal)
        XCTAssertEqual(store.llmEngine, .builtinTemplates)
        XCTAssertEqual(store.llmProviderId, "default")
        XCTAssertTrue(store.postCallStt.isAvailable)
        XCTAssertTrue(store.llmEngine.isAvailable)
    }

    func testUnavailablePostCallEngineIsMarked() {
        XCTAssertFalse(PostCallSttEngine.backendWhisperX.isAvailable)
    }

    func testBackendLlmIsAvailableAndSelectable() {
        let store = ProviderSettingsStore()
        XCTAssertTrue(LlmEngine.backend.isAvailable)
        store.llmEngine = .backend
        store.llmModelId = ""
        XCTAssertEqual(store.llmEngine, .backend)
        XCTAssertFalse(LlmEngine.backend.needsUrl)
        XCTAssertEqual(
            store.artifactsPipelineCaption,
            "Генерация из Final · LLM: backend"
        )
    }

    func testBackendCaptionIncludesProviderAndModelWhenSet() {
        let store = ProviderSettingsStore()
        store.llmEngine = .backend
        store.llmProviderId = "openai"
        store.llmModelId = "gpt-4o-mini"
        XCTAssertEqual(
            store.artifactsPipelineCaption,
            "Генерация из Final · LLM: backend (openai · gpt-4o-mini)"
        )
    }

    func testBackendUsesPickerNotFreeTextModel() {
        XCTAssertFalse(LlmEngine.backend.needsModel)
        XCTAssertTrue(LlmEngine.backend.needsBackendModelPicker)
        XCTAssertFalse(LlmEngine.backend.needsUrl)
        XCTAssertTrue(LlmEngine.ollama.needsModel)
        XCTAssertFalse(LlmEngine.ollama.needsBackendModelPicker)
        XCTAssertTrue(LlmEngine.openaiCompat.needsModel)
        XCTAssertFalse(LlmEngine.builtinTemplates.needsModel)
    }

    func testBackendLlmSelectionKeyRoundTrip() {
        let key = BackendLlmSelection.selectionKey(providerId: "default", model: "gemma2")
        XCTAssertEqual(key, "default|gemma2")
        let parsed = BackendLlmSelection.parse(selectionKey: key)
        XCTAssertEqual(parsed?.providerId, "default")
        XCTAssertEqual(parsed?.model, "gemma2")

        let store = ProviderSettingsStore()
        store.selectedBackendLlmId = "openai|gpt-4o-mini"
        XCTAssertEqual(store.llmProviderId, "openai")
        XCTAssertEqual(store.llmModelId, "gpt-4o-mini")
        XCTAssertEqual(store.selectedBackendLlmId, "openai|gpt-4o-mini")
    }

    func testSelectingUnavailablePostCallResetsToLocalFinal() {
        let store = ProviderSettingsStore()
        store.postCallStt = .backendWhisperX
        XCTAssertEqual(store.postCallStt, .localFinal)
    }

    func testOllamaAndOpenAiCompatAreAvailable() {
        let store = ProviderSettingsStore()
        XCTAssertTrue(LlmEngine.ollama.isAvailable)
        XCTAssertTrue(LlmEngine.openaiCompat.isAvailable)
        XCTAssertTrue(LlmEngine.ollama.needsUrl)

        store.llmEngine = .ollama
        XCTAssertEqual(store.llmEngine, .ollama)
        store.llmEngine = .openaiCompat
        XCTAssertEqual(store.llmEngine, .openaiCompat)
    }

    func testArtifactsCaptionMentionsFinalAndTemplates() {
        let store = ProviderSettingsStore()
        XCTAssertTrue(store.artifactsPipelineCaption.contains("Final"))
        XCTAssertTrue(store.artifactsPipelineCaption.contains("builtin"))
    }

    func testExportFolderPathDefault() {
        let store = ProviderSettingsStore()
        XCTAssertTrue(store.exportFolderPath.contains("MeetingRaft"))
    }

    func testDefaultSelectedSttModelIdIsAuto() {
        let store = ProviderSettingsStore()
        XCTAssertEqual(store.selectedSttModelId, .auto)
    }

    func testBackendBlocksArtifactGenerationWhenCatalogEmpty() {
        let store = ProviderSettingsStore()
        store.llmEngine = .backend
        XCTAssertTrue(store.backendLlmModels.isEmpty)
        XCTAssertFalse(store.allowsArtifactGeneration)
        XCTAssertTrue(store.backendCatalogMissingHelp.contains("PROVIDERS_JSON"))
    }

    func testBackendAllowsArtifactGenerationWhenCatalogPresent() {
        let store = ProviderSettingsStore()
        store.llmEngine = .backend
        store.applyBackendModelsCatalog([
            FfiLlmModelRef(providerId: "home", model: "m1", displayName: "One"),
        ])
        XCTAssertTrue(store.allowsArtifactGeneration)
        XCTAssertEqual(store.llmProviderId, "home")
        XCTAssertEqual(store.llmModelId, "m1")
    }

    func testEmptyCatalogRefreshClearsSelection() {
        let store = ProviderSettingsStore()
        store.llmProviderId = "stale"
        store.llmModelId = "old-model"
        store.applyBackendModelsCatalog([])
        XCTAssertTrue(store.backendLlmModels.isEmpty)
        XCTAssertEqual(store.llmProviderId, "")
        XCTAssertEqual(store.llmModelId, "")
        XCTAssertTrue(store.backendLlmModelsMessage.contains("PROVIDERS_JSON"))
    }

    func testCatalogRefreshNetworkErrorKeepsPreviousCache() {
        let store = ProviderSettingsStore()
        let cached = FfiLlmModelRef(providerId: "home", model: "m1", displayName: "One")
        store.applyBackendModelsCatalog([cached])
        store.llmProviderId = "home"
        store.llmModelId = "m1"

        store.applyBackendModelsCatalog([], connectionError: "connection refused")

        XCTAssertEqual(store.backendLlmModels, [cached])
        XCTAssertEqual(store.llmProviderId, "home")
        XCTAssertEqual(store.llmModelId, "m1")
        XCTAssertTrue(store.backendLlmModelsMessage.contains("connection refused"))
        XCTAssertTrue(store.allowsArtifactGeneration)
    }
}
