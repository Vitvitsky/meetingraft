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

/// Правки, которые нужно завести, чтобы термин применился ко всем
/// вхождениям во встрече.
///
/// Идём через журнал, а не переписыванием таблицы сегментов: распознанное
/// должно остаться распознанным, иначе сравнить версии будет не с чем.
///
/// Места, уже правленные вручную, не трогаются — точечное решение
/// человека сильнее массовой замены, ровно как у `speaker_pinned`.
pub fn occurrences_to_edit(
    term: &GlossaryTerm,
    meeting_id: &str,
    version: u32,
    segments: &[FinalSegment],
    existing: &[SegmentEdit],
    now_ms: u64,
    ids: &mut dyn Iterator<Item = String>,
) -> Vec<SegmentEdit> {
    segments
        .iter()
        .filter(|segment| segment.text.contains(term.surface.as_str()))
        .filter(|segment| {
            !existing.iter().any(|edit| {
                edit.channel == segment.channel
                    && edit.start_ms == segment.start_ms
                    && edit.end_ms == segment.end_ms
            })
        })
        .filter_map(|segment| {
            let id = ids.next()?;
            Some(SegmentEdit {
                id,
                meeting_id: meeting_id.to_owned(),
                channel: segment.channel,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                original_text: segment.text.clone(),
                edited_text: segment.text.replace(term.surface.as_str(), &term.canonical),
                created_at_ms: now_ms,
                applied_version: Some(version),
            })
        })
        .collect()
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
    fn repeat_in_the_same_meeting_keeps_meeting_scope() {
        let existing = GlossaryTerm {
            id: "old".into(),
            surface: "Интра Ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "m1".into(),
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
            GlossaryScope::Meeting {
                meeting_id: "m1".into()
            },
            "повтор внутри одной встречи область не поднимает"
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

    #[test]
    fn replacement_covers_other_occurrences() {
        use super::occurrences_to_edit;
        use domain::SegmentEdit;

        let term = GlossaryTerm {
            id: "t1".into(),
            surface: "интра ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "m1".into(),
            },
            kind: GlossaryKind::Replacement,
        };
        let segments = vec![
            FinalSegment {
                index: 0,
                start_ms: 0,
                end_ms: 100,
                channel: AudioChannel::Mic,
                speaker_id: String::new(),
                speaker_pinned: false,
                text: "открой интра ру".into(),
                text_edited: false,
            },
            FinalSegment {
                index: 1,
                start_ms: 100,
                end_ms: 200,
                channel: AudioChannel::Mic,
                speaker_id: String::new(),
                speaker_pinned: false,
                text: "тут ничего нет".into(),
                text_edited: false,
            },
        ];
        let existing: Vec<SegmentEdit> = Vec::new();
        let mut ids = ["n1".to_string()].into_iter();

        let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

        assert_eq!(created.len(), 1, "правится только сегмент с вхождением");
        assert_eq!(created[0].edited_text, "открой intra.ru");
        assert_eq!(created[0].original_text, "открой интра ру");
    }

    #[test]
    fn replacement_skips_already_edited_places() {
        use super::occurrences_to_edit;
        use domain::SegmentEdit;

        let term = GlossaryTerm {
            id: "t1".into(),
            surface: "интра ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "m1".into(),
            },
            kind: GlossaryKind::Replacement,
        };
        let segments = vec![FinalSegment {
            index: 0,
            start_ms: 0,
            end_ms: 100,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_pinned: false,
            text: "открой интра ру".into(),
            text_edited: false,
        }];
        let existing = vec![SegmentEdit {
            id: "e0".into(),
            meeting_id: "m1".into(),
            channel: AudioChannel::Mic,
            start_ms: 0,
            end_ms: 100,
            original_text: "открой интра ру".into(),
            edited_text: "открой портал".into(),
            created_at_ms: 0,
            applied_version: Some(1),
        }];
        let mut ids = ["n1".to_string()].into_iter();

        let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

        assert!(
            created.is_empty(),
            "ручная правка человека сильнее массовой замены"
        );
    }
}
