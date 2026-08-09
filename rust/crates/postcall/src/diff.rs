//! Пословная диффа Live против Final (Phase 10, T8).
//!
//! Считается здесь, а не в UI: сравнение — доменная логика, вью только
//! раскрашивает результат (`AGENTS.md`).
//!
//! Вкладка Compare до этого показывала две почти одинаковые панели,
//! потому что Final был тем же текстом с подставленными терминами. После
//! настоящего второго прохода разница появляется — но увидеть её глазами
//! в часовом транскрипте невозможно, поэтому её надо показать явно.

/// Что произошло со словом при переходе Live → Final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOp {
    Equal,
    /// Было в Live, исчезло в Final.
    Removed,
    /// Появилось в Final.
    Added,
}

impl DiffOp {
    pub fn code(self) -> &'static str {
        match self {
            Self::Equal => "equal",
            Self::Removed => "removed",
            Self::Added => "added",
        }
    }
}

/// Подряд идущие слова с одинаковой судьбой.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSpan {
    pub op: DiffOp,
    pub text: String,
}

/// Потолок LCS: таблица растёт как произведение длин, и часовая встреча
/// без ограничения съела бы гигабайты. Выше потолка отдаём грубый блок —
/// честнее, чем повесить приложение.
const MAX_LCS_WORDS: usize = 1200;

/// Сравнить два текста по словам.
///
/// Инвариант для потребителей: результат никогда не содержит три и более
/// чередующихся несовпадающих участков подряд (`Removed, Added, Removed` и
/// так далее). На него опирается `term_from_edit`: найдя пару, она прыгает
/// сразу через два участка, и третий проглотила бы молча.
///
/// Инвариант держится перебором коротких пар —
/// `diff_never_emits_three_alternating_spans`. Меняешь разбор — гоняй его:
/// простая «пара на каждое несовпадение» ломает инвариант на `а а` → `б`.
pub fn diff_words(live: &str, final_text: &str) -> Vec<DiffSpan> {
    let left: Vec<&str> = live.split_whitespace().collect();
    let right: Vec<&str> = final_text.split_whitespace().collect();

    // Общие края отрезаются до LCS: в нашем случае тексты совпадают почти
    // целиком, и это убирает основную часть работы.
    let prefix = common_prefix(&left, &right);
    let suffix = common_suffix(&left[prefix..], &right[prefix..]);
    let left_mid = &left[prefix..left.len() - suffix];
    let right_mid = &right[prefix..right.len() - suffix];

    let mut spans: Vec<DiffSpan> = Vec::new();
    // В совпавших участках показываем вариант Final — он и есть результат
    // прохода. Слова равны только после нормализации, поэтому пунктуация
    // и регистр в них могут отличаться, и брать надо причёсанный вариант.
    push_words(&mut spans, DiffOp::Equal, &right[..prefix]);

    if left_mid.len() > MAX_LCS_WORDS || right_mid.len() > MAX_LCS_WORDS {
        push_words(&mut spans, DiffOp::Removed, left_mid);
        push_words(&mut spans, DiffOp::Added, right_mid);
    } else {
        spans.extend(lcs_diff(left_mid, right_mid));
    }

    push_words(&mut spans, DiffOp::Equal, &right[right.len() - suffix..]);
    merge_spans(spans)
}

/// Форма слова для сравнения: регистр и краевая пунктуация не в счёт.
/// Иначе полировка, добавившая запятую, показалась бы правкой смысла.
fn normalized(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

fn same(left: &str, right: &str) -> bool {
    let left = normalized(left);
    !left.is_empty() && left == normalized(right)
}

fn common_prefix(left: &[&str], right: &[&str]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(a, b)| same(a, b))
        .count()
}

fn common_suffix(left: &[&str], right: &[&str]) -> usize {
    left.iter()
        .rev()
        .zip(right.iter().rev())
        .take_while(|(a, b)| same(a, b))
        .count()
}

