//! Термины глоссария и область действия (Phase 5).

use crate::SpeechLanguage;

/// Область действия термина глоссария.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossaryScope {
    Global,
    Meeting { meeting_id: String },
}

/// Что термин делает с текстом.
///
/// Разделение вынужденное: `normalize_caption` заменяет безусловно и
/// везде, поэтому термин, родившийся из грамматической правки, переписывал
/// бы все будущие тексты. Подсказка такого сделать не может — цена ошибки
/// в `initial_prompt` мизерная и обратимая.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlossaryKind {
    /// Только подсказка Whisper.
    Hint,
    /// Замена surface → canonical в готовом тексте.
    Replacement,
}

impl GlossaryKind {
    pub fn code(self) -> i64 {
        match self {
            Self::Hint => 0,
            Self::Replacement => 1,
        }
    }

    /// Неизвестный код читается как подсказка: она безопаснее замены.
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => Self::Replacement,
            _ => Self::Hint,
        }
    }
}

/// Термин глоссария: surface → canonical с языком и scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryTerm {
    pub id: String,
    pub surface: String,
    pub canonical: String,
    pub language: SpeechLanguage,
    pub scope: GlossaryScope,
    pub kind: GlossaryKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glossary_term_holds_meeting_scope() {
        let t = GlossaryTerm {
            id: "1".into(),
            surface: "униффи".into(),
            canonical: "UniFFI".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "s1".into(),
            },
            kind: GlossaryKind::Replacement,
        };
        assert!(matches!(t.scope, GlossaryScope::Meeting { .. }));
    }
}
