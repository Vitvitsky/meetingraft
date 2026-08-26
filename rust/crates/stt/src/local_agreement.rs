//! Стабилизация потоковых субтитров через LocalAgreement-2.
//!
//! Whisper не потоковый: чтобы показывать речь по ходу, движок гоняет
//! растущий буфер повторно. Наивно это даёт мерцание — каждая итерация
//! переписывает строку целиком. Приём: держим две последние гипотезы,
//! фиксируем их наибольший общий префикс и объявляем его окончательным, а
//! расходящийся хвост показываем как неустойчивый.
//!
//! Свойство, ради которого всё делается: **зафиксированное не меняется**.
//! Коммитим только то, с чем независимо согласились две итерации подряд,
//! поэтому ошибка модели правится в хвосте, а не задним числом.
//!
//! Модуль намеренно не зависит от Whisper: он работает со словами и
//! временем, а не с моделью, и поэтому тестируется без неё.
//!
//! **Контракт вызывающего.** Гипотеза покрывает только текущий буфер,
//! то есть аудио **после** обрезки. Зафиксированные слова в следующей
//! гипотезе появляться не должны: сопоставление идёт от начала, и
//! повторно пришедший префикс сдвинул бы выравнивание. Цикл движка:
//! `push` → обрезать аудио до `committed_until_ms` → `rebase` на ту же
//! величину → следующий инференс.

/// Слово гипотезы с временем окончания от начала буфера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HypothesisWord {
    pub text: String,
    pub end_ms: u64,
}

impl HypothesisWord {
    pub fn new(text: impl Into<String>, end_ms: u64) -> Self {
        Self {
            text: text.into(),
            end_ms,
        }
    }

    /// Форма для сравнения гипотез: регистр и краевая пунктуация не в счёт.
    ///
    /// Whisper переставляет запятые между итерациями, и посимвольное
    /// сравнение почти никогда не сошлось бы.
    fn normalized(&self) -> String {
        self.text
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    }
}

/// Что делать с текстом после очередной гипотезы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stabilized {
    /// Текст, ставший окончательным на этом шаге; пусто — фиксировать нечего.
    pub committed_text: String,
    /// Неустойчивый хвост, который ещё может измениться.
    pub pending_text: String,
    /// До какого времени буфера можно выбрасывать аудио.
    pub committed_until_ms: Option<u64>,
}

impl Stabilized {
    pub fn is_empty(&self) -> bool {
        self.committed_text.is_empty() && self.pending_text.is_empty()
    }
}

/// Состояние стабилизации одного потока.
pub struct LocalAgreement {
    /// Хвост предыдущей гипотезы (то, что ещё не зафиксировано).
    previous: Vec<HypothesisWord>,
    /// Потолок неустойчивого хвоста: если согласие не наступает, фиксируем
    /// принудительно, иначе латентность уходит в бесконечность.
    max_pending_words: usize,
}

impl LocalAgreement {
    pub fn new(max_pending_words: usize) -> Self {
        Self {
            previous: Vec::new(),
            max_pending_words: max_pending_words.max(1),
        }
    }

    /// Принять новую гипотезу по текущему буферу.
    pub fn push(&mut self, hypothesis: Vec<HypothesisWord>) -> Stabilized {
        if hypothesis.is_empty() {
            self.previous.clear();
            return Stabilized::default();
        }

        let agreed = common_prefix_len(&self.previous, &hypothesis);
        // Принудительная фиксация: хвост разросся, согласия нет.
        let forced = hypothesis.len().saturating_sub(self.max_pending_words);
        let commit_len = agreed.max(forced).min(hypothesis.len());

        let mut committed = hypothesis;
        let pending = committed.split_off(commit_len);

        let committed_until_ms = committed.last().map(|word| word.end_ms);
        let committed_text = join_words(&committed);
        let pending_text = join_words(&pending);

        self.previous = pending;

        Stabilized {
            committed_text,
            pending_text,
            committed_until_ms,
        }
    }

    /// Речь кончилась: контекста больше не будет, фиксируем остаток.
    pub fn flush(&mut self) -> Stabilized {
        let remaining = std::mem::take(&mut self.previous);
        if remaining.is_empty() {
            return Stabilized::default();
        }
        let committed_until_ms = remaining.last().map(|word| word.end_ms);
        let committed_text = join_words(&remaining);
        Stabilized {
            committed_text,
            pending_text: String::new(),
            committed_until_ms,
        }
    }

