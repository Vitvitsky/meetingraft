import AppKit
import Foundation
import Observation

/// Распознавание запущенного приложения для встреч (ТЗ редизайна §4.7).
///
/// Список bundle id открыт и лежит в коде: детект по процессам — вещь,
/// которую пользователь вправе проверить, а не принимать на веру.
enum MeetingApp: String, CaseIterable, Identifiable, Sendable {
    case zoom = "us.zoom.xos"
    case teams = "com.microsoft.teams2"
    case teamsClassic = "com.microsoft.teams"
    case webex = "Cisco-Systems.Spark"
    case slack = "com.tinyspeck.slackmacgap"
    case discord = "com.hnc.Discord"
    case facetime = "com.apple.FaceTime"

    var id: String {
        rawValue
    }

    var displayName: String {
        switch self {
        case .zoom: "Zoom"
        case .teams, .teamsClassic: "Microsoft Teams"
        case .webex: "Webex"
        case .slack: "Slack"
        case .discord: "Discord"
        case .facetime: "FaceTime"
        }
    }

    /// Приложение по bundle id, если оно нам известно.
    static func named(bundleId: String) -> MeetingApp? {
        allCases.first { $0.rawValue == bundleId }
    }

    /// Первое известное приложение среди запущенных.
    ///
    /// Порядок перебора идёт по списку запущенных, а не по нашему
    /// перечислению: если открыты и Zoom, и Slack, назвать надо тот, что
    /// система считает более активным.
    static func firstRunning(bundleIds: [String]) -> MeetingApp? {
        bundleIds.lazy.compactMap(named(bundleId:)).first
    }
}

/// Следит за тем, запущено ли приложение для встреч.
///
/// Только факт запуска: определять, идёт ли **звонок**, пришлось бы
/// лезть в чужие окна или в микрофонную активность, а это ровно то
/// наблюдение за пользователем, которого продукт обещает не делать.
@Observable
@MainActor
final class MeetingDetector {
    private(set) var detected: MeetingApp?

    private var timer: Timer?
    private let interval: TimeInterval
    private let runningBundleIds: @MainActor () -> [String]

    init(
        interval: TimeInterval = 5,
        runningBundleIds: @escaping @MainActor () -> [String] = {
            NSWorkspace.shared.runningApplications.compactMap(\.bundleIdentifier)
        }
    ) {
        self.interval = interval
        self.runningBundleIds = runningBundleIds
    }

    func start() {
        guard timer == nil else { return }
        refresh()
        let timer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.refresh()
            }
        }
        self.timer = timer
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        detected = nil
    }

    /// Один опрос; отделён от таймера ради тестов.
    func refresh() {
        detected = MeetingApp.firstRunning(bundleIds: runningBundleIds())
    }
}
