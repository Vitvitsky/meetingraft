import Foundation
import Observation

/// Минимальный контракт glossary-фасада для presentation model и тестов.
protocol GlossaryCoreProviding: AnyObject {
    func listGlossaryTerms() -> [FfiGlossaryTerm]
    func upsertGlossaryTerm(term: FfiGlossaryTerm) -> String
    func deleteGlossaryTerm(id: String) -> String
    func importGlossaryCsv(csv: String) -> FfiGlossaryImportResult
}

extension MeetingCore: GlossaryCoreProviding {}

/// Варианты scope, доступные в редакторе термина.
enum GlossaryScopeSelection: String, Identifiable {
    case global
    case meeting

    var id: String {
        rawValue
    }

    var title: String {
        switch self {
        case .global: "Global"
        case .meeting: "Meeting"
        }
    }
}

/// Presentation model списка глоссария и операций UniFFI.
@Observable
@MainActor
final class GlossaryViewModel {
    private(set) var terms: [FfiGlossaryTerm] = []
    private(set) var errorMessage: String?
    private(set) var importMessage: String?
    var filter: GlossaryFilter = .all
    var query = ""

    /// Термины под текущим фильтром и запросом.
    func visibleTerms(liveSessionId: String?) -> [FfiGlossaryTerm] {
        terms.filter {
            filter.matches($0, liveSessionId: liveSessionId) && $0.matches(query: query)
        }
    }

    /// Сколько терминов в области — число рядом с её названием.
    ///
    /// Запрос сюда не входит: счётчики показывают размер словаря, а не
    /// то, сколько нашлось по строке поиска.
    func count(for filter: GlossaryFilter, liveSessionId: String?) -> Int {
        terms.count { filter.matches($0, liveSessionId: liveSessionId) }
    }

    private let core: any GlossaryCoreProviding

    init(core: (any GlossaryCoreProviding)? = nil, dataRoot: String? = nil) {
        if let core {
            self.core = core
            return
        }

        let root: URL
        if let dataRoot {
            root = URL(fileURLWithPath: dataRoot, isDirectory: true)
        } else {
            let support = FileManager.default.urls(
                for: .applicationSupportDirectory,
                in: .userDomainMask
            ).first!
            root = support.appendingPathComponent("meetingraft", isDirectory: true)
        }
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        self.core = MeetingCore.withDataRoot(dataRoot: root.path)
    }

    func reload() {
        terms = core.listGlossaryTerms()
        errorMessage = nil
    }

    func upsert(_ term: FfiGlossaryTerm) {
        let error = core.upsertGlossaryTerm(term: term)
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        reload()
    }

    func delete(id: String) {
        let error = core.deleteGlossaryTerm(id: id)
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        reload()
    }

    func importCsv(_ csv: String) {
        let result = core.importGlossaryCsv(csv: csv)
        guard result.error.isEmpty else {
            errorMessage = result.error
            return
        }
        importMessage = "Импортировано: \(result.imported), пропущено: \(result.skipped)"
        reload()
    }

    func availableScopes(liveSessionId: String?) -> [GlossaryScopeSelection] {
        liveSessionId == nil ? [.global] : [.global, .meeting]
    }

    func canEdit(_ term: FfiGlossaryTerm, liveSessionId: String?) -> Bool {
        switch term.scope {
        case .global:
            true
        case .meeting:
            liveSessionId == term.meetingId
        }
    }

    func showError(_ message: String) {
        errorMessage = message
    }

    func dismissError() {
        errorMessage = nil
    }

    func dismissImportMessage() {
        importMessage = nil
    }
}