fn lcs_diff(left: &[&str], right: &[&str]) -> Vec<DiffSpan> {
    let rows = left.len() + 1;
    let columns = right.len() + 1;
    let mut table = vec![0u32; rows * columns];
    for i in (0..left.len()).rev() {
        for j in (0..right.len()).rev() {
            table[i * columns + j] = if same(left[i], right[j]) {
                table[(i + 1) * columns + j + 1] + 1
            } else {
                table[(i + 1) * columns + j].max(table[i * columns + j + 1])
            };
        }
    }

    let mut spans = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if same(left[i], right[j]) {
            // Показываем вариант Final: он и есть результат прохода.
            spans.push(word_span(DiffOp::Equal, right[j]));
            i += 1;
            j += 1;
        } else if table[(i + 1) * columns + j] >= table[i * columns + j + 1] {
            spans.push(word_span(DiffOp::Removed, left[i]));
            i += 1;
        } else {
            spans.push(word_span(DiffOp::Added, right[j]));
            j += 1;
        }
    }
    push_words(&mut spans, DiffOp::Removed, &left[i..]);
    push_words(&mut spans, DiffOp::Added, &right[j..]);
    spans
}

fn word_span(op: DiffOp, word: &str) -> DiffSpan {
    DiffSpan {
        op,
        text: word.to_string(),
    }
}

fn push_words(spans: &mut Vec<DiffSpan>, op: DiffOp, words: &[&str]) {
    for word in words {
        spans.push(word_span(op, word));
    }
}

