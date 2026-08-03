import Foundation
import Observation

/// Поведение приложения во время записи (ТЗ редизайна §4.6).
///
/// В отличие от остальных настроек, эта переживает перезапуск: человек
/// настраивает её один раз под свою манеру вести встречи, и сбрасывать её
/// при каждом запуске — раздражать без причины.
@Observable
final class PresenceSettingsStore {
    /// Субтитры поверх остальных приложений.
    var showsOverlay: Bool {
        didSet {
            defaults.set(showsOverlay, forKey: Keys.showsOverlay)
        }
    }

    /// Прятать главное окно на время записи.
    var minimizesMainWindow: Bool {
        didSet {
            defaults.set(minimizesMainWindow, forKey: Keys.minimizesMainWindow)
        }
    }

    /// Непрозрачность накладки, 0.2…1.
    var overlayOpacity: Double {
        didSet {
            defaults.set(overlayOpacity, forKey: Keys.overlayOpacity)
        }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        // Накладка включена по умолчанию: без неё смысл сворачивать окно
        // теряется, а именно ради этого сценария всё и делается.
        showsOverlay = defaults.object(forKey: Keys.showsOverlay) as? Bool ?? true
        minimizesMainWindow = defaults.object(forKey: Keys.minimizesMainWindow) as? Bool ?? true
        let storedOpacity = defaults.object(forKey: Keys.overlayOpacity) as? Double
        overlayOpacity = storedOpacity ?? 0.85
    }

    private enum Keys {
        static let showsOverlay = "presence.showsOverlay"
        static let minimizesMainWindow = "presence.minimizesMainWindow"
        static let overlayOpacity = "presence.overlayOpacity"
    }
}

/// Что должно происходить с окнами при текущем состоянии записи.
///
/// Отдельный тип, а не пара `if` внутри вью: решение принимается в двух
/// местах — при старте записи и при смене настройки во время неё, — и
/// расхождение между ними даёт зависшую накладку поверх всех окон.
struct PresenceDecision: Equatable {
    var showsOverlay: Bool
    var hidesMainWindow: Bool

    static func make(isRecording: Bool, settings: PresenceSettingsStore) -> PresenceDecision {
        guard isRecording else {
            // Вне записи окна возвращаются в исходное состояние всегда,
            // независимо от настроек: иначе выключенная посреди сессии
            // опция оставила бы накладку висеть.
            return PresenceDecision(showsOverlay: false, hidesMainWindow: false)
        }
        return PresenceDecision(
            showsOverlay: settings.showsOverlay,
            // Прятать окно, не показав накладку, значит оставить человека
            // без единого признака того, что запись идёт.
            hidesMainWindow: settings.minimizesMainWindow && settings.showsOverlay
        )
    }
}
