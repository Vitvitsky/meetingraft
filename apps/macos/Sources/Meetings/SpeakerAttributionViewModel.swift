import Foundation
import Observation

/// Контракт атрибуции говорящих для presentation model и тестов.
protocol SpeakerAttributionCoreProviding: AnyObject {
    func listSpeakers(meetingId: String) -> [FfiSpeaker]
    func upsertSpeaker(meetingId: String, id: String, displayName: String, sortIndex: Int64) -> String
    func deleteSpeaker(meetingId: String, id: String) -> String
    func listFinalSegments(meetingId: String, version: UInt32) -> [FfiFinalSegment]
    func listSpeakerStats(meetingId: String, version: UInt32) -> [FfiSpeakerStat]
    func assignChannelSpeaker(
        meetingId: String,
        version: UInt32,
        channelCode: String,
        speakerId: String
    ) -> String
    func assignSegmentSpeaker(
        meetingId: String,
        version: UInt32,
        index: UInt32,
        speakerId: String
    ) -> String
    func unpinSegmentSpeaker(meetingId: String, version: UInt32, index: UInt32) -> String
    func editSegmentText(meetingId: String, version: UInt32, index: UInt32, text: String) -> String
    func listUnappliedEdits(meetingId: String) -> [FfiSegmentEdit]
    func deleteSegmentEdit(editId: String) -> String
    func promoteTermToReplacement(termId: String, meetingId: String, version: UInt32) -> String
    func segmentAudio(
        meetingId: String,
        channelCode: String,
        startMs: UInt64,
        endMs: UInt64
    ) -> FfiAudioFragment
    func meetingAudioBytes(meetingId: String) -> UInt64
    func listVoiceprints(meetingId: String) -> [FfiVoicePrint]
    func recomputeVoiceprints(
        meetingId: String,
        version: UInt32,
        accept: Float,
        margin: Float
    ) -> FfiVoicePrintPass
    func voiceprintDefaultAccept() -> Float
    func voiceprintDefaultMargin() -> Float
    func isVoiceEngineAvailable() -> Bool
    func isVoiceMemoryEnabled() -> Bool
    func rememberVoice(meetingId: String, speakerId: String) -> String
}

extension MeetingCore: SpeakerAttributionCoreProviding {}

/// Откуда у реплики взялось имя (ADR-013).
///
/// Зеркало `domain::SpeakerSource`, а не второе решение: коды приходят с
/// границы строками, и разбор их держится в одном месте. Незнакомый код —
/// от более новой версии ядра — читается как отсутствие подписи, потому
/// что защищать от перезаписи то, о чём мы ничего не знаем, нельзя.
enum SpeakerSource: String {
    case none = ""
    case channel
    case human
    case voiceprint

    init(code: String) {
        self = SpeakerSource(rawValue: code) ?? .none
    }
}

extension FfiFinalSegment {
    var source: SpeakerSource {
        SpeakerSource(code: speakerSource)
    }
}

/// Строка экрана Speakers: участник и его вклад в разговор.
struct SpeakerRowModel: Identifiable, Equatable {
    let id: String
    let displayName: String
    /// Пусто, если у участника нет реплик в этой версии Final.
    let channelCode: String
    let segmentCount: UInt32
    let speakingMs: UInt64
    /// Доля от общего времени речи, 0…1.
    let share: Double

    /// Имя для показа: удалённая запись оставляет реплики без имени, но
    /// прятать их нельзя — это время встречи.
    var label: String {
        displayName.isEmpty ? String(localized: "Unnamed") : displayName
    }

    /// Что сохранять из набранного в поле имени. `nil` — сохранять нечего.
    ///
    /// Отдельно от экрана, чтобы правило проверялось тестом: сохранение
    /// имени срабатывает и по уходу фокуса, и по исчезновению строки, и
    /// без общего решения эти три пути разъехались бы.
    ///
    /// Пустое имя не сохраняется — участник остался бы без подписи, и это
    /// выглядело бы как сбой атрибуции. Совпадение с прежним не пишется
    /// тоже: `upsertSpeaker` пересобирает markdown всей встречи, и делать
    /// это на каждый уход фокуса незачем.
    func nameToCommit(draft: String) -> String? {
        let trimmed = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed != displayName else { return nil }
        return trimmed
    }
}

