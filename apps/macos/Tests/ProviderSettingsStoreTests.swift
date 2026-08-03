@testable import MeetingRaft
import XCTest

@MainActor
final class ProviderSettingsStoreTests: XCTestCase {
    func testDefaultsAreLocalFinalAndBuiltinTemplates() {
        let store = ProviderSettingsStore()
        XCTAssertEqual(store.postCallStt, .localFinal)
        XCTAssertEqual(store.llmEngine, .builtinTemplates)
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
        XCTAssertEqual(store.llmEngine, .backend)
        XCTAssertFalse(LlmEngine.backend.needsUrl)
        XCTAssertEqual(
            store.artifactsPipelineCaption,
            "Генерация из Final · LLM: backend"
        )
    }

    func testBackendNeedsModelButNotUrl() {
        XCTAssertTrue(LlmEngine.backend.needsModel)
        XCTAssertFalse(LlmEngine.backend.needsUrl)
        XCTAssertTrue(LlmEngine.ollama.needsModel)
        XCTAssertTrue(LlmEngine.openaiCompat.needsModel)
        XCTAssertFalse(LlmEngine.builtinTemplates.needsModel)
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
