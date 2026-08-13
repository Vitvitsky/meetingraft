//! Post-call артефакты (отдельно от live caption — ADR-002).

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::AudioChannel;

/// Вид post-call артефакта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    Brief,
    FollowUp,
}

/// Финальный транскрипт встречи (refined, post-call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalTranscript {
    pub meeting_id: String,
    pub version: u32,
    pub body_markdown: String,
    pub created_at_ms: u64,
}

/// Post-call артефакт (brief, follow-up и т.д.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub id: String,
    pub meeting_id: String,
    pub kind: ArtifactKind,
    pub template_id: String,
    pub body_markdown: String,
    pub created_at_ms: u64,
    /// Версия Final, из которой артефакт собран.
    ///
    /// `None` — артефакт из базы, заведённой до отслеживания источника.
    /// Это не то же, что «устарел»: про такой ничего не известно.
    pub source_version: Option<u32>,
    /// Отпечаток тела Final на момент сборки ([`body_fingerprint`]).
    ///
    /// Версии мало: правка сегмента и переименование спикера переписывают
    /// текст на месте, номер версии при этом не меняя.
    pub source_fingerprint: Option<String>,
}

/// Краткая сводка встречи для списка/истории.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingSummary {
    pub id: String,
    /// Пустое название допустимо: подстановку делает презентационный слой.
    pub title: String,
    pub started_at_ms: u64,
    /// `None`, пока встреча не завершена.
    pub ended_at_ms: Option<u64>,
    pub has_final: bool,
    pub artifact_count: u64,
    /// Когда у встречи удалили запись. `None` — не удаляли (Epic 22).
    pub audio_deleted_at_ms: Option<u64>,
}

impl MeetingSummary {
    /// Длительность встречи, если она завершена.
    pub fn duration_ms(&self) -> Option<u64> {
        self.ended_at_ms
            .map(|ended| ended.saturating_sub(self.started_at_ms))
    }
}

/// Сегмент распознанного текста: результат работы движка, ещё без
/// порядкового номера, канала и спикера.
///
/// Живёт в домене, а не в `stt`: это общий словарь между движком, который
/// его производит, и post-call сборкой, которая его потребляет. Иначе
/// одному крейту пришлось бы зависеть от другого без необходимости.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl TranscriptSegment {
    pub fn new(start_ms: u64, end_ms: u64, text: impl Into<String>) -> Self {
        Self {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }
}

/// Откуда у реплики взялось имя говорящего (ADR-013).
///
/// Порядок здесь не декоративный: человек сильнее канала, канал сильнее
/// слепка. Каждый шаг подписи обязан спрашивать источник **до** того, как
/// писать своё имя, иначе автоматика молча затирает ручную работу — ровно
/// то, ради чего в Phase 11 заводился `speaker_pinned`.
///
/// Отдельным значением стоит и отсутствие подписи. Неопознанная реплика —
/// законный исход, а не отсутствие данных: при слепках ошибка идёт именно
/// в отказ, и отличать «никто не подписал» от «подписал канал» нужно и
/// коду, и человеку.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeakerSource {
    /// Никто: реплика неопознана.
    #[default]
    None,
    /// Канал захвата (ADR-012).
    Channel,
    /// Человек назначил вручную.
    Human,
    /// Слепок голоса (ADR-013).
    VoicePrint,
}

impl SpeakerSource {
    /// Код для хранения и границы UniFFI.
    pub fn code(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Channel => "channel",
            Self::Human => "human",
            Self::VoicePrint => "voiceprint",
        }
    }

    /// Разбор кода; неизвестное значение считается отсутствием подписи.
    ///
    /// Сюда же попадает пустая строка. Выбор в сторону «никто» осознан:
    /// незнакомый код от будущей версии лучше показать неопознанным, чем
    /// выдать за ручную работу и тем защитить от перезаписи.
    pub fn from_code(code: &str) -> Self {
        match code {
            "channel" => Self::Channel,
            "human" => Self::Human,
            "voiceprint" => Self::VoicePrint,
            _ => Self::None,
        }
    }

    /// Вправе ли `writer` переписать подпись этого источника.
    ///
    /// Единственное место, где записан порядок человек → канал → слепок.
    /// Разложенное по вызовам сравнение разъехалось бы: в ядре три пути
    /// подписи, и каждый решал бы заново.
    pub fn may_overwrite(self, writer: SpeakerSource) -> bool {
        match self {
            // Неопознанное берёт кто угодно, в том числе повторно.
            Self::None => true,
            // Ручное не трогает ничто, кроме руки.
            Self::Human => writer == Self::Human,
            // Канал перебивается рукой и пересчётом самого канала, но не
            // слепком: слепок уточняет там, где канал промолчал.
            Self::Channel => writer != Self::VoicePrint,
            // Слепок уступает всем, включая следующий пересчёт слепков.
            Self::VoicePrint => true,
        }
    }
}

