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
}
