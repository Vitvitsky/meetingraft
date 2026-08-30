//! Термины глоссария и цена их продвижения.
//!
//! Смещение меряется **с двух сторон**, и это главное здесь. Считать
//! только пойманные термины — значит получить механизм, который всегда
//! выглядит успешным: подними силу, и в расшифровке появится больше
//! терминов. Появятся они и там, где их не говорили.
//!
//! Поэтому рядом с «сколько терминов доехало» стоит «сколько слов
//! эталона заменилось на термин, которым они не были».

use serde::{Deserialize, Serialize};

use crate::wer::normalize;

/// Что смещение дало и чего оно стоило.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiasingReport {
    /// Термины, встретившиеся в эталоне и попавшие в расшифровку.
    pub caught: usize,
    /// Термины, встретившиеся в эталоне и **не** попавшие.
    pub missed: usize,
    /// Термины в расшифровке, которых в эталоне нет вовсе.
    ///
    /// Это и есть цена. Слово, притянутое смещением на место чужого, —
    /// не «лишний термин», а **подменённая речь человека**.
    pub pulled_in: usize,
}

/// Сравнить эталон и расшифровку по списку терминов.
///
/// Считается по числу вхождений, а не по факту наличия: термин,
/// сказанный трижды и распознанный один раз, — это два промаха, а не
/// успех.
pub fn measure(terms: &[String], reference: &str, heard: &str) -> BiasingReport {
    let reference_words = normalize(reference);
    let heard_words = normalize(heard);

    let mut caught = 0usize;
    let mut missed = 0usize;
    let mut pulled_in = 0usize;

    for term in terms {
        let term = normalize(term);
        if term.is_empty() {
            continue;
        }
        let in_reference = count_occurrences(&reference_words, &term);
        let in_heard = count_occurrences(&heard_words, &term);

        caught += in_reference.min(in_heard);
        missed += in_reference.saturating_sub(in_heard);
        pulled_in += in_heard.saturating_sub(in_reference);
    }

    BiasingReport {
        caught,
        missed,
        pulled_in,
    }
}

/// Сколько раз последовательность слов встречается в тексте.
fn count_occurrences(text: &[String], phrase: &[String]) -> usize {
    if phrase.is_empty() || text.len() < phrase.len() {
        return 0;
    }
    text.windows(phrase.len())
        .filter(|window| *window == phrase)
        .count()
}

/// Прочитать термины из файла: по одному на строку, пустые и `#` — мимо.
pub fn read_terms(path: &std::path::Path) -> Result<Vec<String>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}

/// Записать файл терминов в том виде, в каком его ждёт sherpa: по одной
/// фразе на строку.
pub fn write_hotwords(terms: &[String], path: &std::path::Path) -> Result<(), String> {
    let body = terms.join("\n");
    std::fs::write(path, body).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|term| term.to_string()).collect()
    }

    /// Термин сказан и распознан — пойман.
    #[test]
    fn a_term_that_was_said_and_heard_is_caught() {
        let report = measure(
            &terms(&["униффи"]),
            "мы вынесли это в униффи",
            "мы вынесли это в униффи",
        );
        assert_eq!(report.caught, 1);
        assert_eq!(report.missed, 0);
        assert_eq!(report.pulled_in, 0);
    }

    /// Термин сказан и не распознан — промах, а не тишина.
    #[test]
    fn a_term_that_was_said_and_missed_is_counted() {
        let report = measure(
            &terms(&["униффи"]),
            "мы вынесли это в униффи",
            "мы вынесли это в юнифай",
        );
        assert_eq!(report.caught, 0);
        assert_eq!(report.missed, 1);
        assert_eq!(report.pulled_in, 0);
    }

    /// **Термин не сказан, но появился — это цена, и её видно.**
    ///
    /// Тот случай, ради которого метрика двусторонняя: смещение,
    /// считающее только первую строку таблицы, здесь показало бы успех.
    #[test]
    fn a_term_that_was_not_said_but_appeared_is_the_price() {
        let report = measure(
            &terms(&["униффи"]),
            "мы вынесли это в интерфейс",
            "мы вынесли это в униффи",
        );
        assert_eq!(report.caught, 0);
        assert_eq!(report.pulled_in, 1, "притянутое слово обязано считаться");
    }

    /// Повторы считаются по числу вхождений: сказанный трижды и
    /// распознанный однажды — это два промаха, а не успех.
    #[test]
    fn repeats_are_counted_not_collapsed() {
        let report = measure(&terms(&["ядро"]), "ядро ядро ядро", "ядро дыра дыра");
        assert_eq!(report.caught, 1);
        assert_eq!(report.missed, 2);
    }

    /// Термин из нескольких слов ищется целиком, а не по словам.
    #[test]
    fn a_multi_word_term_is_matched_as_a_phrase() {
        let report = measure(
            &terms(&["живые субтитры"]),
            "мы правим живые субтитры",
            "мы правим живые субтитры",
        );
        assert_eq!(report.caught, 1);

        let split = measure(
            &terms(&["живые субтитры"]),
            "мы правим живые субтитры",
            "живые мы правим субтитры",
        );
        assert_eq!(split.caught, 0, "рассыпанная фраза термином не считается");
        assert_eq!(split.missed, 1);
    }

    /// Пустой список терминов ничего не утверждает.
    #[test]
    fn an_empty_glossary_claims_nothing() {
        let report = measure(&[], "любой текст", "любой другой текст");
        assert_eq!(report.caught, 0);
        assert_eq!(report.missed, 0);
        assert_eq!(report.pulled_in, 0);
    }
}
