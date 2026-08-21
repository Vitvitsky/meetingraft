@testable import MeetingRaft
import SwiftUI
import XCTest

final class AppearanceSettingsStoreTests: XCTestCase {
    private func makeDefaults(_ name: String = #function) -> UserDefaults {
        let suite = "test.appearance.\(name)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }

    /// Умолчание — системная тема. Приложение, навязывающее свою при
    /// первом запуске, спорит с настройкой, которую человек уже сделал в
    /// системе.
    func testDefaultsToAuto() {
        let store = AppearanceSettingsStore(defaults: makeDefaults())

        XCTAssertEqual(store.preference, .auto)
        XCTAssertNil(store.colorScheme)
    }

    func testExplicitChoiceMapsToScheme() {
        let store = AppearanceSettingsStore(defaults: makeDefaults())

        store.preference = .light
        XCTAssertEqual(store.colorScheme, .light)

        store.preference = .dark
        XCTAssertEqual(store.colorScheme, .dark)
    }

    func testChoiceSurvivesRestart() {
        let defaults = makeDefaults()
        let first = AppearanceSettingsStore(defaults: defaults)
        first.preference = .light

        let second = AppearanceSettingsStore(defaults: defaults)

        XCTAssertEqual(second.preference, .light)
    }

    /// Мусор в `UserDefaults` не должен ронять запуск и не должен молча
    /// становиться тёмной темой: неизвестное значение — это «не выбрано».
    func testUnknownStoredValueFallsBackToAuto() {
        let defaults = makeDefaults()
        defaults.set("chartreuse", forKey: "appearance.preference")

        let store = AppearanceSettingsStore(defaults: defaults)

        XCTAssertEqual(store.preference, .auto)
    }

    /// Список для пикера обязан быть полным: пропущенный вариант — это
    /// тема, которую невозможно выбрать, при живом хранилище.
    func testEveryPreferenceIsOfferedToThePicker() {
        XCTAssertEqual(AppearancePreference.allCases, [.auto, .light, .dark])
        for preference in AppearancePreference.allCases {
            XCTAssertFalse(preference.title.isEmpty)
        }
    }
}