/// Сегмент финального транскрипта: текст с положением во времени и
/// каналом. В отличие от live, канал здесь известен точно — post-call
/// распознаёт дорожки раздельно (ADR-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalSegment {
    /// Порядковый номер внутри версии.
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub channel: AudioChannel,
    /// Пусто, пока спикер не назначен.
    pub speaker_id: String,
    /// Откуда взялась подпись (ADR-013).
    ///
    /// Раньше здесь стоял булев `speaker_pinned` — «поставлено рукой», — и
    /// его хватало ровно на два источника. Со слепками их три, и порядок
    /// между ними жёсткий; булевым он не выражается.
    pub speaker_source: SpeakerSource,
    pub text: String,
    /// Текст заменён ручной правкой из журнала (Epic 19).
    ///
    /// Не хранится в таблице сегментов — вычисляется при чтении, потому
    /// что источником истины остаётся журнал.
    pub text_edited: bool,
    /// Что распознала модель на этом месте (Epic 19).
    ///
    /// Берётся из журнала правок, а не из таблицы сегментов: правка
    /// ищет своё место вхождением (`reattach_edits`), поэтому после
    /// пересбора текст сегмента бывает длиннее сохранённого в правке.
    /// Возврат к исходному сравнивается именно с журналом, и подмена
    /// источника завела бы новую правку вместо удаления старой.
    ///
    /// Пусто, когда правки нет.
    pub original_text: String,
}

impl FinalSegment {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    /// Место сегмента в записи: канал и границы во времени.
    pub fn position(&self) -> EditPosition {
        (self.channel, self.start_ms, self.end_ms)
    }
}

/// Место реплики: канал и границы во времени.
///
/// Правка привязана к месту, а не к порядковому номеру: пересбор режет
/// запись заново и номера меняются, а место остаётся тем же.
///
/// В базе этот ключ ничем не закреплён — ни уникальным индексом, ни
/// внешним ключом, и это осознанно (комментарий к шагу 8 в
/// `storage::migrations` объясняет, почему иначе нельзя). Значит,
/// согласованность держится только на том, что все считают место через
/// [`edits_by_position`], а не заводят свой расчёт.
pub type EditPosition = (AudioChannel, u64, u64);

/// Откуда взялась правка в журнале.
///
/// Различие не косметическое: правка, набранная человеком, — последнее
/// слово по этому месту, и переписывать её нельзя ничем. Замена всюду
/// (`occurrences_to_edit`) — производная от термина, и её собственный
/// результат следующая замена вправе пересчитать.
///
/// Пока признака не было, обе выглядели одинаково: одно нажатие
/// «заменять всюду» навсегда закрывало свои позиции от будущих замен, и
/// человек об этом не узнавал.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOrigin {
    /// Набрана человеком в поле правки.
    Human,
    /// Поставлена массовой заменой по термину.
    Bulk,
}

impl EditOrigin {
    pub fn code(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Bulk => "bulk",
        }
    }

    /// Неизвестный код читается как ручная правка.
    ///
    /// Ошибиться можно в две стороны, и они не равны: принять ручную за
    /// массовую значит позволить её переписать молча, принять массовую
    /// за ручную — всего лишь не тронуть лишнего.
    pub fn from_code(code: &str) -> Self {
        match code {
            "bulk" => Self::Bulk,
            _ => Self::Human,
        }
    }
}

/// Ручная правка текста сегмента.
///
/// Живёт отдельно от сегментов: сегменты производны от распознавания, а
/// пересбор создаёт новую версию с другой нарезкой. Журнал переживает
/// пересбор, таблица сегментов — нет (Epic 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentEdit {
    pub id: String,
    pub meeting_id: String,
    pub channel: AudioChannel,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Что распознала модель.
    pub original_text: String,
    /// Что ввёл человек.
    pub edited_text: String,
    pub created_at_ms: u64,
    /// Версия, в которой правка сейчас применена. `None` — не применилась.
    pub applied_version: Option<u32>,
    /// Кто её поставил: человек или замена всюду.
    pub origin: EditOrigin,
}

impl SegmentEdit {
    /// Место, к которому привязана правка.
    pub fn position(&self) -> EditPosition {
        (self.channel, self.start_ms, self.end_ms)
    }

