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
}

impl SttDiagnosticKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::DroppedHallucination => "dropped_hallucination",
            Self::HeldPrefix => "held_prefix",
            Self::ReleasedHeld => "released_held",
            Self::DroppedNoSpeech => "dropped_no_speech",
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
        ];
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len());
    }
}
