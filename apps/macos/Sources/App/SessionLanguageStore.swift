import Foundation
import Observation

/// Stub политики языка сессии; primary прокидывается в MeetingCore через UI.
@Observable
final class SessionLanguageStore {
    /// Primary language распознавания; по умолчанию русский.
    var primary: SpeechLanguage = .ru

    /// Разрешённый набор v1.
    let allowed: [SpeechLanguage] = [.ru, .en, .es]
}
