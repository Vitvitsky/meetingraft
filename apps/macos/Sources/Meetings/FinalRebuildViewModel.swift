import Foundation
import Observation

/// Контракт пересбора Final для presentation-модели и тестов.
protocol FinalRebuildCoreProviding: AnyObject {
    func startFinalRebuild(meetingId: String) -> String
    func finalRebuildProgress(jobId: String) -> FfiRebuildProgress
    func cancelFinalRebuild(jobId: String)
    func activeFinalRebuild(meetingId: String) -> String
    func diffLiveVsFinal(meetingId: String, version: UInt32) -> [FfiDiffSpan]
}

extension MeetingCore: FinalRebuildCoreProviding {}

/// Presentation-модель фонового пересбора Final (Phase 10, T9).
///
/// Проход идёт минутами в потоке Rust, поэтому здесь только опрос
/// состояния: вся работа и вся её отмена живут в ядре.
@Observable
@MainActor
final class FinalRebuildViewModel {
    private(set) var jobId = ""
    /// `queued` | `running` | `succeeded` | `failed` | `cancelled`; пусто — прохода не было.
    private(set) var state = ""
    private(set) var fraction: Double = 0
    private(set) var errorMessage = ""
    /// Что фактически отработало — источник provenance, а не ожидание.
    private(set) var provenance = ""

    private let core: any FinalRebuildCoreProviding
    private let pollNanoseconds: UInt64
    private var pollTask: Task<Void, Never>?

    var isRunning: Bool {
        state == "queued" || state == "running"
    }

    init(
        core: any FinalRebuildCoreProviding,
        pollNanoseconds: UInt64 = 500_000_000
    ) {
        self.core = core
        self.pollNanoseconds = pollNanoseconds
    }

    /// Подхватить проход, начатый до открытия экрана.
    func attach(meetingId: String) {
        let existing = core.activeFinalRebuild(meetingId: meetingId)
        guard !existing.isEmpty else { return }
        jobId = existing
        startPolling()
    }

    func start(meetingId: String) {
        guard !isRunning else { return }
        errorMessage = ""
        provenance = ""
        fraction = 0
        jobId = core.startFinalRebuild(meetingId: meetingId)
        state = "queued"
        startPolling()
    }

    func cancel() {
        guard !jobId.isEmpty else { return }
        core.cancelFinalRebuild(jobId: jobId)
    }

    func stopPolling() {
        pollTask?.cancel()
        pollTask = nil
    }

    private func startPolling() {
        stopPolling()
        pollTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                refresh()
                if !isRunning {
                    return
                }
                try? await Task.sleep(nanoseconds: pollNanoseconds)
            }
        }
    }

    /// Один опрос состояния; отделён от цикла ради тестов.
    func refresh() {
        guard !jobId.isEmpty else { return }
        let progress = core.finalRebuildProgress(jobId: jobId)
        state = progress.state
        fraction = progress.total > 0 ? Double(progress.done) / Double(progress.total) : 0
        errorMessage = progress.error
        // Provenance берётся только у успешного прохода: у прерванного
        // или упавшего называть источник нечем.
        provenance = progress.state == "succeeded" ? progress.note : ""
    }

    /// Текст состояния для интерфейса.
    var statusText: String {
        switch state {
        case "queued": String(localized: "Queued")
        case "running": String(localized: "Rebuilding… \(Int(fraction * 100))%")
        case "succeeded": provenance.isEmpty ? String(localized: "Done") : provenance
        case "failed": errorMessage.isEmpty ? String(localized: "Failed") : errorMessage
        case "cancelled": String(localized: "Cancelled")
        default: ""
        }
    }
}