    /// Буфер обрезали на `by_ms` — сдвинуть время оставшегося хвоста.
    pub fn rebase(&mut self, by_ms: u64) {
        for word in &mut self.previous {
            word.end_ms = word.end_ms.saturating_sub(by_ms);
        }
    }

    /// Новый сегмент речи.
    pub fn reset(&mut self) {
        self.previous.clear();
    }
}

/// Длина наибольшего общего префикса двух гипотез.
fn common_prefix_len(previous: &[HypothesisWord], current: &[HypothesisWord]) -> usize {
    previous
        .iter()
        .zip(current.iter())
        .take_while(|(left, right)| {
            let left = left.normalized();
            !left.is_empty() && left == right.normalized()
        })
        .count()
}

/// Собрать слова из токенов модели.
///
/// Whisper отдаёт subword-токены: новое слово начинается с ведущего
/// пробела, остальные приклеиваются к текущему. Время слова — время его
/// последнего токена.
pub fn words_from_tokens(tokens: &[(String, u64)]) -> Vec<HypothesisWord> {
    let mut words: Vec<HypothesisWord> = Vec::new();
    for (raw, end_ms) in tokens {
        if raw.trim().is_empty() {
            continue;
        }
        let starts_word = raw.starts_with(char::is_whitespace) || words.is_empty();
        if starts_word {
            words.push(HypothesisWord::new(raw.trim_start().to_string(), *end_ms));
        } else if let Some(last) = words.last_mut() {
            last.text.push_str(raw);
            last.end_ms = *end_ms;
        }
    }
    words.retain(|word| !word.text.trim().is_empty());
    words
}

/// Собрать слова из посимвольных токенов.
///
/// GigaAM отдаёт не subword-токены, а буквы: его `tokens.txt` — русский
/// алфавит плюс пробел (id 0) плюс `<blk>`. Слово здесь кончается не
/// ведущим пробелом следующего токена, а **отдельным токеном-пробелом**,
/// и [`words_from_tokens`] на таком входе склеит всю фразу в одно слово:
/// пробел он выбрасывает как пустой, а буква после него не начинается с
/// пробела и приклеивается к предыдущему.
///
/// Время слова — время его последней буквы, как и у соседа.
pub fn words_from_char_tokens(tokens: &[(String, u64)]) -> Vec<HypothesisWord> {
    let mut words: Vec<HypothesisWord> = Vec::new();
    let mut open = false;
    for (raw, end_ms) in tokens {
        if raw.trim().is_empty() {
            // Пробел закрывает слово. Подряд идущих пробелов модель не
            // отдаёт, но пустое слово от них не заводится и так.
            open = false;
            continue;
        }
        if open && let Some(last) = words.last_mut() {
            last.text.push_str(raw);
            last.end_ms = *end_ms;
        } else {
            words.push(HypothesisWord::new(raw.clone(), *end_ms));
            open = true;
        }
    }
    words.retain(|word| !word.text.trim().is_empty());
    words
}

/// Подставить время сегмента словам, которым модель его не дала.
///
/// Whisper заполняет `t1` токенов только когда сам посчитал token-level
/// тайм-коды; на коротких буферах он этого не делает, и тогда все слова
/// приходят с нулём. Без подстановки обрезка буфера молча перестаёт
/// работать — а это весь смысл стабилизации.
pub fn backfill_end_ms(words: &mut [HypothesisWord], fallback_ms: u64) {
    let mut last = 0;
    for word in words.iter_mut() {
        if word.end_ms == 0 {
            word.end_ms = fallback_ms.max(last);
        }
        // Время не может идти назад: у монотонности зависит корректность
        // обрезки.
        word.end_ms = word.end_ms.max(last);
        last = word.end_ms;
    }
}

