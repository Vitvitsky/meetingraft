//! Доля ошибок в словах: сколько правок нужно, чтобы получить эталон.
//!
//! Считается расстоянием Левенштейна **по словам**, а не по буквам:
//! мерять качество распознавания посимвольно — значит хвалить движок за
//! верные окончания в неверном слове.
//!
//! ## Нормализация обязательна, и вот почему
//!
//! GigaAM-transducer пунктуации и заглавных не ставит, Whisper ставит.
//! Сравнить их без снятия того и другого нельзя вовсе: половина
//! «ошибок» окажется точками. `ё` сводится к `е` по той же причине —
//! движки расходятся в ней между собой, а слово это одно и то же.

/// Что показал разбор одной пары «эталон — расшифровка».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WerReport {
    /// Слов в эталоне. Ноль означает, что мерить было нечего.
    pub reference_words: usize,
    pub substitutions: usize,
    pub insertions: usize,
    pub deletions: usize,
}

impl WerReport {
    /// Доля ошибок. Больше единицы — законно: лишних слов бывает больше,
    /// чем эталонных.
    pub fn rate(&self) -> f32 {
        if self.reference_words == 0 {
            // Делить не на что. Ноль ошибок при пустом эталоне — не
            // «идеально», а «не мерялось»; отвечать за это должен
            // вызывающий, поэтому здесь честный ноль слов в отчёте.
            return 0.0;
        }
        (self.substitutions + self.insertions + self.deletions) as f32 / self.reference_words as f32
    }
}

/// Слова после нормализации: нижний регистр, без пунктуации, `ё` → `е`.
pub fn normalize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|symbol| symbol.is_alphanumeric())
                .flat_map(|symbol| symbol.to_lowercase())
                .map(|symbol| if symbol == 'ё' { 'е' } else { symbol })
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Сравнить расшифровку с эталоном.
pub fn wer(reference: &str, hypothesis: &str) -> WerReport {
    let reference = normalize(reference);
    let hypothesis = normalize(hypothesis);

    // Классическая таблица правок; в клетке — тройка (замены, вставки,
    // пропуски), чтобы отчёт называл вид ошибки, а не только их сумму.
    type Counts = (usize, usize, usize);
    let total = |c: Counts| c.0 + c.1 + c.2;

    let mut previous: Vec<Counts> = (0..=hypothesis.len()).map(|i| (0, i, 0)).collect();
    for (row, reference_word) in reference.iter().enumerate() {
        let mut current: Vec<Counts> = vec![(0, 0, row + 1); hypothesis.len() + 1];
        for (column, hypothesis_word) in hypothesis.iter().enumerate() {
            let matched = reference_word == hypothesis_word;
            let substitute = {
                let (s, i, d) = previous[column];
                if matched { (s, i, d) } else { (s + 1, i, d) }
            };
            let insert = {
                let (s, i, d) = current[column];
                (s, i + 1, d)
            };
            let delete = {
                let (s, i, d) = previous[column + 1];
                (s, i, d + 1)
            };
            let best = [substitute, insert, delete]
                .into_iter()
                .min_by_key(|counts| total(*counts))
                .expect("три варианта");
            current[column + 1] = best;
        }
        previous = current;
    }

    let (substitutions, insertions, deletions) = previous[hypothesis.len()];
    WerReport {
        reference_words: reference.len(),
        substitutions,
        insertions,
        deletions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: &str = "у лукоморья дуб зелёный";

    #[test]
    fn the_same_text_has_no_errors() {
        let report = wer(REFERENCE, REFERENCE);
        assert_eq!(report.rate(), 0.0, "{report:?}");
    }

    /// Пунктуация, регистр и `ё` ошибками не считаются: движки расходятся
    /// в них между собой, а слово — одно и то же.
    #[test]
    fn case_punctuation_and_yo_are_not_errors() {
        let report = wer(REFERENCE, "У лукоморья, дуб зеленый!");
        assert_eq!(report.rate(), 0.0, "{report:?}");
    }

    /// Одно слово из четырёх заменено. Ожидаемое выведено из фикстуры —
    /// одна ошибка на длину эталона, — а не вписано числом: вписанное
    /// пришлось бы подбирать под ответ и молча ломалось бы от правки
    /// текста.
    #[test]
    fn one_wrong_word_costs_one_error() {
        let expected = 1.0 / normalize(REFERENCE).len() as f32;
        let report = wer(REFERENCE, "у лукоморья дуб железный");
        assert_eq!(report.substitutions, 1, "{report:?}");
        assert_eq!(report.rate(), expected, "{report:?}");
    }

    #[test]
    fn a_missing_word_and_an_extra_word_are_told_apart() {
        let dropped = wer(REFERENCE, "у лукоморья зелёный");
        assert_eq!(dropped.deletions, 1, "{dropped:?}");
        assert_eq!(dropped.substitutions, 0, "{dropped:?}");

        let added = wer(REFERENCE, "у самого лукоморья дуб зелёный");
        assert_eq!(added.insertions, 1, "{added:?}");
        assert_eq!(added.substitutions, 0, "{added:?}");
    }

    /// Тот случай, ради которого всё: считалка, всегда возвращающая ноль,
    /// проходит три теста выше и валится здесь.
    #[test]
    fn a_completely_different_text_costs_everything() {
        let report = wer(REFERENCE, "совершенно другие слова про другое");
        assert!(report.rate() >= 1.0, "{report:?}");
    }

    /// Пустая расшифровка — это все слова пропущены, а не «ошибок нет».
    /// Движок, который молчит, обязан выглядеть как молчащий.
    #[test]
    fn an_empty_hypothesis_loses_every_word() {
        let report = wer(REFERENCE, "");
        assert_eq!(report.deletions, normalize(REFERENCE).len(), "{report:?}");
        assert_eq!(report.rate(), 1.0, "{report:?}");
    }

    /// А пустой эталон мерить нечем, и притворяться, что мерилось, нельзя.
    #[test]
    fn an_empty_reference_measures_nothing() {
        let report = wer("", "хоть что-нибудь");
        assert_eq!(report.reference_words, 0, "{report:?}");
    }
}
