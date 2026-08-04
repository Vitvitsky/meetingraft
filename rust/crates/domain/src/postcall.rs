//! Post-call артефакты (отдельно от live caption — ADR-002).

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
}

impl FinalSegment {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
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

#[cfg(test)]
mod tests {
    use super::ArtifactKind;

    #[test]
    fn artifact_kind_brief_distinct_from_follow_up() {
        assert_ne!(ArtifactKind::Brief, ArtifactKind::FollowUp);
    }
}
