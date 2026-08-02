//! Термины глоссария и область действия (Phase 5).

use crate::SpeechLanguage;

/// Область действия термина глоссария.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossaryScope {
    Global,
    Meeting { meeting_id: String },
}

/// Термин глоссария: surface → canonical с языком и scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryTerm {
    pub id: String,
    pub surface: String,
    pub canonical: String,
    pub language: SpeechLanguage,
    pub scope: GlossaryScope,
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
        };
        assert!(matches!(t.scope, GlossaryScope::Meeting { .. }));
    }
}
