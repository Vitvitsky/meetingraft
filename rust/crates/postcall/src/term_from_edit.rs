//! Извлечение термина глоссария из ручной правки (Epic 19).
//!
//! Распознанный текст и есть surface, введённый — canonical: оба поля
//! заполняются из действия, и человеку не приходится понимать схему.
//!
//! Термином становится только короткая замена. Длинная — это правка
//! смысла, а не словарная, и в глоссарии она стала бы мусором, который
//! через `initial_prompt` портит распознавание.

use crate::{DiffOp, diff_words};

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

#[cfg(test)]
mod tests {
    use super::*;

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