fn join_words(words: &[HypothesisWord]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(pairs: &[(&str, u64)]) -> Vec<HypothesisWord> {
        pairs
            .iter()
            .map(|(text, end_ms)| HypothesisWord::new(*text, *end_ms))
            .collect()
    }

    #[test]
    fn tokens_merge_into_words_by_leading_space() {
        let tokens = vec![
            ("Об".to_string(), 100),
            ("судим".to_string(), 200),
            (" билл".to_string(), 400),
            ("инг".to_string(), 500),
        ];

        let words = words_from_tokens(&tokens);

        assert_eq!(
            words,
            vec![
                HypothesisWord::new("Обсудим", 200),
                HypothesisWord::new("биллинг", 500),
            ]
        );
    }

    #[test]
    fn empty_and_whitespace_tokens_are_dropped() {
        let tokens = vec![
            ("".to_string(), 100),
            ("   ".to_string(), 150),
            (" да".to_string(), 200),
        ];

        assert_eq!(
            words_from_tokens(&tokens),
            vec![HypothesisWord::new("да", 200)]
        );
    }

    #[test]
    fn no_tokens_give_no_words() {
        assert!(words_from_tokens(&[]).is_empty());
    }

    #[test]
    fn backfill_replaces_missing_times_with_segment_end() {
        let mut words = vec![HypothesisWord::new("раз", 0), HypothesisWord::new("два", 0)];

        backfill_end_ms(&mut words, 900);

        assert_eq!(words[0].end_ms, 900);
        assert_eq!(words[1].end_ms, 900);
    }

    #[test]
    fn backfill_keeps_known_times_and_enforces_monotonicity() {
        let mut words = vec![
            HypothesisWord::new("раз", 400),
            HypothesisWord::new("два", 0),
            // Модель дала время назад — оно подтягивается вперёд.
            HypothesisWord::new("три", 200),
        ];

        backfill_end_ms(&mut words, 700);

        assert_eq!(words[0].end_ms, 400);
        assert_eq!(words[1].end_ms, 700);
        assert_eq!(words[2].end_ms, 700);
    }

    #[test]
    fn backfill_on_empty_slice_is_noop() {
        let mut words: Vec<HypothesisWord> = Vec::new();
        backfill_end_ms(&mut words, 500);
        assert!(words.is_empty());
    }

    #[test]
    fn first_hypothesis_commits_nothing() {
        let mut agreement = LocalAgreement::new(16);

        let result = agreement.push(words(&[("привет", 300), ("команда", 700)]));

        assert!(result.committed_text.is_empty(), "сравнивать ещё не с чем");
        assert_eq!(result.pending_text, "привет команда");
        assert_eq!(result.committed_until_ms, None);
    }

    #[test]
    fn agreeing_prefix_becomes_final() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("обсудим", 300), ("билинг", 700)]));

        let result = agreement.push(words(&[
            ("обсудим", 300),
            ("биллинг", 700),
            ("сегодня", 1100),
        ]));

        // Согласились только на первом слове — второе модель исправила.
        assert_eq!(result.committed_text, "обсудим");
        assert_eq!(result.pending_text, "биллинг сегодня");
        assert_eq!(result.committed_until_ms, Some(300));
    }

    /// Ровно тот случай, ради которого приём и нужен: модель ошиблась в
    /// последнем слове и через итерацию исправила его, ничего не переписав
    /// в уже зафиксированном. Заодно моделирует полный цикл с обрезкой.
    #[test]
    fn corrected_word_never_reaches_committed_text() {
        let mut agreement = LocalAgreement::new(16);

        // Итерация 1: модель услышала «файлы».
        agreement.push(words(&[("отправь", 300), ("файлы", 700)]));

        // Итерация 2: с новым контекстом исправила на «файл». Согласие
        // есть только на первом слове — его и фиксируем.
        let first = agreement.push(words(&[("отправь", 300), ("файл", 700)]));
        assert_eq!(first.committed_text, "отправь");
        assert_eq!(first.committed_until_ms, Some(300));

        // Движок обрезал аудио до 300 мс и сдвинул время хвоста.
        agreement.rebase(300);

        // Итерация 3: буфер начинается уже после «отправь».
        let second = agreement.push(words(&[("файл", 400), ("позже", 800)]));

        assert_eq!(second.committed_text, "файл");
        assert_eq!(second.pending_text, "позже");
        assert!(!first.committed_text.contains("файлы"));
        assert!(!second.committed_text.contains("файлы"));
    }

    #[test]
    fn punctuation_and_case_do_not_break_agreement() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("Привет", 300), ("команда", 700)]));

        let result = agreement.push(words(&[
            ("привет,", 300),
            ("команда.", 700),
            ("начнём", 1100),
        ]));

        assert_eq!(result.committed_text, "привет, команда.");
        assert_eq!(result.pending_text, "начнём");
    }

    #[test]
    fn divergence_in_the_middle_stops_commit_there() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("раз", 100), ("два", 200), ("три", 300)]));

        let result = agreement.push(words(&[("раз", 100), ("две", 200), ("три", 300)]));

        assert_eq!(result.committed_text, "раз");
        assert_eq!(result.pending_text, "две три");
    }

    /// Без потолка неустойчивый хвост рос бы бесконечно на шумной речи.
    #[test]
    fn pending_tail_is_force_committed_at_the_ceiling() {
        let mut agreement = LocalAgreement::new(2);
        agreement.push(words(&[("а", 100)]));

        let result = agreement.push(words(&[("б", 100), ("в", 200), ("г", 300), ("д", 400)]));

        // Согласия нет ни на одном слове, но хвост длиннее потолка.
        assert_eq!(result.committed_text, "б в");
        assert_eq!(result.pending_text, "г д");
        assert_eq!(result.committed_until_ms, Some(200));
    }

    #[test]
    fn flush_commits_the_remaining_tail() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("почти", 300), ("всё", 700)]));

        let result = agreement.flush();

        assert_eq!(result.committed_text, "почти всё");
        assert!(result.pending_text.is_empty());
        assert_eq!(result.committed_until_ms, Some(700));
        assert!(agreement.flush().is_empty(), "повторный flush пуст");
    }

    #[test]
    fn empty_hypothesis_clears_pending() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("шум", 100)]));

        let result = agreement.push(Vec::new());

        assert!(result.is_empty());
        assert!(agreement.flush().is_empty());
    }

    /// После обрезки буфера время хвоста должно уехать вместе с ним.
    #[test]
    fn rebase_shifts_pending_timestamps() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("раз", 500), ("два", 900)]));
        let committed = agreement.push(words(&[("раз", 500), ("два", 900), ("три", 1300)]));
        assert_eq!(committed.committed_until_ms, Some(900));

        agreement.rebase(900);
        let after = agreement.push(words(&[("три", 400), ("четыре", 800)]));

        assert_eq!(after.committed_text, "три");
        assert_eq!(after.committed_until_ms, Some(400));
    }

    /// Посимвольные токены: слово кончается токеном-пробелом.
    #[test]
    fn char_tokens_split_on_the_space_token() {
        let tokens = vec![
            ("с".to_string(), 100),
            ("л".to_string(), 150),
            ("о".to_string(), 200),
            (" ".to_string(), 220),
            ("в".to_string(), 300),
            ("о".to_string(), 380),
        ];

        let words = words_from_char_tokens(&tokens);

        assert_eq!(words.len(), 2, "слов должно быть два: {words:?}");
        assert_eq!(words[0].text, "сло");
        assert_eq!(words[0].end_ms, 200, "время слова — время последней буквы");
        assert_eq!(words[1].text, "во");
        assert_eq!(words[1].end_ms, 380);
    }

    /// А вот зачем эта функция вообще заведена: соседняя, рассчитанная на
    /// subword-токены Whisper, на том же входе даёт **одно** слово.
    ///
    /// Без этой проверки `words_from_char_tokens` выглядит дублем и
    /// однажды будет выброшена как лишняя — а разница между ними молчит:
    /// склеенная в одно слово фраза выглядит как плохое распознавание,
    /// а не как ошибка сборки слов.
    #[test]
    fn the_whisper_reader_glues_the_same_char_tokens_into_one_word() {
        let tokens = vec![
            ("с".to_string(), 100),
            ("л".to_string(), 150),
            ("о".to_string(), 200),
            (" ".to_string(), 220),
            ("в".to_string(), 300),
            ("о".to_string(), 380),
        ];

        let glued = words_from_tokens(&tokens);

        assert_eq!(glued.len(), 1, "ожидалась склейка: {glued:?}");
        assert_eq!(glued[0].text, "слово");
    }

    /// Ведущий и хвостовой пробелы пустых слов не заводят.
    #[test]
    fn leading_and_trailing_space_tokens_add_no_empty_words() {
        let tokens = vec![
            (" ".to_string(), 10),
            ("д".to_string(), 100),
            ("а".to_string(), 140),
            (" ".to_string(), 160),
        ];

        let words = words_from_char_tokens(&tokens);

        assert_eq!(words.len(), 1, "{words:?}");
        assert_eq!(words[0].text, "да");
    }

    #[test]
    fn reset_drops_pending_of_the_previous_segment() {
        let mut agreement = LocalAgreement::new(16);
        agreement.push(words(&[("старое", 100), ("ещё", 200)]));

        agreement.reset();

        assert!(agreement.flush().is_empty());
    }
}