/// Склеить соседние слова одной судьбы: UI рисует блоки, не слова.
fn merge_spans(spans: Vec<DiffSpan>) -> Vec<DiffSpan> {
    let mut out: Vec<DiffSpan> = Vec::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if last.op == span.op => {
                last.text.push(' ');
                last.text.push_str(&span.text);
            }
            _ => out.push(span),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(spans: &[DiffSpan]) -> Vec<(DiffOp, &str)> {
        spans
            .iter()
            .map(|span| (span.op, span.text.as_str()))
            .collect()
    }

    #[test]
    fn identical_texts_give_one_equal_span() {
        let spans = diff_words("обсудили биллинг и сроки", "обсудили биллинг и сроки");

        assert_eq!(
            ops(&spans),
            vec![(DiffOp::Equal, "обсудили биллинг и сроки")]
        );
    }

    /// Ровно тот случай, ради которого вкладка Compare и существует:
    /// второй проход исправил слово.
    #[test]
    fn corrected_word_shows_as_removed_and_added() {
        let spans = diff_words("обсудили билинг и сроки", "обсудили биллинг и сроки");

        assert_eq!(
            ops(&spans),
            vec![
                (DiffOp::Equal, "обсудили"),
                (DiffOp::Removed, "билинг"),
                (DiffOp::Added, "биллинг"),
                (DiffOp::Equal, "и сроки"),
            ]
        );
    }

    #[test]
    fn inserted_words_are_added_only() {
        let spans = diff_words("решили сроки", "решили по срокам сроки");

        assert_eq!(
            ops(&spans),
            vec![
                (DiffOp::Equal, "решили"),
                (DiffOp::Added, "по срокам"),
                (DiffOp::Equal, "сроки"),
            ]
        );
    }

    #[test]
    fn dropped_words_are_removed_only() {
        let spans = diff_words("решили э по срокам", "решили по срокам");

        assert_eq!(
            ops(&spans),
            vec![
                (DiffOp::Equal, "решили"),
                (DiffOp::Removed, "э"),
                (DiffOp::Equal, "по срокам"),
            ]
        );
    }

    /// Полировка добавила пунктуацию — это не правка смысла.
    #[test]
    fn punctuation_and_case_changes_are_equal() {
        let spans = diff_words("обсудили биллинг", "Обсудили биллинг.");

        assert_eq!(ops(&spans), vec![(DiffOp::Equal, "Обсудили биллинг.")]);
    }

    #[test]
    fn empty_live_gives_everything_added() {
        let spans = diff_words("", "новый текст");

        assert_eq!(ops(&spans), vec![(DiffOp::Added, "новый текст")]);
    }

    #[test]
    fn empty_final_gives_everything_removed() {
        let spans = diff_words("старый текст", "");

        assert_eq!(ops(&spans), vec![(DiffOp::Removed, "старый текст")]);
    }

    #[test]
    fn both_empty_gives_nothing() {
        assert!(diff_words("", "").is_empty());
    }

    /// Часовой транскрипт не должен положить приложение: выше потолка
    /// отдаём грубый блок вместо таблицы на миллиарды ячеек.
    #[test]
    fn oversized_difference_falls_back_to_coarse_blocks() {
        let live = (0..MAX_LCS_WORDS + 50)
            .map(|i| format!("л{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let final_text = (0..MAX_LCS_WORDS + 50)
            .map(|i| format!("ф{i}"))
            .collect::<Vec<_>>()
            .join(" ");

        let spans = diff_words(&live, &final_text);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].op, DiffOp::Removed);
        assert_eq!(spans[1].op, DiffOp::Added);
    }

    /// Первый участок из трёх подряд несовпадающих, или `None`.
    ///
    /// Прибор для инварианта, на который опирается `term_from_edit`.
    fn first_alternating_triple(spans: &[DiffSpan]) -> Option<usize> {
        (0..spans.len().saturating_sub(2))
            .find(|&i| spans[i..i + 3].iter().all(|span| span.op != DiffOp::Equal))
    }

    /// Проверка прибора: на заведомо плохом наборе он обязан сработать.
    ///
    /// Без этого «ни одной тройки не найдено» ниже значило бы только то,
    /// что искать не умеют.
    #[test]
    fn triple_detector_fires_on_a_known_bad_sequence() {
        let bad = vec![
            word_span(DiffOp::Equal, "начало"),
            word_span(DiffOp::Removed, "а"),
            word_span(DiffOp::Added, "б"),
            word_span(DiffOp::Removed, "в"),
            word_span(DiffOp::Equal, "конец"),
        ];

        assert_eq!(first_alternating_triple(&bad), Some(1));
    }

    /// Инвариант для `term_from_edit`: трёх несовпадающих подряд не бывает.
    ///
    /// Потребитель прыгает через два участка, найдя пару, — и если дифф
    /// когда-нибудь выдаст `Removed, Added, Removed`, он молча проглотит
    /// третий и заведёт термин из половины правки. Перебором, а не
    /// примерами: тройки, если они есть, живут в коротких текстах с
    /// перестановками, а такие в примеры руками не попадают.
    ///
    /// Алфавита из трёх слов и длины до четырёх хватает: LCS смотрит на
    /// равенство слов, а не на их вид, и все формы чередования
    /// укладываются в такие пары (14641 пара, около секунды).
    #[test]
    fn diff_never_emits_three_alternating_spans() {
        let alphabet = ["а", "б", "в"];
        let texts = all_texts(&alphabet, 4);
        let mut with_pair = 0usize;

        for live in &texts {
            for final_text in &texts {
                let spans = diff_words(live, final_text);
                assert_eq!(
                    first_alternating_triple(&spans),
                    None,
                    "три несовпадающих подряд: {live:?} → {final_text:?} дало {spans:?}"
                );
                if spans
                    .windows(2)
                    .any(|pair| pair.iter().all(|span| span.op != DiffOp::Equal))
                {
                    with_pair += 1;
                }
            }
        }

        // Инвариант о том, чего не бывает, выполняется сам собой, если
        // не бывает и пар. Непустоту материала утверждаем отдельно.
        assert!(
            with_pair > 100,
            "пар (Removed, Added) нашлось всего {with_pair} — перебор проходит мимо проверяемого"
        );
    }

    /// Все тексты длиной от нуля до `max_words` из данных слов.
    fn all_texts(alphabet: &[&str], max_words: usize) -> Vec<String> {
        let mut texts = vec![String::new()];
        let mut current = vec![String::new()];
        for _ in 0..max_words {
            let mut next = Vec::new();
            for text in &current {
                for word in alphabet {
                    next.push(if text.is_empty() {
                        (*word).to_string()
                    } else {
                        format!("{text} {word}")
                    });
                }
            }
            texts.extend(next.iter().cloned());
            current = next;
        }
        texts
    }

    /// Общие края отрезаются до LCS, поэтому длинный, но почти одинаковый
    /// текст остаётся в дешёвой ветке.
    #[test]
    fn long_but_similar_texts_still_diff_word_by_word() {
        let head = (0..2000)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let live = format!("{head} билинг конец");
        let final_text = format!("{head} биллинг конец");

        let spans = diff_words(&live, &final_text);

        assert_eq!(
            ops(&spans).last(),
            Some(&(DiffOp::Equal, "конец")),
            "хвост должен остаться общим"
        );
        assert!(
            spans
                .iter()
                .any(|s| s.op == DiffOp::Added && s.text == "биллинг"),
            "{spans:?}"
        );
    }
}