/// Presentation model атрибуции говорящих: экраны Speakers и Final.
///
/// Обе вкладки читают одни данные — сегменты версии Final и сводку по
/// участникам, — поэтому модель одна: две разошлись бы в момент, когда
/// на одном экране правят имя, а на другом смотрят реплики.
@Observable
@MainActor
final class SpeakerAttributionViewModel {
    private(set) var speakers: [FfiSpeaker] = []
    private(set) var segments: [FfiFinalSegment] = []
    private(set) var rows: [SpeakerRowModel] = []
    private(set) var errorMessage: String?
    /// Индекс правящейся реплики; `nil` — никто не правится.
    private(set) var editingIndex: UInt32?
    /// Черновик правки живёт в модели, а не во вью: список
    /// перерисовывается на каждое обновление, и набранное терялось бы.
    var draftText = ""
    /// Правки, не легшие ни на одну версию после пересбора.
    private(set) var unappliedEdits: [FfiSegmentEdit] = []
    /// Есть ли у встречи запись вообще.
    ///
    /// Один вопрос на встречу, а не на реплику: чтение фрагмента — это
    /// поход на диск, а список перерисовывается на каждое нажатие
    /// клавиши. Кнопка прослушивания при удалённой записи (Epic 22) не
    /// показывается вовсе: нерабочая кнопка — это заглушка в интерфейсе.
    private(set) var audioAvailable = false
    /// Слепки голоса участников этой встречи (ADR-013).
    private(set) var voicePrints: [FfiVoicePrint] = []
    /// Итог последнего пересчёта; `nil` — в этой сессии не запускали.
    ///
    /// Держится в модели, а не выбрасывается в `errorMessage`: «пересчитал
    /// и ничего не изменилось» — законный ответ, и показывать его надо
    /// числами, а не тишиной.
    private(set) var lastPass: FfiVoicePrintPass?
    /// Собран ли движок голосов.
    ///
    /// Нет — вся ветка слепков не показывается. Это не то же, что
    /// «модель не скачана»: то человек исправит, а фичи, которой нет в
    /// бинаре, он не добавит ничем, и кнопка была бы заглушкой.
    private(set) var voiceEngineAvailable = false
    /// Включена ли память на голоса между встречами (ADR-013, задача 7).
    ///
    /// Читается из ядра, а не хранится здесь: признак живёт рядом с
    /// данными, которыми управляет, и переживает перезапуск выключенным.
    private(set) var voiceMemoryEnabled = false

    private let core: any SpeakerAttributionCoreProviding
    private var meetingId = ""
    /// `nil` — версии Final нет; тогда сегментов и статистики не бывает.
    private var version: UInt32?

    init(core: any SpeakerAttributionCoreProviding) {
        self.core = core
    }

    /// Сегменты есть только у версий, собранных повторным распознаванием
    /// (ADR-011). У старых Final их нет — это не ошибка.
    var hasSegments: Bool {
        !segments.isEmpty
    }

    var canAssign: Bool {
        version != nil && !segments.isEmpty
    }

    func load(meetingId: String, version: UInt32?) {
        self.meetingId = meetingId
        self.version = version
        reload()
    }

    func addSpeaker(primaryLanguage: String) {
        let number = speakers.count + 1
        let displayName = primaryLanguage == "ru"
            ? "Спикер \(number)" // loc:allow — имя следует языку встречи, не интерфейса
            : "Speaker \(number)"
        let error = core.upsertSpeaker(
            meetingId: meetingId,
            id: "",
            displayName: displayName,
            sortIndex: Int64(speakers.count)
        )
        finish(error: error)
    }

    func rename(id: String, displayName: String) {
        guard let speaker = speakers.first(where: { $0.id == id }) else { return }
        let trimmed = displayName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed != speaker.displayName else { return }
        let error = core.upsertSpeaker(
            meetingId: meetingId,
            id: id,
            displayName: trimmed,
            sortIndex: speaker.sortIndex
        )
        finish(error: error)
    }

    /// Удаление снимает привязку: висячий идентификатор выглядел бы как
    /// «атрибуция не сработала», а не как «участник удалён».
    func remove(id: String) {
        finish(error: core.deleteSpeaker(meetingId: meetingId, id: id))
    }

    /// Назначить участника всему каналу — основная операция для звонка
    /// один на один.
    func assignChannel(_ channelCode: String, to speakerId: String) {
        guard let version else { return }
        let error = core.assignChannelSpeaker(
            meetingId: meetingId,
            version: version,
            channelCode: channelCode,
            speakerId: speakerId
        )
        finish(error: error)
    }

    /// Переназначить одну реплику; она перестаёт подчиняться каналу.
    func assignSegment(index: UInt32, to speakerId: String) {
        guard let version else { return }
        let error = core.assignSegmentSpeaker(
            meetingId: meetingId,
            version: version,
            index: index,
            speakerId: speakerId
        )
        finish(error: error)
    }

