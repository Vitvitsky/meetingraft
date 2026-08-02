import Foundation
import Observation

/// Stub политики языка сессии; в Phase 2 заменяется Rust/UniFFI.
@Observable
final class SessionLanguageStore {
    /// Primary language; по умолчанию русский.
    var primary: SpeechLanguage = .ru

    /// Разрешённый набор v1.
    let allowed: [SpeechLanguage] = [.ru, .en, .es]
}
