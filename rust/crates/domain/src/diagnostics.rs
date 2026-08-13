//! Записи диагностики распознавания.
//!
//! Нужны потому, что отсев галлюцинаций (Epic 16) работает **молча**: он
//! выбрасывает текст, и если под нож попала настоящая речь, узнать об
//! этом неоткуда. Фильтр без журнала — это доверие без проверки.

/// Что именно произошло с текстом на пути к субтитрам.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttDiagnosticKind {
    /// Текст признан галлюцинацией и выброшен.
    DroppedHallucination,
    /// Начало похоже на известную фразу; текст придержан до следующей порции.
    HeldPrefix,
    /// Придержанное оказалось речью и отдано в конце реплики.
    ReleasedHeld,
    /// Сегмент отброшен по вероятности «здесь нет речи».
    DroppedNoSpeech,
    /// Реплика не набрала порога речи — модель по ней не гонялась.
    ///
    /// Гейт пропустил всплеск, но речи в нём меньше `MIN_SPEECH_FRAMES`,
    /// и partial по такому куску движок не гонял никогда. Раньше конец
    /// такой «реплики» всё равно стоил полного прохода: замер 2026-08-11
    /// дал 22 таких прохода из 48 на двух минутах молчащей комнаты.
    ///
    /// Запись в журнале обязательна: вместе с проходом пропадает и
    /// возможность распознать очень короткое «да». Потеря должна быть
    /// видимой — молчаливая здесь хуже.
    SkippedShortSegment,
    /// Канал захвата привязан к общему времени записи.
    ///
    /// `text` — код канала, `buffer_ms` — насколько его первый буфер
    /// позже начала записи.
    CaptureChannelStart,
    /// Разница стартов двух каналов.
    ///
    /// `text` — какой канал начался позже, `buffer_ms` — на сколько.
    /// Пишется обязательно: молча эта разница уже раз стоила недели
    /// разбора. На встрече `6CE19EC5` каналы разошлись на 1150 мс, оба
    /// пометили своё начало нулём, и любое их сопоставление было смещено
    /// — включая приборы, которые это должны были поймать.
    CaptureChannelSkew,
}

impl SttDiagnosticKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::DroppedHallucination => "dropped_hallucination",
            Self::HeldPrefix => "held_prefix",
            Self::ReleasedHeld => "released_held",
            Self::DroppedNoSpeech => "dropped_no_speech",
            Self::SkippedShortSegment => "skipped_short_segment",
            Self::CaptureChannelStart => "capture_channel_start",
            Self::CaptureChannelSkew => "capture_channel_skew",
        }
    }
}

/// Одно решение движка, о котором стоит знать при разборе жалобы.
#[derive(Debug, Clone, PartialEq)]
pub struct SttDiagnostic {
    pub kind: SttDiagnosticKind,
    /// Текст, которого коснулось решение. Может быть пустым.
    pub text: String,
    /// Длина буфера в миллисекундах на момент решения.
    pub buffer_ms: u64,
}

impl SttDiagnostic {
    pub fn new(kind: SttDiagnosticKind, text: impl Into<String>, buffer_ms: u64) -> Self {
        Self {
            kind,
            text: text.into(),
            buffer_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct() {
        let codes = [
            SttDiagnosticKind::DroppedHallucination.code(),
            SttDiagnosticKind::HeldPrefix.code(),
            SttDiagnosticKind::ReleasedHeld.code(),
            SttDiagnosticKind::DroppedNoSpeech.code(),
            SttDiagnosticKind::SkippedShortSegment.code(),
            SttDiagnosticKind::CaptureChannelStart.code(),
            SttDiagnosticKind::CaptureChannelSkew.code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len());
    }
}