    func unpinSegment(index: UInt32) {
        guard let version else { return }
        finish(error: core.unpinSegmentSpeaker(
            meetingId: meetingId,
            version: version,
            index: index
        ))
    }

    /// Кто сейчас закреплён за каналом — большинством непоправленных
    /// реплик. Точечные правки на подпись канала не влияют: иначе одна
    /// исправленная реплика переписывала бы заголовок всего канала.
    func channelSpeakerName(_ channelCode: String) -> String {
        var counts: [String: Int] = [:]
        for segment in segments
            where segment.channel == channelCode
            && segment.source == .channel
            && !segment.speakerId.isEmpty
        {
            counts[segment.speakerId, default: 0] += 1
        }
        guard let winner = counts.max(by: { left, right in
            left.value == right.value ? left.key > right.key : left.value < right.value
        }) else {
            return ""
        }
        return speakers.first(where: { $0.id == winner.key })?.displayName ?? ""
    }

    /// Реплики, до которых имя не дотянулось.
    ///
    /// Не «голоса, которых не узнали»: голосов как отдельных сущностей в
    /// схеме со слепками нет вовсе (ADR-013). Есть люди со слепками и
    /// реплики, оставшиеся без подписи, — и это законный исход, а не сбой.
    var unidentifiedSegments: [FfiFinalSegment] {
        segments.filter(\.speakerId.isEmpty)
    }

    /// Сколько реплик подписал человек. Ровно из них складываются слепки.
    var humanLabelledCount: Int {
        segments.filter { $0.source == .human && !$0.speakerId.isEmpty }.count
    }

    /// Есть ли чем пересчитывать.
    ///
    /// Без единой ручной подписи слепок сложить не из чего, и пересчёт
    /// вернул бы ноль. Кнопка в этом случае не нажимается, а рядом с ней
    /// сказано, почему: «не работает молча» здесь означает «не работает
    /// заметно».
    var canRecomputeVoicePrints: Bool {
        voiceEngineAvailable && version != nil && !segments.isEmpty && humanLabelledCount > 0
    }

    /// Сменилась ли модель под уже сложенными слепками.
    ///
    /// Векторы разных моделей несравнимы. Слепки при этом не выбрасываются
    /// и не используются: человеку говорится пересчитать.
    var voicePrintsNeedRecompute: Bool {
        voicePrints.contains { !$0.modelMatches }
    }

    /// Пересчитать слепки и разнести неподписанные реплики.
    func recomputeVoicePrints() {
        guard let version else { return }
        let pass = core.recomputeVoiceprints(
            meetingId: meetingId,
            version: version,
            accept: core.voiceprintDefaultAccept(),
            margin: core.voiceprintDefaultMargin()
        )
        lastPass = pass
        // Отказ показывается как ошибка **и** остаётся в `lastPass`: без
        // первого он потерялся бы среди чисел, без второго исчез бы при
        // следующем действии.
        finish(error: pass.error)
    }

