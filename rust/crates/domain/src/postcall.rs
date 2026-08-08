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
    /// Спикер поставлен человеком вручную.
    ///
    /// Массовое назначение по каналу такие сегменты не трогает: иначе
    /// исправление одной реплики терялось бы при следующем назначении, и
    /// заметить это было бы нечем.
    pub speaker_pinned: bool,
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
pub type EditPosition = (AudioChannel, u64, u64);

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
    fn precedence(&self) -> (u64, &str) {
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
    use super::{ArtifactKind, SegmentEdit, body_fingerprint, edits_by_position};
    use crate::AudioChannel;

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

    /// Две правки на одном месте: побеждает последнее решение человека,
    /// а не порядок выборки из базы.
    #[test]
    fn latest_by_created_at_wins_the_position() {
        let edits = vec![edit("e1", Some(1), 200), edit("e2", Some(1), 100)];

        let by_position = edits_by_position(&edits, 1);

        assert_eq!(by_position.values().next().expect("правка").id, "e1");
    }
}
