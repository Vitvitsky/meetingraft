//! Что происходит при правке текста сегмента (Epic 19).
//!
//! Чистая функция: решает, что записать, но ничего не пишет. Так решение
//! тестируется без базы, а слой FFI остаётся тонким (`AGENTS.md`).

use domain::{
    FinalSegment, GlossaryKind, GlossaryScope, GlossaryTerm, SegmentEdit, SpeechLanguage,
    edits_by_position,
};
use glossary::GlossaryEngine;

use crate::term_from_edit;

/// Что нужно записать по итогам правки.
pub struct EditOutcome {
    /// `None` — правку надо удалить: текст вернули к исходному.
    pub edit: Option<SegmentEdit>,
    /// Термин к записи. `None` — правка не словарная.
    pub term: Option<GlossaryTerm>,
    /// Подсказки, которые накрыл глобальный термин и которые надо снести.
    ///
    /// Появляются, когда `term` получился глобальным: та же пара, лежащая
    /// в области встречи, после этого не значит уже ничего — глобальная
    /// покрывает все встречи, включая эту. Раньше такие строки оставались,
    /// и в словаре копились пары «встреча + глобальная» на один термин.
    ///
    /// Сюда попадают только подсказки. Замена области встречи переживает
    /// повышение: её поставил человек явным жестом, и снести её молча —
    /// ровно та потеря ручной работы, которой не должно быть.
    ///
    /// Порядок записи важен: сперва глобальный термин, потом удаление.
    /// Обрыв между ними оставит лишнюю строку, обратный порядок — потерю
    /// пары целиком.
    pub obsolete_term_ids: Vec<String>,
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
            obsolete_term_ids: Vec::new(),
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

    let mut obsolete_term_ids: Vec<String> = Vec::new();
    let term = term_from_edit(&segment.text, edited).map(|(surface, canonical)| {
        let same_pair = |term: &&GlossaryTerm| {
            term.language == language
                && term.surface.to_lowercase() == surface.to_lowercase()
                && term.canonical.to_lowercase() == canonical.to_lowercase()
        };

        let seen_elsewhere = existing_terms.iter().filter(same_pair).any(|term| {
            term.kind == GlossaryKind::Hint
                && !matches!(&term.scope, GlossaryScope::Meeting { meeting_id: id } if id == meeting_id)
        });

        // Термин, уже действующий в этой встрече, переиспользуется целиком:
        // запись в хранилище это «удалить по (форма, язык, область) и
        // вставить», поэтому новый id на каждую правку молча стирал бы
        // прежнюю строку — вместе с видом «замена», который человек
        // поставил явным жестом. Глобальный сильнее: он уже покрывает эту
        // встречу.
        let current = existing_terms
            .iter()
            .filter(same_pair)
            .find(|term| term.scope == GlossaryScope::Global)
            .or_else(|| {
                existing_terms.iter().filter(same_pair).find(|term| {
                    matches!(&term.scope, GlossaryScope::Meeting { meeting_id: id } if id == meeting_id)
                })
            });

        // Замена никогда не понижается до подсказки: понизить её может
        // только человек, и правка текста таким жестом не является.
        let kind = match current {
            Some(term) if term.kind == GlossaryKind::Replacement => GlossaryKind::Replacement,
            _ => GlossaryKind::Hint,
        };

        // Сама поднимается только подсказка: она готовый текст не трогает.
        // Замену повышать до глобальной без спроса нельзя — она перепишет
        // все будущие тексты (спека, «Повышение до глобального»).
        let promoted = seen_elsewhere && kind == GlossaryKind::Hint;
        let scope = match current {
            Some(term) if !promoted => term.scope.clone(),
            _ if promoted => GlossaryScope::Global,
            _ => GlossaryScope::Meeting { meeting_id: meeting_id.to_owned() },
        };

        let id = current.map_or_else(|| term_id.to_owned(), |term| term.id.clone());

        // Глобальный термин накрывает ту же пару в области встречи, и
        // держать обе строки незачем: они дают одну и ту же подсказку.
        // Пишем сюда только подсказки и только чужие id — свою строку
        // хранилище перезапишет само.
        if scope == GlossaryScope::Global {
            obsolete_term_ids.extend(
                existing_terms
                    .iter()
                    .filter(same_pair)
                    .filter(|term| {
                        term.kind == GlossaryKind::Hint
                            && term.scope != GlossaryScope::Global
                            && term.id != id
                    })
                    .map(|term| term.id.clone()),
            );
        }

        GlossaryTerm {
            id,
            surface,
            canonical,
            language,
            scope,
            kind,
        }
    });

    EditOutcome {
        edit: Some(edit),
        term,
        obsolete_term_ids,
    }
}

