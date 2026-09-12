import SwiftUI
import Translation

/// Невидимая вью, за которой живёт `TranslationSession` (ADR-008).
///
/// Сессия выдаётся только модификатором `.translationTask` и умирает
/// вместе с вью: Apple бросает `fatalError` при обращении к ней после
/// исчезновения вью или смены конфигурации. Поэтому цикл перевода живёт
/// в замыкании, а не в отдельном объекте.
struct TranslationHostView: View {
    let bridge: HostTranslationBridge
    let source: SpeechLanguage
    let target: SpeechLanguage
    let isEnabled: Bool

    private var configuration: TranslationSession.Configuration? {
        guard isEnabled, source != target else { return nil }
        return TranslationSession.Configuration(
            source: Locale.Language(identifier: source.rawValue),
            target: Locale.Language(identifier: target.rawValue)
        )
    }

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .translationTask(configuration) { session in
                await bridge.run(
                    session: SystemTranslationSession(session),
                    source: source,
                    target: target
                )
            }
    }
}
