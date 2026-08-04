//! Движок применения терминов глоссария.

use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};
use std::collections::HashSet;

use crate::normalize;

/// Подготовленный набор терминов для нормализации и prompt.
#[derive(Debug, Clone)]
pub struct GlossaryEngine {
    terms: Vec<GlossaryTerm>,
}

impl GlossaryEngine {
    /// Создаёт движок и располагает длинные surface-формы первыми.
    pub fn from_terms(mut terms: Vec<GlossaryTerm>) -> Self {
        terms.sort_by_key(|term| std::cmp::Reverse(term.surface.chars().count()));
        Self { terms }
    }

    /// Заменяет целые surface-фразы на canonical-формы.
    ///
    /// Подсказки не участвуют: они существуют ради `initial_prompt` и
    /// готовый текст не трогают (Epic 19).
    pub fn normalize_caption(&self, text: &str) -> String {
        normalize::normalize_with_kind(text, &self.terms, GlossaryKind::Replacement)
    }

    /// Собирает уникальные canonical-формы с приоритетом русского языка.
    pub fn build_whisper_prompt(&self, max_chars: usize) -> String {
        let mut terms: Vec<&GlossaryTerm> = self.terms.iter().collect();
        terms.sort_by_key(|term| language_rank(term.language));

        let mut seen = HashSet::new();
        let prompt = terms
            .into_iter()
            .filter(|term| !term.canonical.is_empty())
            .filter_map(|term| {
                seen.insert(term.canonical.as_str())
                    .then_some(term.canonical.as_str())
            })
            .collect::<Vec<_>>()
            .join(" ");

        prompt.chars().take(max_chars).collect()
    }
}

fn language_rank(language: SpeechLanguage) -> u8 {
    match language {
        SpeechLanguage::Ru => 0,
        SpeechLanguage::En => 1,
        SpeechLanguage::Es => 2,
    }
}

/// Выбирает глобальные термины и термины текущей встречи.
pub fn active_terms(all: &[GlossaryTerm], session_id: Option<&str>) -> Vec<GlossaryTerm> {
    all.iter()
        .filter(|term| match &term.scope {
            GlossaryScope::Global => true,
            GlossaryScope::Meeting { meeting_id } => session_id == Some(meeting_id.as_str()),
        })
        .cloned()
        .collect()
}