/// Правки, которые нужно завести, чтобы термин применился ко всем
/// вхождениям во встрече.
///
/// Идём через журнал, а не переписыванием таблицы сегментов: распознанное
/// должно остаться распознанным, иначе сравнить версии будет не с чем.
///
/// Места, уже правленные вручную **в этой версии**, не трогаются —
/// точечное решение человека сильнее массовой замены, ровно как у
/// `speaker_pinned`. Правка другой версии не в счёт: пересбор при
/// неизменной модели даёт ту же нарезку, и без фильтра по версии старая
/// правка беспричинно блокировала бы замену в текущей.
///
/// Поиск и замена идут через `GlossaryEngine::normalize_caption` — тот же
/// сопоставитель, что применяется к тексту при распознавании. Он не
/// зависит от регистра и проверяет границы слова, поэтому массовая замена
/// не расходится с тем, что увидит человек после следующей записи.
///
/// Термин обязан быть `GlossaryKind::Replacement`: `normalize_caption`
/// подсказки намеренно не применяет, поэтому с подсказкой функция вернёт
/// пустой список. Вид проверяет вызывающий — здесь молча подменять его
/// нельзя, это ровно то повышение подсказки до замены, которое требует
/// явного жеста человека.
pub fn occurrences_to_edit(
    term: &GlossaryTerm,
    meeting_id: &str,
    version: u32,
    segments: &[FinalSegment],
    existing: &[SegmentEdit],
    now_ms: u64,
    ids: &mut dyn Iterator<Item = String>,
) -> Vec<SegmentEdit> {
    let engine = GlossaryEngine::from_terms(vec![term.clone()]);
    let edited_places = edits_by_position(existing, version);

    segments
        .iter()
        .filter_map(|segment| {
            let replaced = engine.normalize_caption(&segment.text);
            (replaced != segment.text).then_some((segment, replaced))
        })
        .filter(|(segment, _)| !edited_places.contains_key(&segment.position()))
        .filter_map(|(segment, replaced)| {
            let id = ids.next()?;
            Some(SegmentEdit {
                id,
                meeting_id: meeting_id.to_owned(),
                channel: segment.channel,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                original_text: segment.text.clone(),
                edited_text: replaced,
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
            original_text: String::new(),
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

    fn hint(id: &str, scope: GlossaryScope) -> GlossaryTerm {
        GlossaryTerm {
            id: id.into(),
            surface: "интра ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope,
            kind: GlossaryKind::Hint,
        }
    }

    fn edit_in_m1(existing: &[GlossaryTerm]) -> super::EditOutcome {
        plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на intra.ru",
            SpeechLanguage::Ru,
            existing,
            "edit-1",
            "term-1",
            42,
        )
    }

    /// Глобальная подсказка накрывает свою же копию в области встречи.
    ///
    /// Раньше строка встречи оставалась, и в словаре копились пары
    /// «встреча + глобальная» на один термин: обе дают одну подсказку, и
    /// разобраться, зачем их две, человеку было нечем.
    #[test]
    fn global_scope_makes_the_meeting_twin_obsolete() {
        let existing = vec![
            hint("global", GlossaryScope::Global),
            hint(
                "twin",
                GlossaryScope::Meeting {
                    meeting_id: "m1".into(),
                },
            ),
        ];

        let outcome = edit_in_m1(&existing);

        let term = outcome.term.expect("термин");
        assert_eq!(term.scope, GlossaryScope::Global);
        assert_eq!(term.id, "global", "переиспользуется действующая строка");
        assert_eq!(outcome.obsolete_term_ids, vec!["twin".to_string()]);
    }

    /// Замену области встречи повышение не сносит.
    ///
    /// Её поставил человек явным жестом; глобальная подсказка переписывать
    /// готовый текст не станет и потому заменой не является.
    #[test]
    fn meeting_replacement_survives_promotion() {
        let mut replacement = hint(
            "mine",
            GlossaryScope::Meeting {
                meeting_id: "m1".into(),
            },
        );
        replacement.kind = GlossaryKind::Replacement;
        let existing = vec![hint("global", GlossaryScope::Global), replacement];

        let outcome = edit_in_m1(&existing);

        assert_eq!(
            outcome.term.expect("термин").scope,
            GlossaryScope::Global,
            "область всё же глобальная — иначе тест проверяет не тот случай"
        );
        assert!(
            outcome.obsolete_term_ids.is_empty(),
            "снесли замену: {:?}",
            outcome.obsolete_term_ids
        );
    }

    /// Первая правка: сносить нечего, и списка быть не должно.
    #[test]
    fn meeting_scope_makes_nothing_obsolete() {
        let outcome = edit_in_m1(&[]);

        assert_eq!(
            outcome.term.expect("термин").scope,
            GlossaryScope::Meeting {
                meeting_id: "m1".into()
            }
        );
        assert!(outcome.obsolete_term_ids.is_empty());
    }

    /// Своя же строка в список на снос не попадает.
    ///
    /// Повышаемая подсказка встречи переезжает в глобальную область под
    /// тем же id, и хранилище перепишет её само. Попади она в список —
    /// удаление после записи стёрло бы только что заведённый термин.
    #[test]
    fn promoted_term_does_not_delete_itself() {
        let existing = vec![
            hint(
                "elsewhere",
                GlossaryScope::Meeting {
                    meeting_id: "m0".into(),
                },
            ),
            hint(
                "mine",
                GlossaryScope::Meeting {
                    meeting_id: "m1".into(),
                },
            ),
        ];

        let outcome = edit_in_m1(&existing);

        let term = outcome.term.expect("термин");
        assert_eq!(term.scope, GlossaryScope::Global);
        assert_eq!(term.id, "mine", "переезжает строка этой встречи");
        assert!(
            !outcome.obsolete_term_ids.contains(&term.id),
            "термин сносит сам себя: {:?}",
            outcome.obsolete_term_ids
        );
        assert_eq!(outcome.obsolete_term_ids, vec!["elsewhere".to_string()]);
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

    /// Правка той же фразы не должна разжаловать замену обратно в
    /// подсказку. Запись термина — это «удалить по (форма, язык, область)
    /// и вставить», поэтому новый термин с новым id молча стёр бы строку
    /// «замена», созданную явным жестом человека, и замена перестала бы
    /// работать без единого сигнала.
    #[test]
    fn repeated_edit_keeps_the_replacement_and_its_id() {
        let existing = GlossaryTerm {
            id: "term-old".into(),
            surface: "Интра Ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "m1".into(),
            },
            kind: GlossaryKind::Replacement,
        };

        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на intra.ru",
            SpeechLanguage::Ru,
            &[existing],
            "edit-1",
            "term-new",
            42,
        );

        let term = outcome.term.expect("термин");
        assert_eq!(
            term.id, "term-old",
            "идентификатор термина переиспользуется"
        );
        assert_eq!(
            term.kind,
            GlossaryKind::Replacement,
            "замену понижает только человек"
        );
    }

    /// Подсказка при повторе в другой встрече поднимается сама, замена —
    /// нет: она переписывает готовый текст, и цена ошибки другая (спека,
    /// «Повышение до глобального»).
    #[test]
    fn replacement_is_not_promoted_to_global_on_its_own() {
        let here = GlossaryTerm {
            id: "term-here".into(),
            surface: "интра ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "m1".into(),
            },
            kind: GlossaryKind::Replacement,
        };
        let elsewhere = GlossaryTerm {
            id: "term-there".into(),
            surface: "интра ру".into(),
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
            &[here, elsewhere],
            "edit-1",
            "term-new",
            42,
        );

        let term = outcome.term.expect("термин");
        assert_eq!(
            term.scope,
            GlossaryScope::Meeting {
                meeting_id: "m1".into()
            },
            "замена не расползается по всем встречам сама"
        );
        assert_eq!(term.kind, GlossaryKind::Replacement);
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
                original_text: String::new(),
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
                original_text: String::new(),
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
            original_text: String::new(),
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

    /// Ручная правка другой версии массовую замену в текущей не блокирует:
    /// пересбор при неизменной модели даёт ту же нарезку, и совпадение
    /// координат между версиями — норма, а не редкость.
    #[test]
    fn replacement_ignores_manual_edits_of_other_versions() {
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
            original_text: String::new(),
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

        let created = occurrences_to_edit(&term, "m1", 2, &segments, &existing, 7, &mut ids);

        assert_eq!(
            created.len(),
            1,
            "правка первой версии вторую версию не защищает"
        );
    }

    /// Массовая замена опирается на `normalize_caption`, а тот подсказки
    /// намеренно не применяет. Требование к виду документировано в
    /// `occurrences_to_edit`; тест закрепляет, что подсказка проходит
    /// молча и вхождений не создаёт.
    #[test]
    fn hint_term_produces_no_occurrences() {
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
            kind: GlossaryKind::Hint,
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
            original_text: String::new(),
        }];
        let existing: Vec<SegmentEdit> = Vec::new();
        let mut ids = ["n1".to_string()].into_iter();

        let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

        assert!(
            created.is_empty(),
            "подсказка готовый текст не переписывает"
        );
    }

    #[test]
    fn replacement_ignores_case() {
        use super::occurrences_to_edit;
        use domain::SegmentEdit;

        // Распознаватель пишет первое слово предложения с заглавной —
        // сопоставление обязано игнорировать регистр, иначе часть вхождений
        // останется нетронутой без какого-либо сигнала об этом.
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
            text: "Интра ру открывается медленно".into(),
            text_edited: false,
            original_text: String::new(),
        }];
        let existing: Vec<SegmentEdit> = Vec::new();
        let mut ids = ["n1".to_string()].into_iter();

        let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

        assert_eq!(
            created.len(),
            1,
            "вхождение с заглавной буквы тоже находится"
        );
        assert_eq!(created[0].edited_text, "intra.ru открывается медленно");
    }

    #[test]
    fn replacement_covers_both_occurrences_in_one_segment() {
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
            text: "открой интра ру, а потом снова интра ру".into(),
            text_edited: false,
            original_text: String::new(),
        }];
        let existing: Vec<SegmentEdit> = Vec::new();
        let mut ids = ["n1".to_string()].into_iter();

        let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

        assert_eq!(created.len(), 1, "обе замены — правка одного сегмента");
        assert_eq!(
            created[0].edited_text, "открой intra.ru, а потом снова intra.ru",
            "заменяются все вхождения в сегменте, а не только первое"
        );
    }
}
