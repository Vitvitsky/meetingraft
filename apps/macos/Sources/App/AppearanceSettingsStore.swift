import Foundation
import Observation
import SwiftUI

/// Тема оформления: системная, светлая или тёмная.
///
/// До этого окно было принудительно тёмным, потому что светлой палитры не
/// существовало. Теперь существует, и выбор возвращается человеку.
enum AppearancePreference: String, CaseIterable, Identifiable {
    case auto
    case light
    case dark

    var id: String {
        rawValue
    }

    var title: String {
        switch self {
        case .auto: String(localized: "System")
        case .light: String(localized: "Light")
        case .dark: String(localized: "Dark")
        }
    }
}

/// Выбранная тема. Переживает перезапуск: настройка делается один раз.
@Observable
final class AppearanceSettingsStore {
    var preference: AppearancePreference {
        didSet {
            defaults.set(preference.rawValue, forKey: Keys.preference)
        }
    }

    /// `nil` означает «как в системе» — именно это `preferredColorScheme`
    /// понимает как отсутствие навязанной темы.
    var colorScheme: ColorScheme? {
        switch preference {
        case .auto: nil
        case .light: .light
        case .dark: .dark
        }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let stored = defaults.string(forKey: Keys.preference)
        // Неизвестное значение — это «не выбрано», а не повод упасть или
        // молча назначить тёмную.
        preference = stored.flatMap(AppearancePreference.init(rawValue:)) ?? .auto
    }

    private enum Keys {
        static let preference = "appearance.preference"
    }
}
