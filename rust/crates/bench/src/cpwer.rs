//! cpWER: правильное ли слово попало правильному говорящему.
//!
//! Ни WER, ни DER на этот вопрос не отвечают. WER не знает про людей
//! вовсе. DER считает **время**, а не слова, и запись, где каждому отдана
//! верная доля секунд с полностью перепутанным текстом, у него отличная.
//!
//! Считается так: текст каждого говорящего склеивается по времени
//! отдельно в правде и в гипотезе, метки сопоставляются перебором,
//! берётся сопоставление с наименьшей общей ошибкой. Перебор законен,
//! пока людей единицы; на большем числе метрика **отказывает**, а не
//! считает наугад.
//!
//! Метки гипотезы произвольны по построению: диаризация отдаёт номера
//! кластеров, а не имена. Поэтому перепутанные имена при верном тексте —
//! не ошибка, и это проверяется тестом.
//!
//! **Из этого следует то, чего метрика не умеет.** Она судит разбиение,
//! а не имена: «перепутанный текст при верных именах» и «верный текст
//! при перепутанных именах» — для неё одно и то же, и оба стоят ноль.
//! Ловится это не ей, а тем, что рядом печатается обычный WER.

use crate::wer::{normalize, wer};

/// Больше этого числа голосов перебор не судит: перестановок становится
/// слишком много, а встреч с таким числом людей у нас нет.
const MAX_SPEAKERS: usize = 6;

/// Реплика с говорящим: начало, конец, текст, метка.
pub type Turn = (u64, u64, String, String);

/// Доля ошибок при лучшем сопоставлении меток.
pub fn cpwer(truth: &[Turn], hypothesis: &[Turn]) -> Result<f32, String> {
    let truth_labels = labels(truth);
    let hypothesis_labels = labels(hypothesis);

    if truth_labels.is_empty() {
        return Err("в правде нет ни одной реплики: мерить нечем".to_string());
    }
    if truth_labels.len() > MAX_SPEAKERS || hypothesis_labels.len() > MAX_SPEAKERS {
        return Err(format!(
            "перебор сопоставлений не судит больше {MAX_SPEAKERS} голосов: \
             в правде {}, в гипотезе {}",
            truth_labels.len(),
            hypothesis_labels.len()
        ));
    }

    // Меток в гипотезе может быть меньше, чем в правде: движок мог
    // услышать не всех. Недостающие подставляются пустыми — говорящий,
    // которому не сопоставлено ничего, теряет все свои слова, и это
    // верная цена, а не отказ.
    let mut padded = hypothesis_labels.clone();
    while padded.len() < truth_labels.len() {
        padded.push(String::new());
    }

    let mut best = f32::INFINITY;
    for mapping in permutations(&padded) {
        let mut errors = 0.0f32;
        let mut words = 0.0f32;
        for (index, truth_label) in truth_labels.iter().enumerate() {
            let reference = joined(truth, truth_label);
            let candidate = mapping.get(index).cloned().unwrap_or_default();
            let heard = joined(hypothesis, &candidate);
            let report = wer(&reference, &heard);
            let reference_words = normalize(&reference).len() as f32;
            errors += report.rate() * reference_words;
            words += reference_words;
        }
        let rate = if words == 0.0 { 0.0 } else { errors / words };
        if rate < best {
            best = rate;
        }
    }
    Ok(best)
}

/// Метки, встречающиеся в репликах.
fn labels(turns: &[Turn]) -> Vec<String> {
    let mut out: Vec<String> = turns.iter().map(|turn| turn.3.clone()).collect();
    out.sort();
    out.dedup();
    out
}

