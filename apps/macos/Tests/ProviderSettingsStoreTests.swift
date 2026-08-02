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

    func testUnavailableEnginesAreMarked() {
        XCTAssertFalse(PostCallSttEngine.backendWhisperX.isAvailable)
        XCTAssertFalse(LlmEngine.ollama.isAvailable)
        XCTAssertFalse(LlmEngine.openaiCompat.isAvailable)
        XCTAssertFalse(LlmEngine.backend.isAvailable)
    }

    func testSelectingUnavailablePostCallResetsToLocalFinal() {
        let store = ProviderSettingsStore()
        store.postCallStt = .backendWhisperX
        XCTAssertEqual(store.postCallStt, .localFinal)
    }

    func testSelectingUnavailableLlmResetsToBuiltin() {
        let store = ProviderSettingsStore()
        store.llmEngine = .ollama
        XCTAssertEqual(store.llmEngine, .builtinTemplates)
    }

    func testArtifactsCaptionMentionsFinalAndTemplates() {
        let store = ProviderSettingsStore()
        XCTAssertTrue(store.artifactsPipelineCaption.contains("Final"))
        XCTAssertTrue(store.artifactsPipelineCaption.contains("builtin"))
    }
}