    /// Кто побеждает при коллизии двух правок на одном месте.
    ///
    /// Позднейшая по `created_at_ms` — это последнее решение человека по
    /// этому месту. При равном времени берётся больший `id`: нужен хоть
    /// какой-то детерминизм, а порядок выборки из базы к давности правки
    /// отношения не имеет.
    ///
    /// Наружу это правило выведено ради переноса правок
    /// (`postcall::reattach_edits`): он отвязывает проигравших, чтобы те
    /// не пропадали из виду, и обязан назвать победителем ту же правку,
    /// которую покажет [`edits_by_position`] при чтении сегментов.
    /// Разъедься эти два выбора — в сегментах стояла бы одна правка, а
    /// отвязана была бы другая, и обе исчезли бы из виду разом.
    pub fn precedence(&self) -> (u64, &str) {
        (self.created_at_ms, self.id.as_str())
    }
}

/// Разложить журнал по местам одной версии.
///
/// Единственное правило сопоставления правки с местом на все три случая,
/// где оно нужно: чтение сегментов, ручная правка текста и массовая
/// замена по термину. Раньше каждое место решало по-своему, и правка
/// первой версии молча перехватывалась правкой того же места во второй —
/// исходный текст при этом оставался от первой версии, и «вернуть
/// исходное» становилось недостижимым.
///
/// Фильтр по версии обязателен: пересбор при неизменной модели даёт ту же
/// нарезку, поэтому совпадение координат между версиями — норма, а не
/// редкость.
///
/// На одно место может прийтись несколько правок — пересбор пересаживает
/// журнал на новую нарезку по перекрытию времени, и слияние двух ранее
/// правленых сегментов в один даёт им общий ключ. Побеждает та, что
/// сильнее по [`SegmentEdit::precedence`].
pub fn edits_by_position(
    edits: &[SegmentEdit],
    version: u32,
) -> HashMap<EditPosition, &SegmentEdit> {
    let mut by_position: HashMap<EditPosition, &SegmentEdit> = HashMap::new();
    for edit in edits {
        if edit.applied_version != Some(version) {
            continue;
        }
        match by_position.entry(edit.position()) {
            Entry::Vacant(slot) => {
                slot.insert(edit);
            }
            Entry::Occupied(mut slot) => {
                if edit.precedence() > slot.get().precedence() {
                    slot.insert(edit);
                }
            }
        }
    }
    by_position
}

/// Где нашлось совпадение полнотекстового поиска.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchHitKind {
    Caption,
    Final,
    Artifact,
}

impl SearchHitKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Caption => "caption",
            Self::Final => "final",
            Self::Artifact => "artifact",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "final" => Self::Final,
            "artifact" => Self::Artifact,
            _ => Self::Caption,
        }
    }
}

/// Одно совпадение поиска по материалам встреч.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub meeting_id: String,
    pub kind: SearchHitKind,
    /// Идентификатор исходной записи: id caption/артефакта или номер версии.
    pub ref_id: String,
    /// Фрагмент с подсветкой найденного.
    pub snippet: String,
}

