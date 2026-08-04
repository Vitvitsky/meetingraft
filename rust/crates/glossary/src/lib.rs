//! Нормализация подписей, CSV-импорт и prompt из глоссария.

mod csv_import;
mod engine;
mod normalize;

pub use csv_import::parse_csv;
pub use engine::{GlossaryEngine, active_terms};

#[cfg(test)]
mod tests {
    use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};

    use crate::{GlossaryEngine, active_terms, parse_csv};

    fn term_global(surface: &str, canonical: &str) -> GlossaryTerm {
        term(
            surface,
            canonical,
            SpeechLanguage::Ru,
            GlossaryScope::Global,
        )
    }

    fn term(
        surface: &str,
        canonical: &str,
        language: SpeechLanguage,
        scope: GlossaryScope,
    ) -> GlossaryTerm {
        GlossaryTerm {
            id: surface.into(),
            surface: surface.into(),
            canonical: canonical.into(),
            language,
            scope,
            kind: GlossaryKind::Replacement,
        }
    }

    #[test]
    fn normalizes_uniffi_surface() {
        let eng = GlossaryEngine::from_terms(vec![term_global("униффи", "UniFFI")]);
        assert_eq!(
            eng.normalize_caption("посмотри униффи завтра"),
            "посмотри UniFFI завтра"
        );
    }

    #[test]
    fn longer_surface_wins() {
        let eng = GlossaryEngine::from_terms(vec![
            term_global("униф", "SHORT"),
            term_global("униффи", "UniFFI"),
        ]);
        assert_eq!(eng.normalize_caption("униффи"), "UniFFI");
    }

    #[test]
    fn empty_glossary_is_identity() {
        let eng = GlossaryEngine::from_terms(vec![]);
        assert_eq!(eng.normalize_caption("привет"), "привет");
    }

    #[test]
    fn parses_valid_csv_rows_and_skips_invalid_rows() {
        let csv = "surface,canonical,language,scope\n\
                   униффи,UniFFI,ru,global\n\
                   foo,Foo,en,global\n\
                   bad,,,global\n";

        let (terms, skipped) = parse_csv(csv).expect("корректный CSV должен разбираться");

        assert_eq!(terms.len(), 2);
        assert!(skipped >= 1);
        assert_eq!(terms[0].surface, "униффи");
        assert_eq!(terms[0].canonical, "UniFFI");
        assert_eq!(terms[0].language, SpeechLanguage::Ru);
        assert_eq!(terms[1].language, SpeechLanguage::En);
        assert!(terms.iter().all(|term| !term.id.is_empty()));
    }

    #[test]
    fn parses_quoted_csv_fields_with_commas_and_escaped_quotes() {
        let csv = "surface,canonical,language,scope\n\
                   \"рафт, митинг\",\"Meeting \"\"Raft\"\"\",ru,global\n";

        let (terms, skipped) = parse_csv(csv).expect("CSV с кавычками должен разбираться");

        assert_eq!(skipped, 0);
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].surface, "рафт, митинг");
        assert_eq!(terms[0].canonical, "Meeting \"Raft\"");
    }

    #[test]
    fn builds_unique_russian_first_prompt_with_character_limit() {
        let eng = GlossaryEngine::from_terms(vec![
            term("foo", "Foo", SpeechLanguage::En, GlossaryScope::Global),
            term_global("униффи", "UniFFI"),
            term_global("дубликат", "UniFFI"),
            term("bar", "Bar", SpeechLanguage::Es, GlossaryScope::Global),
        ]);

        assert_eq!(eng.build_whisper_prompt(100), "UniFFI Foo Bar");
        assert_eq!(eng.build_whisper_prompt(10), "UniFFI Foo");
    }

    #[test]
    fn selects_global_and_matching_meeting_terms() {
        let global = term_global("униффи", "UniFFI");
        let matching = term(
            "рафт",
            "Raft",
            SpeechLanguage::Ru,
            GlossaryScope::Meeting {
                meeting_id: "session-1".into(),
            },
        );
        let unrelated = term(
            "другое",
            "Other",
            SpeechLanguage::Ru,
            GlossaryScope::Meeting {
                meeting_id: "session-2".into(),
            },
        );

        assert_eq!(
            active_terms(
                &[global.clone(), matching.clone(), unrelated],
                Some("session-1")
            ),
            vec![global.clone(), matching]
        );
        assert_eq!(
            active_terms(std::slice::from_ref(&global), None),
            vec![global]
        );
    }

    #[test]
    fn hint_does_not_rewrite_text() {
        let engine = GlossaryEngine::from_terms(vec![GlossaryTerm {
            id: "1".into(),
            surface: "пошли".into(),
            canonical: "пошёл".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Global,
            kind: GlossaryKind::Hint,
        }]);

        assert_eq!(engine.normalize_caption("пошли дальше"), "пошли дальше");
    }

    #[test]
    fn hint_still_reaches_whisper_prompt() {
        let engine = GlossaryEngine::from_terms(vec![GlossaryTerm {
            id: "1".into(),
            surface: "интра ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Global,
            kind: GlossaryKind::Hint,
        }]);

        assert_eq!(engine.build_whisper_prompt(100), "intra.ru");
    }
}
