@testable import MeetingRaft
import XCTest

/// Детект по списку процессов — то, что пользователь вправе проверить.
/// Тест фиксирует, что мы смотрим ровно на bundle id и ни на что больше.
final class MeetingDetectorTests: XCTestCase {
    func testKnownBundleIdentifiersResolve() {
        XCTAssertEqual(MeetingApp.named(bundleId: "us.zoom.xos"), .zoom)
        XCTAssertEqual(MeetingApp.named(bundleId: "com.microsoft.teams2"), .teams)
        XCTAssertNil(MeetingApp.named(bundleId: "com.apple.Safari"))
    }

    func testNothingDetectedAmongUnrelatedApps() {
        let running = ["com.apple.Safari", "com.apple.Terminal", "com.apple.dt.Xcode"]

        XCTAssertNil(MeetingApp.firstRunning(bundleIds: running))
    }

    /// Порядок берётся из списка запущенных, а не из нашего перечисления:
    /// если открыты и Zoom, и Slack, назвать надо первый из системного
    /// списка, а не первый по алфавиту.
    func testDetectionFollowsOrderOfRunningApps() {
        XCTAssertEqual(
            MeetingApp.firstRunning(bundleIds: ["com.tinyspeck.slackmacgap", "us.zoom.xos"]),
            .slack
        )
        XCTAssertEqual(
            MeetingApp.firstRunning(bundleIds: ["us.zoom.xos", "com.tinyspeck.slackmacgap"]),
            .zoom
        )
    }

    func testBothTeamsGenerationsShareDisplayName() {
        XCTAssertEqual(MeetingApp.teams.displayName, MeetingApp.teamsClassic.displayName)
    }

    func testIdentifiersAreUnique() {
        let ids = MeetingApp.allCases.map(\.id)

        XCTAssertEqual(Set(ids).count, ids.count)
    }

    /// Значок обязан отличаться формой: в монохромной строке меню цвет не
    /// передаётся вовсе.
    func testMenuIconDiffersInEveryState() {
        let idle = MenuBarIcon.systemImage(isRecording: false, hasDetectedMeeting: false)
        let detected = MenuBarIcon.systemImage(isRecording: false, hasDetectedMeeting: true)
        let recording = MenuBarIcon.systemImage(isRecording: true, hasDetectedMeeting: false)

        XCTAssertEqual(Set([idle, detected, recording]).count, 3)
    }

    /// Во время записи значок не должен зависеть от детекта: идёт запись —
    /// значит идёт, что бы ни было запущено рядом.
    func testRecordingIconIgnoresDetection() {
        XCTAssertEqual(
            MenuBarIcon.systemImage(isRecording: true, hasDetectedMeeting: true),
            MenuBarIcon.systemImage(isRecording: true, hasDetectedMeeting: false)
        )
    }
}

@MainActor
final class MeetingDetectorPollingTests: XCTestCase {
    func testRefreshPicksUpRunningMeetingApp() {
        var running = ["com.apple.Safari"]
        let detector = MeetingDetector(runningBundleIds: { running })

        detector.refresh()
        XCTAssertNil(detector.detected)

        running.append("us.zoom.xos")
        detector.refresh()

        XCTAssertEqual(detector.detected, .zoom)
    }

    func testStopClearsDetection() {
        let detector = MeetingDetector(runningBundleIds: { ["us.zoom.xos"] })
        detector.refresh()
        XCTAssertNotNil(detector.detected)

        detector.stop()

        XCTAssertNil(detector.detected)
    }
}