    /// Можно ли запомнить голос этого участника между встречами.
    ///
    /// Три условия сразу, и каждое — отказ по своей причине: память
    /// выключена, слепка нет, имени нет. Кнопка без любого из них
    /// показывала бы действие, которое не сработает.
    func canRememberVoice(speakerId: String) -> Bool {
        guard voiceEngineAvailable, voiceMemoryEnabled else { return false }
        guard voicePrints.contains(where: { $0.speakerId == speakerId }) else { return false }
        return speakers
            .first(where: { $0.id == speakerId })
            .map { !$0.displayName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
            ?? false
    }

    /// Запомнить голос участника между встречами.
    func rememberVoice(speakerId: String) {
        finish(error: core.rememberVoice(meetingId: meetingId, speakerId: speakerId))
    }

    func dismissError() {
        errorMessage = nil
    }

    /// Записи за диапазон реплики не оказалось.
    ///
    /// Говорится вслух: у встречи запись есть, кнопка была показана, а
    /// звука за этот кусок нет. Молча погасить кнопку значило бы оставить
    /// человека гадать, нажал он или нет.
    func reportMissingFragment() {
        errorMessage = String(
            localized: "There is no recording for this fragment — nothing to play."
        )
    }

    /// Предлагать ли «заменять всюду» для этой реплики.
    ///
    /// Решает ядро: непустой `promotableTermId` означает, что подсказка
    /// родилась из правки, действует в этой встрече и ещё не стала
    /// заменой. Повторять этот разбор в Swift нельзя (`AGENTS.md`).
    func canPromote(index: UInt32) -> Bool {
        guard let segment = segments.first(where: { $0.index == index }) else { return false }
        return !segment.promotableTermId.isEmpty
    }

    func beginEdit(index: UInt32) {
        guard let segment = segments.first(where: { $0.index == index }) else { return }
        editingIndex = index
        draftText = segment.text
    }

    /// Esc: ядро не трогаем — от правки отказались.
    func cancelEdit() {
        editingIndex = nil
        draftText = ""
    }

    /// Enter или потеря фокуса.
    ///
    /// Состояние сбрасывается до вызова ядра: `finish` перечитывает
    /// сегменты, и оставленный индекс открыл бы поле заново поверх уже
    /// сохранённого текста.
    func commitEdit() {
        guard let index = editingIndex, let version else {
            cancelEdit()
            return
        }
        let text = draftText
        editingIndex = nil
        draftText = ""
        finish(error: core.editSegmentText(
            meetingId: meetingId,
            version: version,
            index: index,
            text: text
        ))
    }

    /// Вернуть распознанное. Это отмена, а не ещё одна правка: получив
    /// исходный текст обратно, ядро удаляет запись из журнала.
    func revertToOriginal(index: UInt32) {
        guard let version,
              let segment = segments.first(where: { $0.index == index }),
              !segment.originalText.isEmpty
        else { return }
        finish(error: core.editSegmentText(
            meetingId: meetingId,
            version: version,
            index: index,
            text: segment.originalText
        ))
    }

    func promoteTerm(index: UInt32) {
        guard let version,
              let segment = segments.first(where: { $0.index == index }),
              !segment.promotableTermId.isEmpty
        else { return }
        finish(error: core.promoteTermToReplacement(
            termId: segment.promotableTermId,
            meetingId: meetingId,
            version: version
        ))
    }

    func dismissUnapplied(id: String) {
        finish(error: core.deleteSegmentEdit(editId: id))
    }

    /// Звук реплики. Пустой фрагмент (`sampleRate == 0`) означает, что
    /// записи за диапазон нет, — вью на это прячет кнопку.
    func audioFragment(for segment: FfiFinalSegment) -> FfiAudioFragment {
        core.segmentAudio(
            meetingId: meetingId,
            channelCode: segment.channel,
            startMs: segment.startMs,
            endMs: segment.endMs
        )
    }

    /// Звук неприменившейся правки: сегмента у неё нет, а место есть.
    func audioFragment(channelCode: String, startMs: UInt64, endMs: UInt64) -> FfiAudioFragment {
        core.segmentAudio(
            meetingId: meetingId,
            channelCode: channelCode,
            startMs: startMs,
            endMs: endMs
        )
    }

    private func reload() {
        speakers = core.listSpeakers(meetingId: meetingId)
        audioAvailable = core.meetingAudioBytes(meetingId: meetingId) > 0
        voicePrints = core.listVoiceprints(meetingId: meetingId)
        voiceEngineAvailable = core.isVoiceEngineAvailable()
        voiceMemoryEnabled = core.isVoiceMemoryEnabled()
        // Именно здесь, а не под `guard let version`: правка без версии —
        // как раз та, которую надо показать.
        unappliedEdits = core.listUnappliedEdits(meetingId: meetingId)
        guard let version else {
            segments = []
            rows = Self.rows(speakers: speakers, stats: [])
            return
        }
        segments = core.listFinalSegments(meetingId: meetingId, version: version)
        let stats = core.listSpeakerStats(meetingId: meetingId, version: version)
        rows = Self.rows(speakers: speakers, stats: stats)
    }

    private func finish(error: String) {
        guard error.isEmpty else {
            errorMessage = error
            return
        }
        errorMessage = nil
        reload()
    }

    /// Свести сводку и список участников в строки экрана.
    ///
    /// Участник без реплик всё равно показывается: он заведён вручную, и
    /// молча пропасть из списка после добавления он не должен.
    static func rows(speakers: [FfiSpeaker], stats: [FfiSpeakerStat]) -> [SpeakerRowModel] {
        var result = stats.map { stat in
            SpeakerRowModel(
                id: stat.speakerId,
                displayName: speakers
                    .first(where: { $0.id == stat.speakerId })?
                    .displayName ?? stat.displayName,
                channelCode: stat.channel,
                segmentCount: stat.segmentCount,
                speakingMs: stat.speakingMs,
                share: stat.share
            )
        }
        let counted = Set(result.map(\.id))
        for speaker in speakers where !counted.contains(speaker.id) {
            result.append(
                SpeakerRowModel(
                    id: speaker.id,
                    displayName: speaker.displayName,
                    channelCode: "",
                    segmentCount: 0,
                    speakingMs: 0,
                    share: 0
                )
            )
        }
        return result
    }
}

/// Форматирование данных атрибуции для показа.
enum SpeakerFormat {
    static func channelLabel(_ code: String) -> String {
        switch code {
        case "mic": "Mic"
        case "system": "System"
        default: ""
        }
    }

