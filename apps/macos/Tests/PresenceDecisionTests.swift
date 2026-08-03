@testable import MeetingRaft
import XCTest

/// Решение об окнах принимается в нескольких местах — при старте записи
/// и при смене настройки во время неё. Расхождение между ними оставляет
/// накладку висеть поверх всех окон, и убрать её будет нечем.
final class PresenceDecisionTests: XCTestCase {
    private func settings(
        overlay: Bool = true,
        minimize: Bool = true
    ) -> PresenceSettingsStore {
        let defaults = UserDefaults(suiteName: "presence-tests-\(UUID().uuidString)")!
        let store = PresenceSettingsStore(defaults: defaults)
        store.showsOverlay = overlay
        store.minimizesMainWindow = minimize
        return store
    }

    func testRecordingWithBothOptionsShowsOverlayAndHidesWindow() {
        let decision = PresenceDecision.make(isRecording: true, settings: settings())

        XCTAssertTrue(decision.showsOverlay)
        XCTAssertTrue(decision.hidesMainWindow)
    }

    /// Спрятать окно, не показав накладку, значит оставить человека без
    /// единого признака того, что запись идёт.
    func testWindowIsNeverHiddenWithoutOverlay() {
        let decision = PresenceDecision.make(
            isRecording: true,
            settings: settings(overlay: false, minimize: true)
        )

        XCTAssertFalse(decision.showsOverlay)
        XCTAssertFalse(decision.hidesMainWindow)
    }

    func testOverlayWithoutMinimizeKeepsWindow() {
        let decision = PresenceDecision.make(
            isRecording: true,
            settings: settings(overlay: true, minimize: false)
        )

        XCTAssertTrue(decision.showsOverlay)
        XCTAssertFalse(decision.hidesMainWindow)
    }

    /// Вне записи окна возвращаются всегда, что бы ни стояло в настройках:
    /// иначе выключенная посреди сессии опция оставит накладку висеть.
    func testStoppingRecordingRestoresEverything() {
        for overlay in [true, false] {
            for minimize in [true, false] {
                let decision = PresenceDecision.make(
                    isRecording: false,
                    settings: settings(overlay: overlay, minimize: minimize)
                )

                XCTAssertEqual(
                    decision,
                    PresenceDecision(showsOverlay: false, hidesMainWindow: false),
                    "overlay=\(overlay) minimize=\(minimize)"
                )
            }
        }
    }

    /// Настройка переживает перезапуск: сбрасывать её каждый раз —
    /// раздражать без причины.
    func testSettingsPersistAcrossInstances() {
        let suite = "presence-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!

        let first = PresenceSettingsStore(defaults: defaults)
        first.showsOverlay = false
        first.overlayOpacity = 0.4

        let second = PresenceSettingsStore(defaults: defaults)

        XCTAssertFalse(second.showsOverlay)
        XCTAssertEqual(second.overlayOpacity, 0.4, accuracy: 0.001)
    }

    func testDefaultsEnableOverlay() {
        let defaults = UserDefaults(suiteName: "presence-fresh-\(UUID().uuidString)")!

        let store = PresenceSettingsStore(defaults: defaults)

        XCTAssertTrue(store.showsOverlay, "без накладки сворачивать окно незачем")
    }
}