/// Отпечаток текста, из которого собран артефакт.
///
/// FNV-1a 64 бита. `DefaultHasher` из std для этого не годится: он не
/// обещает одинакового значения между версиями Rust, а отпечаток ложится
/// в базу и обязан пережить обновление приложения.
///
/// Криптостойкость не нужна — текст сравнивается сам с собой, злого
/// умысла в сценарии нет.
pub fn body_fingerprint(body: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in body.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactKind, EditOrigin, FinalSegment, SegmentEdit, SpeakerSource, body_fingerprint,
        edits_by_position,
    };
    use crate::AudioChannel;

    /// Ради этого признак и заводился: автоматика не смеет трогать
    /// поставленное рукой. До ADR-013 это держал `speaker_pinned`, и
    /// потерять правило при замене булева на перечисление было бы
    /// откатом Phase 11.
    #[test]
    fn nothing_but_a_human_overwrites_a_human_label() {
        assert!(!SpeakerSource::Human.may_overwrite(SpeakerSource::Channel));
        assert!(!SpeakerSource::Human.may_overwrite(SpeakerSource::VoicePrint));
        assert!(SpeakerSource::Human.may_overwrite(SpeakerSource::Human));
    }

    /// Слепок уточняет там, где канал промолчал, и только там. Иначе на
    /// звонке один на один — случае, где канал точен абсолютно
    /// (ADR-012), — модель переписывала бы верное на вероятное.
    #[test]
    fn a_voiceprint_fills_the_gaps_but_does_not_argue_with_the_channel() {
        assert!(SpeakerSource::None.may_overwrite(SpeakerSource::VoicePrint));
        assert!(!SpeakerSource::Channel.may_overwrite(SpeakerSource::VoicePrint));
    }

    /// Пересчёт обязан переписывать собственную прошлую работу: иначе
    /// первый прогон с плохим слепком остался бы навсегда, и кнопка
    /// «пересчитать» ничего бы не пересчитывала.
    #[test]
    fn each_source_may_redo_its_own_work() {
        for source in [
            SpeakerSource::None,
            SpeakerSource::Channel,
            SpeakerSource::Human,
            SpeakerSource::VoicePrint,
        ] {
            assert!(
                source.may_overwrite(source),
                "источник {source:?} обязан переписывать сам себя"
            );
        }
    }

    /// Незнакомый код — от базы, тронутой более новой версией. Считать
    /// его ручной подписью значило бы защитить от перезаписи то, о чём
    /// мы ничего не знаем.
    #[test]
    fn an_unknown_code_reads_as_no_label() {
        assert_eq!(SpeakerSource::from_code("nonsense"), SpeakerSource::None);
        assert_eq!(SpeakerSource::from_code(""), SpeakerSource::None);
        for source in [
            SpeakerSource::None,
            SpeakerSource::Channel,
            SpeakerSource::Human,
            SpeakerSource::VoicePrint,
        ] {
            assert_eq!(SpeakerSource::from_code(source.code()), source);
        }
    }

    #[test]
    fn fingerprint_is_stable_for_the_same_text() {
        assert_eq!(
            body_fingerprint("Обсудили границу FFI."),
            body_fingerprint("Обсудили границу FFI.")
        );
    }

    #[test]
    fn fingerprint_changes_with_a_single_character() {
        assert_ne!(body_fingerprint("Пётр: да"), body_fingerprint("Пётр: нет"));
    }

    /// Значение прибито константой: отпечатки лежат в базах пользователей,
    /// и смена алгоритма обязана быть решением, а не побочным эффектом.
    #[test]
    fn fingerprint_value_is_pinned() {
        assert_eq!(body_fingerprint(""), "cbf29ce484222325");
        assert_eq!(body_fingerprint("MeetingRaft"), "888faeb606a56a89");
    }

    #[test]
    fn artifact_kind_brief_distinct_from_follow_up() {
        assert_ne!(ArtifactKind::Brief, ArtifactKind::FollowUp);
    }

    fn edit(id: &str, version: Option<u32>, created_at_ms: u64) -> SegmentEdit {
        SegmentEdit {
            id: id.into(),
            meeting_id: "m1".into(),
            channel: AudioChannel::Mic,
            start_ms: 1000,
            end_ms: 2000,
            original_text: "интра ру".into(),
            edited_text: id.into(),
            created_at_ms,
            applied_version: version,
            origin: EditOrigin::Human,
        }
    }

    /// Пересбор при неизменной модели даёт ту же нарезку, поэтому одно и
    /// то же место в двух версиях — обычное дело. Без фильтра по версии
    /// правка второй версии перехватывала бы место первой.
    #[test]
    fn takes_only_edits_of_the_asked_version() {
        let edits = vec![edit("e1", Some(1), 10), edit("e2", Some(2), 20)];

        let first = edits_by_position(&edits, 1);
        let second = edits_by_position(&edits, 2);

        assert_eq!(first.len(), 1);
        assert_eq!(first.values().next().expect("правка").id, "e1");
        assert_eq!(second.values().next().expect("правка").id, "e2");
    }

    #[test]
    fn skips_unapplied_edits() {
        let edits = vec![edit("e1", None, 10)];

        assert!(edits_by_position(&edits, 1).is_empty());
    }

    /// Сегмент и правка на одних координатах дают одно место.
    ///
    /// Ключ места — договорённость трёх слоёв, а не ограничение схемы:
    /// уникального индекса на `segment_edits` нет и быть не может
    /// (см. комментарий к шагу 8 в `storage::migrations`). Держится он на
    /// том, что обе `position()` считают одно и то же, а impl'ы у них
    /// разные. Разъедутся — правки перестанут находить свои сегменты
    /// молча: не ошибка, а пустой результат.
    #[test]
    fn segment_and_edit_agree_on_the_position() {
        let segment = FinalSegment {
            index: 7,
            start_ms: 1000,
            end_ms: 2000,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_source: SpeakerSource::None,
            text: "интра ру".into(),
            text_edited: false,
            original_text: String::new(),
        };
        let edit = edit("e1", Some(1), 10);

        // Проверять равенство мест имеет смысл, только если координаты
        // и правда совпадают: разойдись они, тест сравнивал бы мимо.
        assert_eq!(
            (segment.channel, segment.start_ms, segment.end_ms),
            (edit.channel, edit.start_ms, edit.end_ms),
            "данные теста разъехались по координатам"
        );
        assert_eq!(segment.position(), edit.position());
    }

    /// Две правки на одном месте: побеждает последнее решение человека,
    /// а не порядок выборки из базы.
    #[test]
    fn latest_by_created_at_wins_the_position() {
        let edits = vec![edit("e1", Some(1), 200), edit("e2", Some(1), 100)];

        let by_position = edits_by_position(&edits, 1);

        assert_eq!(by_position.values().next().expect("правка").id, "e1");
    }
}
