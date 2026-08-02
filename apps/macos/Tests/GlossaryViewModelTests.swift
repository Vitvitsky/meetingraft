@testable import MeetingRaft
import XCTest

@MainActor
final class GlossaryViewModelTests: XCTestCase {
    func testReloadPublishesTermsFromCore() {
        let expected = makeTerm(id: "term-1")
        let core = GlossaryCoreSpy(terms: [expected])
        let viewModel = GlossaryViewModel(core: core)

        viewModel.reload()

        XCTAssertEqual(viewModel.terms, [expected])
        XCTAssertNil(viewModel.errorMessage)
    }

    func testUpsertReloadsTermsAfterSuccess() {
        let term = makeTerm(id: "term-1")
        let core = GlossaryCoreSpy()
        core.termsAfterUpsert = [term]
        let viewModel = GlossaryViewModel(core: core)

        viewModel.upsert(term)

        XCTAssertEqual(core.upsertedTerms, [term])
        XCTAssertEqual(viewModel.terms, [term])
        XCTAssertNil(viewModel.errorMessage)
    }

    func testDeletePublishesCoreErrorWithoutReloading() {
        let core = GlossaryCoreSpy()
        core.deleteError = "delete failed"
        let viewModel = GlossaryViewModel(core: core)

        viewModel.delete(id: "term-1")

        XCTAssertEqual(core.deletedIds, ["term-1"])
        XCTAssertEqual(viewModel.errorMessage, "delete failed")
        XCTAssertEqual(core.listCallCount, 0)
    }

    func testImportPublishesSummaryAndReloadsTerms() {
        let imported = makeTerm(id: "imported")
        let core = GlossaryCoreSpy()
        core.importResult = FfiGlossaryImportResult(imported: 1, skipped: 2, error: "")
        core.termsAfterImport = [imported]
        let viewModel = GlossaryViewModel(core: core)

        viewModel.importCsv("surface,canonical,language,scope")

        XCTAssertEqual(core.importedCsv, ["surface,canonical,language,scope"])
        XCTAssertEqual(viewModel.importMessage, "Импортировано: 1, пропущено: 2")
        XCTAssertEqual(viewModel.terms, [imported])
    }

    func testMeetingScopeAvailableOnlyForLiveSession() {
        let viewModel = GlossaryViewModel(core: GlossaryCoreSpy())

        XCTAssertEqual(viewModel.availableScopes(liveSessionId: nil), [.global])
        XCTAssertEqual(viewModel.availableScopes(liveSessionId: "live-1"), [.global, .meeting])
    }

    private func makeTerm(id: String) -> FfiGlossaryTerm {
        FfiGlossaryTerm(
            id: id,
            surface: "униффи",
            canonical: "UniFFI",
            language: "ru",
            scope: .global,
            meetingId: ""
        )
    }
}

private final class GlossaryCoreSpy: GlossaryCoreProviding {
    var terms: [FfiGlossaryTerm]
    var termsAfterUpsert: [FfiGlossaryTerm]?
    var termsAfterImport: [FfiGlossaryTerm]?
    var deleteError = ""
    var importResult = FfiGlossaryImportResult(imported: 0, skipped: 0, error: "")
    private(set) var upsertedTerms: [FfiGlossaryTerm] = []
    private(set) var deletedIds: [String] = []
    private(set) var importedCsv: [String] = []
    private(set) var listCallCount = 0

    init(terms: [FfiGlossaryTerm] = []) {
        self.terms = terms
    }

    func listGlossaryTerms() -> [FfiGlossaryTerm] {
        listCallCount += 1
        return terms
    }

    func upsertGlossaryTerm(term: FfiGlossaryTerm) -> String {
        upsertedTerms.append(term)
        if let termsAfterUpsert {
            terms = termsAfterUpsert
        }
        return ""
    }

    func deleteGlossaryTerm(id: String) -> String {
        deletedIds.append(id)
        return deleteError
    }

    func importGlossaryCsv(csv: String) -> FfiGlossaryImportResult {
        importedCsv.append(csv)
        if let termsAfterImport {
            terms = termsAfterImport
        }
        return importResult
    }
}
