//! Извлечение термина глоссария из ручной правки (Epic 19).
//!
//! Распознанный текст и есть surface, введённый — canonical: оба поля
//! заполняются из действия, и человеку не приходится понимать схему.
//!
//! Термином становится только короткая замена. Длинная — это правка
//! смысла, а не словарная, и в глоссарии она стала бы мусором, который
//! через `initial_prompt` портит распознавание.

use crate::{DiffOp, diff_words};
use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};

/// Сколько слов с каждой стороны ещё считается термином.
const MAX_WORDS: usize = 3;

/// Пара `(surface, canonical)` или `None`, если правка не словарная.
///
/// Берётся ровно одна замена: несколько правок в одном сегменте
/// разобрать однозначно нельзя, и угадывать здесь хуже, чем промолчать.
pub fn term_from_edit(original: &str, edited: &str) -> Option<(String, String)> {
    let spans = diff_words(original, edited);

    // Порядок соседей не фиксирован: грубая ветка diff_words ставит
    // Removed перед Added, LCS может выдать наоборот. surface всегда
    // берётся из Removed — это то, что распознала модель.
    let mut pair: Option<(String, String)> = None;
    let mut index = 0;
    // Цикл опирается на инвариант diff_words: никогда не выдаёт три и более
    // чередующихся несовпадающих участка подряд (т.е. Removed, Added, Removed,
    // Added и т.д.). Это позволяет пропускать сразу на индекс+2 при нахождении
    // пары (Removed, Added) или (Added, Removed). Если diff_words будет изменена,
    // это правило может сломаться тихо (см. зависимость в diff.rs).
    while index + 1 < spans.len() {
        let (left, right) = (&spans[index], &spans[index + 1]);
        let found = match (left.op, right.op) {
            (DiffOp::Removed, DiffOp::Added) => Some((&left.text, &right.text)),
            (DiffOp::Added, DiffOp::Removed) => Some((&right.text, &left.text)),
            _ => None,
        };
        if let Some((removed, added)) = found {
            if pair.is_some() {
                return None;
            }
            pair = Some((removed.trim().to_owned(), added.trim().to_owned()));
            index += 2;
            continue;
        }
        index += 1;
    }

    let (surface, canonical) = pair?;
    if surface.is_empty() || canonical.is_empty() {
        return None;
    }
    if word_count(&surface) > MAX_WORDS || word_count(&canonical) > MAX_WORDS {
        return None;
    }
    Some((surface, canonical))
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Подсказка, родившаяся из этой правки и действующая в этой встрече.
///
/// Пара берётся тем же разбором, что и при заведении термина
/// (`plan_edit`), и сравнивается без учёта регистра — как в `normalize`.
///
/// Замена не возвращается: она уже подтверждена человеком, повышать
/// нечего. Термин чужой встречи — тоже: замена там не применится, и
/// следующий пересбор её не подтвердит.
pub fn promotable_term<'a>(
    original_text: &str,
    edited_text: &str,
    terms: &'a [GlossaryTerm],
    meeting_id: &str,
    language: SpeechLanguage,
) -> Option<&'a GlossaryTerm> {
    let (surface, canonical) = term_from_edit(original_text, edited_text)?;
    terms.iter().find(|term| {
        term.kind == GlossaryKind::Hint
            && term.language == language
            && term.surface.to_lowercase() == surface.to_lowercase()
            && term.canonical.to_lowercase() == canonical.to_lowercase()
            && match &term.scope {
                GlossaryScope::Global => true,
                GlossaryScope::Meeting { meeting_id: id } => id == meeting_id,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};

    fn term(
        id: &str,
        surface: &str,
        canonical: &str,
        kind: GlossaryKind,
        scope: GlossaryScope,
    ) -> GlossaryTerm {
        GlossaryTerm {
            id: id.to_string(),
            surface: surface.to_string(),
            canonical: canonical.to_string(),
            language: SpeechLanguage::Ru,
            scope,
            kind,
        }
    }

    #[test]
    fn hint_born_from_edit_is_promotable() {
        let terms = vec![term(
            "t1",
            "юни-эф-эф-ай",
            "UniFFI",
            GlossaryKind::Hint,
            GlossaryScope::Meeting {
                meeting_id: "m1".to_string(),
            },
        )];
        let found = promotable_term(
            "упирается в юни-эф-эф-ай",
            "упирается в UniFFI",
            &terms,
            "m1",
            SpeechLanguage::Ru,
        );
        assert_eq!(found.map(|t| t.id.as_str()), Some("t1"));
    }

    /// Замена уже подтверждена человеком — повышать нечего.
    ///
    /// В данных лежит именно `Replacement`: с подсказкой тест прошёл бы
    /// вхолостую и ничего бы не проверил.
    #[test]
    fn replacement_is_not_promotable() {
        let terms = vec![term(
            "t1",
            "юни-эф-эф-ай",
            "UniFFI",
            GlossaryKind::Replacement,
            GlossaryScope::Global,
        )];
        let found = promotable_term(
            "упирается в юни-эф-эф-ай",
            "упирается в UniFFI",
            &terms,
            "m1",
            SpeechLanguage::Ru,
        );
        assert!(found.is_none());
    }

    /// Термин чужой встречи предлагать нельзя: замена там не применится.
    #[test]
    fn term_of_another_meeting_is_not_promotable() {
        let terms = vec![term(
            "t1",
            "юни-эф-эф-ай",
            "UniFFI",
            GlossaryKind::Hint,
            GlossaryScope::Meeting {
                meeting_id: "m2".to_string(),
            },
        )];
        let found = promotable_term(
            "упирается в юни-эф-эф-ай",
            "упирается в UniFFI",
            &terms,
            "m1",
            SpeechLanguage::Ru,
        );
        assert!(found.is_none());
    }

    /// Длинная правка термином не становится — значит и повышать нечего.
    #[test]
    fn long_edit_yields_no_promotable_term() {
        let terms = vec![term(
            "t1",
            "что-то",
            "другое",
            GlossaryKind::Hint,
            GlossaryScope::Global,
        )];
        let found = promotable_term(
            "мы решили это на прошлой неделе целиком",
            "мы договорились обсудить это в следующий понедельник",
            &terms,
            "m1",
            SpeechLanguage::Ru,
        );
        assert!(found.is_none());
    }

    #[test]
    fn single_word_replacement_becomes_term() {
        let result = term_from_edit("зашли на интра ру вчера", "зашли на intra.ru вчера");
        assert_eq!(result, Some(("интра ру".into(), "intra.ru".into())));
    }

    #[test]
    fn rewritten_sentence_gives_nothing() {
        let result = term_from_edit(
            "ну вот это самое надо бы посмотреть наверное",
            "нужно проверить это на следующей неделе обязательно",
        );
        assert_eq!(result, None, "правка смысла термином не становится");
    }

    #[test]
    fn long_side_gives_nothing() {
        let result = term_from_edit("открой интра ру", "открой внутренний портал нашей компании");
        assert_eq!(result, None, "больше трёх слов с одной стороны — не термин");
    }

    #[test]
    fn pure_insertion_gives_nothing() {
        let result = term_from_edit("зашли вчера", "зашли на intra.ru вчера");
        assert_eq!(result, None, "нечего заменять — нет surface");
    }

    #[test]
    fn identical_text_gives_nothing() {
        assert_eq!(term_from_edit("одно и то же", "одно и то же"), None);
    }
}