/// Склеить текст одного говорящего по времени.
fn joined(turns: &[Turn], label: &str) -> String {
    if label.is_empty() {
        return String::new();
    }
    let mut picked: Vec<&Turn> = turns.iter().filter(|turn| turn.3 == label).collect();
    picked.sort_by_key(|turn| turn.0);
    picked
        .iter()
        .map(|turn| turn.2.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Все перестановки меток.
fn permutations(items: &[String]) -> Vec<Vec<String>> {
    if items.is_empty() {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(index);
        for tail in permutations(&rest) {
            let mut one = vec![item.clone()];
            one.extend(tail);
            out.push(one);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(start: u64, end: u64, text: &str, speaker: &str) -> Turn {
        (start, end, text.to_string(), speaker.to_string())
    }

    fn truth() -> Vec<Turn> {
        vec![
            turn(0, 1000, "привет всем рад видеть", "аня"),
            turn(1000, 2000, "добрый день начнём пожалуй", "боря"),
        ]
    }

    /// Перепутанные **имена** при верном тексте не стоят ничего: номер
    /// кластера произволен по построению, и метрика ищет лучшее
    /// сопоставление меток.
    #[test]
    fn swapped_labels_with_correct_text_cost_nothing() {
        let hypothesis = vec![
            turn(0, 1000, "привет всем рад видеть", "1"),
            turn(1000, 2000, "добрый день начнём пожалуй", "0"),
        ];
        assert_eq!(cpwer(&truth(), &hypothesis).expect("посчиталось"), 0.0);
    }

    /// Перепутанный текст при **верных** именах метрике неотличим от
    /// верного текста при перепутанных именах: это одна и та же
    /// перестановка, а метрика по построению ищет лучшую.
    ///
    /// Тест писался с обратным ожиданием («полная ошибка») и упал. Он был
    /// неправ: cpWER судит **разбиение**, а не имена, и требовать от неё
    /// различать эти два случая — значит требовать невозможного. Тест
    /// остаётся здесь именно как запись этого свойства: без него
    /// следующий читатель поставит то же неверное ожидание снова.
    #[test]
    fn swapped_text_with_correct_names_is_the_same_thing_as_swapped_names() {
        let hypothesis = vec![
            turn(0, 1000, "добрый день начнём пожалуй", "аня"),
            turn(1000, 2000, "привет всем рад видеть", "боря"),
        ];
        assert_eq!(cpwer(&truth(), &hypothesis).expect("посчиталось"), 0.0);
    }

    /// А неверный текст — ошибка, и её видно.
    ///
    /// Вот заведомо отрицательный случай, которого не даёт предыдущий
    /// тест: метрика, всегда отдающая ноль, проходит оба теста выше и
    /// валится здесь.
    #[test]
    fn wrong_words_cost_even_with_the_right_partition() {
        let hypothesis = vec![
            turn(0, 1000, "привет всем рад видеть", "0"),
            turn(1000, 2000, "совершенно другие слова тут", "1"),
        ];
        let rate = cpwer(&truth(), &hypothesis).expect("посчиталось");
        // Восемь слов в правде, четыре из них не угаданы вовсе.
        assert!(
            (0.45..=0.55).contains(&rate),
            "половина слов неверна, а вышло {rate}"
        );
    }

    /// Слияние двух голосов в один стоит **всего**, а не половины.
    ///
    /// Ожидание «примерно половина» тоже упало и тоже было неправо:
    /// слитому голосу достаются чужие слова (вставки), а второй остаётся
    /// без своих (пропуски) — платится дважды. Это верное поведение
    /// метрики, и знать его надо заранее: иначе прогон, где диаризация
    /// слила всех в одного, прочтётся как «ну, наполовину сработало».
    #[test]
    fn merging_two_speakers_into_one_costs_everything() {
        let hypothesis = vec![
            turn(0, 1000, "привет всем рад видеть", "0"),
            turn(1000, 2000, "добрый день начнём пожалуй", "0"),
        ];
        let rate = cpwer(&truth(), &hypothesis).expect("посчиталось");
        assert!(
            rate >= 1.0,
            "слияние платит и вставками, и пропусками, а вышло {rate}"
        );
    }

    /// Пустая правда — отказ, а не ноль. Ноль здесь читался бы как
    /// «идеально».
    #[test]
    fn an_empty_truth_refuses_instead_of_scoring_zero() {
        let error = cpwer(&[], &truth()).expect_err("обязан отказать");
        assert!(error.contains("мерить нечем"), "{error}");
    }

    /// Слишком много голосов — отказ с причиной, а не тихая
    /// комбинаторная яма.
    #[test]
    fn too_many_speakers_is_refused_by_name() {
        let many: Vec<Turn> = (0..MAX_SPEAKERS + 1)
            .map(|index| turn(index as u64, index as u64 + 1, "слово", &index.to_string()))
            .collect();
        let error = cpwer(&many, &many).expect_err("обязан отказать");
        assert!(error.contains("не судит больше"), "{error}");
    }
}
