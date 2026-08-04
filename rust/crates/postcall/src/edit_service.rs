//! Что происходит при правке текста сегмента (Epic 19).
//!
//! Чистая функция: решает, что записать, но ничего не пишет. Так решение
//! тестируется без базы, а слой FFI остаётся тонким (`AGENTS.md`).

use domain::{
    FinalSegment, GlossaryKind, GlossaryScope, GlossaryTerm, SegmentEdit, SpeechLanguage,
};

use crate::term_from_edit;

/// Что нужно записать по итогам правки.
pub struct EditOutcome {
    /// `None` — правку надо удалить: текст вернули к исходному.
    pub edit: Option<SegmentEdit>,
    /// Термин к записи. `None` — правка не словарная.
    pub term: Option<GlossaryTerm>,
}

/// Разобрать правку: журнал плюс, возможно, подсказка в глоссарий.
///
/// `existing_terms` нужны, чтобы поймать повтор той же пары в другой
/// встрече: подсказка при повторе поднимается в глобальную область сама,
/// потому что готовый текст она не трогает и ошибиться ею нечем.
#[allow(clippy::too_many_arguments)]
pub fn plan_edit(
    meeting_id: &str,
    version: u32,
    segment: &FinalSegment,
    edited_text: &str,
    language: SpeechLanguage,
    existing_terms: &[GlossaryTerm],
    edit_id: &str,
    term_id: &str,
    now_ms: u64,
) -> EditOutcome {
    let edited = edited_text.trim();
    if edited == segment.text.trim() {
        return EditOutcome {
            edit: None,
            term: None,
        };
    }

    let edit = SegmentEdit {
        id: edit_id.to_owned(),
        meeting_id: meeting_id.to_owned(),
        channel: segment.channel,
        start_ms: segment.start_ms,
        end_ms: segment.end_ms,
        original_text: segment.text.clone(),
        edited_text: edited.to_owned(),
        created_at_ms: now_ms,
        applied_version: Some(version),
    };

    let term = term_from_edit(&segment.text, edited).map(|(surface, canonical)| {
        let seen_elsewhere = existing_terms.iter().any(|term| {
            term.kind == GlossaryKind::Hint
                && term.surface.to_lowercase() == surface.to_lowercase()
                && term.canonical.to_lowercase() == canonical.to_lowercase()
                && !matches!(&term.scope, GlossaryScope::Meeting { meeting_id: id } if id == meeting_id)
        });

        GlossaryTerm {
            id: term_id.to_owned(),
            surface,
            canonical,
            language,
            scope: if seen_elsewhere {
                GlossaryScope::Global
            } else {
                GlossaryScope::Meeting { meeting_id: meeting_id.to_owned() }
            },
            kind: GlossaryKind::Hint,
        }
    });

    EditOutcome {
        edit: Some(edit),
        term,
    }
}

#[cfg(test)]
mod tests {
    use domain::{
        AudioChannel, FinalSegment, GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage,
    };

    use super::plan_edit;

    fn segment() -> FinalSegment {
        FinalSegment {
            index: 0,
            start_ms: 1000,
            end_ms: 2000,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_pinned: false,
            text: "зашли на интра ру".into(),
            text_edited: false,
        }
    }

    #[test]
    fn edit_produces_meeting_hint() {
        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на intra.ru",
            SpeechLanguage::Ru,
            &[],
            "edit-1",
            "term-1",
            42,
        );

        let edit = outcome.edit.expect("правка записывается");
        assert_eq!(edit.original_text, "зашли на интра ру");
        assert_eq!(edit.applied_version, Some(1));

        let term = outcome.term.expect("термин рождается сам");
        assert_eq!(
            term.kind,
            GlossaryKind::Hint,
            "автоматически только подсказка"
        );
        assert_eq!(
            term.scope,
            GlossaryScope::Meeting {
                meeting_id: "m1".into()
            }
        );
        assert_eq!(term.surface, "интра ру");
        assert_eq!(term.canonical, "intra.ru");
    }

    #[test]
    fn repeat_in_another_meeting_promotes_hint_to_global() {
        let existing = GlossaryTerm {
            id: "old".into(),
            surface: "Интра Ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "m0".into(),
            },
            kind: GlossaryKind::Hint,
        };

        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на intra.ru",
            SpeechLanguage::Ru,
            &[existing],
            "edit-1",
            "term-1",
            42,
        );

        let term = outcome.term.expect("термин");
        assert_eq!(
            term.scope,
            GlossaryScope::Global,
            "повтор в другой встрече поднимает область"
        );
        assert_eq!(
            term.kind,
            GlossaryKind::Hint,
            "вид не меняется — поднимается только область"
        );
    }

    #[test]
    fn returning_original_text_removes_edit() {
        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на интра ру",
            SpeechLanguage::Ru,
            &[],
            "edit-1",
            "term-1",
            42,
        );

        assert!(outcome.edit.is_none(), "возврат к исходному — это отмена");
        assert!(outcome.term.is_none());
    }

    #[test]
    fn sentence_rewrite_saves_edit_without_term() {
        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "надо будет посмотреть портал на следующей неделе",
            SpeechLanguage::Ru,
            &[],
            "edit-1",
            "term-1",
            42,
        );

        assert!(outcome.edit.is_some(), "правка сохраняется всегда");
        assert!(outcome.term.is_none(), "но термином не становится");
    }
}
