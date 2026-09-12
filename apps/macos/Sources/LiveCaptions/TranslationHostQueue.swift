/// Часть `MeetingCore`, которой пользуется мост. Шов для тестов: мост
/// проверяется без настоящего ядра и без ожидания тактов demo-скрипта.
@MainActor
protocol TranslationHostQueue: AnyObject {
    func setHostTranslationAvailable(available: Bool)
    func drainHostTranslationRequests() -> [FfiHostTranslationRequest]
    func completeHostTranslation(id: String, translatedText: String) -> String
}

extension MeetingCore: TranslationHostQueue {}