    /// Доля речи в процентах. Ненулевая доля никогда не показывается как
    /// `0%`: участник, сказавший одну фразу, всё-таки говорил.
    static func shareText(_ share: Double) -> String {
        guard share > 0 else { return "0%" }
        let percent = Int((share * 100).rounded())
        return "\(max(percent, 1))%"
    }

    /// Длительность как `m:ss`, от часа — `h:mm:ss`.
    static func durationText(ms: UInt64) -> String {
        let totalSeconds = ms / 1000
        let seconds = totalSeconds % 60
        let minutes = (totalSeconds / 60) % 60
        let hours = totalSeconds / 3600
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%d:%02d", minutes, seconds)
    }

    /// Тайм-код реплики: тот же формат, но с ведущим нулём у минут —
    /// в столбце он должен быть одной ширины.
    static func timecode(ms: UInt64) -> String {
        let totalSeconds = ms / 1000
        let seconds = totalSeconds % 60
        let minutes = (totalSeconds / 60) % 60
        let hours = totalSeconds / 3600
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%02d:%02d", minutes, seconds)
    }

    static func segmentCountText(_ count: UInt32) -> String {
        String(localized: "\(count) lines")
    }

    /// Из чего сложен слепок: сколько **ваших подписей** в нём и сколько
    /// в них секунд.
    ///
    /// Слово выбрано после того, как прежнее сбило с толку. Рядом в той же
    /// строке стоит «N репл.» — число реплик, которые участнику
    /// **достались**, и оно растёт после пересчёта. Два счётчика с одной
    /// подписью «репл.» читались как одно и то же число, и совпадение
    /// первого с числом своих подписей выглядело как «пересчёт ничего не
    /// сделал».
    ///
    /// Секунды показываются целыми и без округления вверх: слепок на 4.6 с
    /// это «4 с», а не «5 с». Материала в нём столько, сколько есть, и
    /// приписывать ему лишнюю секунду незачем — как раз по таким крохам
    /// на замере и вышли самые тонкие места.
    static func voicePrintText(_ print: FfiVoicePrint) -> String {
        String(localized: "print: \(print.samples) labels, \(Int(print.seconds)) s")
    }

    /// Из чего сложен запомненный голос — тем же языком, что и слепок
    /// встречи: человеку это одна и та же величина, и разные подписи под
    /// ней он читал бы как разные вещи.
    static func knownVoiceText(_ voice: FfiKnownVoice) -> String {
        String(localized: "\(voice.samples) labels, \(Int(voice.seconds)) s")
    }

    /// Итог пересчёта числами.
    ///
    /// Неопознанные названы **отдельно** от «без звука»: первое — ответ
    /// модели, второе — её молчание. Сложив их в одно число, мы выдали бы
    /// нехватку материала за несходство голосов, и человек искал бы
    /// причину не там.
    static func passSummary(_ pass: FfiVoicePrintPass) -> String {
        var parts = [String(localized: "labelled \(pass.signed)")]
        if pass.cleared > 0 {
            parts.append(String(localized: "cleared \(pass.cleared)"))
        }
        // «Не узнано», а не «без имени»: реплика, которую слепок не
        // опознал, сохраняет подпись по каналу и на экране имя имеет.
        // Пока оба числа звались «без имени», отчёт спорил с тем, что
        // человек видит в списке.
        parts.append(String(localized: "unrecognised \(pass.unknown)"))
        if pass.withoutVector > 0 {
            parts.append(String(localized: "no audio \(pass.withoutVector)"))
        }
        if pass.signedFromMemory > 0 {
            // Отдельной строкой, а не внутри «подписано»: человек включил
            // память на голоса осознанно и вправе видеть, сколько она
            // сделала. Слитое в общую сумму, это исчезло бы у функции,
            // которая требует доверия больше прочих.
            parts.append(String(localized: "recognised from memory \(pass.signedFromMemory)"))
        }
        parts.append(String(localized: "prints \(pass.prints)"))
        return parts.joined(separator: " · ")
    }

    /// Инициал для аватара; пусто — вопрос, а не пустой круг.
    static func initial(_ name: String) -> String {
        guard let first = name.trimmingCharacters(in: .whitespacesAndNewlines).first else {
            return "?"
        }
        return String(first).uppercased()
    }
}
