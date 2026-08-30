//! Слепое парное сравнение двух прогонов и проверка самого судьи.
//!
//! Судья читает два куска расшифровки одного и того же отрезка записи,
//! **без имён движков**, и говорит, какой связнее. Порядок сторон
//! случаен и задаётся зерном прогона.
//!
//! ## Что он мерит и чего не мерит
//!
//! **Связность, а не верность.** Он не слышал звук. Гладкая выдумка
//! выиграет у корявой правды, и это не дефект промпта, а свойство
//! задачи. Поэтому там, где есть эталон, решает WER, а судья объясняет,
//! *чем* одна расшифровка хуже другой; при расхождении отчёт печатает
//! расхождение, а не среднее.
//!
//! ## Три контроля, без которых его вывод ничего не значит
//!
//! Идут в каждом прогоне вперемешку с настоящими парами:
//!
//! 1. **A против A** — один и тот же текст обеими сторонами. Правильный
//!    ответ «ничья». Уверенный победитель означает, что судья выбирает
//!    позицию, а не текст, и весь прогон недействителен.
//! 2. **Перемешанный против исходного** — слова внутри куска
//!    переставлены зерном. Судья обязан выбрать исходный. Не выбрал —
//!    он слеп, и его мнение о двух приличных расшифровках тем более
//!    ничего не стоит.
//! 3. **Доля отказов** на настоящих парах печатается рядом: судья,
//!    у которого нет права сказать «ничья», всегда называет победителя.
//!
//! Не прошли контроли — результаты сравнения **не показываются вовсе**.
//! Показать их с оговоркой хуже: оговорку прочтут один раз, а число
//! запомнят.

use serde::{Deserialize, Serialize};

/// Кто победил.
///
/// Регистр принимается любой, и это не вежливость к модели, а **замер**:
/// промпт просит `"A"`, а разбор без псевдонимов принимал только `"a"` —
/// то есть каждый ответ судьи стал бы ошибкой, и прогон отказал бы
/// целиком с формулировкой про сломанный разбор. Поймал это тест, а
/// вылезло бы на первой живой Ollama.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Winner {
    #[serde(alias = "A")]
    A,
    #[serde(alias = "B")]
    B,
    #[serde(alias = "TIE", alias = "Tie")]
    Tie,
}

/// Ответ судьи по одной паре.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub winner: Winner,
    pub reason: String,
}

/// Зачем эта пара в прогоне.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
    /// Настоящее сравнение двух движков.
    Real,
    /// Контроль: обе стороны — один и тот же текст.
    SameText,
    /// Контроль: одна сторона перемешана.
    Shuffled,
}

/// Сторона пары.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Side {
    Left,
    Right,
}

/// Пара на суд.
#[derive(Debug, Clone)]
pub struct Pair {
    pub kind: Kind,
    pub left: String,
    pub right: String,
    /// Слева стоит первый прогон. Нужно, чтобы вернуть победу движку
    /// после того, как стороны переставили.
    pub left_is_first: bool,
    /// Где исходный текст в контроле с перемешиванием.
    pub original: Option<Side>,
}

/// Судья. Видит **только два текста** — ни вида пары, ни ожидаемого
/// ответа.
///
/// Слепота здесь не удобство подписи, а условие: судья, которому видно,
/// что перед ним контроль, проходит контроль по построению.
pub trait Judge {
    fn compare(&self, left: &str, right: &str) -> Result<Verdict, String>;
}

/// Порог контроля A/A: доля «ничья» на одинаковых текстах.
pub const SAME_TEXT_MIN_TIE: f32 = 0.8;
/// Порог контроля с перемешиванием: доля верных ответов.
pub const SHUFFLED_MIN_CORRECT: f32 = 0.9;

/// Что показал прогон судьи.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JudgeReport {
    /// Причина, по которой смотреть на результаты нельзя.
    pub refused: Option<String>,
    /// Победы первого прогона на настоящих парах.
    pub first_wins: usize,
    pub second_wins: usize,
    pub ties: usize,
    /// Пары, на которых судья не ответил: не отозвался транспорт либо
    /// ответ не разобран.
    ///
    /// Это **не ничья**: недоступная модель и осознанное «не могу
    /// выбрать» — разные вещи, и слить их значило бы спрятать поломку в
    /// результат.
    pub errors: usize,
    /// Доля «ничья» на контроле A/A.
    pub same_text_tie_share: f32,
    /// Доля верных ответов на контроле с перемешиванием.
    pub shuffled_correct_share: f32,
    pub same_text_pairs: usize,
    pub shuffled_pairs: usize,
}

impl JudgeReport {
    /// Доля «ничья» на настоящих парах. Ноль подозрителен: судья без
    /// права отказаться всегда называет победителя.
    pub fn real_tie_share(&self) -> f32 {
        let total = self.first_wins + self.second_wins + self.ties;
        if total == 0 {
            0.0
        } else {
            self.ties as f32 / total as f32
        }
    }
}

/// Прогнать пары через судью и проверить его самого.
pub fn evaluate(judge: &dyn Judge, pairs: &[Pair]) -> JudgeReport {
    let mut first_wins = 0usize;
    let mut second_wins = 0usize;
    let mut ties = 0usize;
    let mut errors = 0usize;

    let mut same_text_total = 0usize;
    let mut same_text_ties = 0usize;
    let mut shuffled_total = 0usize;
    let mut shuffled_correct = 0usize;

    for pair in pairs {
        let verdict = match judge.compare(&pair.left, &pair.right) {
            Ok(verdict) => verdict,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        match pair.kind {
            Kind::SameText => {
                same_text_total += 1;
                if verdict.winner == Winner::Tie {
                    same_text_ties += 1;
                }
            }
            Kind::Shuffled => {
                shuffled_total += 1;
                let picked = match verdict.winner {
                    Winner::A => Some(Side::Left),
                    Winner::B => Some(Side::Right),
                    Winner::Tie => None,
                };
                if picked.is_some() && picked == pair.original {
                    shuffled_correct += 1;
                }
            }
            Kind::Real => match verdict.winner {
                Winner::Tie => ties += 1,
                Winner::A if pair.left_is_first => first_wins += 1,
                Winner::A => second_wins += 1,
                Winner::B if pair.left_is_first => second_wins += 1,
                Winner::B => first_wins += 1,
            },
        }
    }

    let same_text_tie_share = share(same_text_ties, same_text_total);
    let shuffled_correct_share = share(shuffled_correct, shuffled_total);

    let refused = refusal(
        same_text_total,
        same_text_tie_share,
        shuffled_total,
        shuffled_correct_share,
    );

    JudgeReport {
        refused,
        first_wins,
        second_wins,
        ties,
        errors,
        same_text_tie_share,
        shuffled_correct_share,
        same_text_pairs: same_text_total,
        shuffled_pairs: shuffled_total,
    }
}

/// Можно ли верить этому судье.
fn refusal(
    same_text_pairs: usize,
    same_text_tie_share: f32,
    shuffled_pairs: usize,
    shuffled_correct_share: f32,
) -> Option<String> {
    // Отсутствие контролей — тоже отказ. Прогон без них выглядит
    // безупречно и не значит ничего: именно так проверка проходит на
    // пустом входе.
    if same_text_pairs == 0 || shuffled_pairs == 0 {
        return Some(format!(
            "контролей не было: A/A — {same_text_pairs}, перемешанных — {shuffled_pairs}. \
             Прогон без контролей ничего не утверждает"
        ));
    }
    if same_text_tie_share < SAME_TEXT_MIN_TIE {
        return Some(format!(
            "контроль A/A: доля «ничья» {same_text_tie_share:.2} при пороге \
             {SAME_TEXT_MIN_TIE:.2}. Судья выбирает позицию, а не текст"
        ));
    }
    if shuffled_correct_share < SHUFFLED_MIN_CORRECT {
        return Some(format!(
            "контроль с перемешиванием: верных {shuffled_correct_share:.2} при пороге \
             {SHUFFLED_MIN_CORRECT:.2}. Судья не отличает связный текст от рассыпанного"
        ));
    }
    None
}

fn share(part: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        part as f32 / total as f32
    }
}

/// Кого предпочёл каждый из двух способов судить.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divergence {
    /// Судья выбрал первый прогон.
    pub judge_prefers_first: bool,
    /// Эталон выбрал первый прогон.
    pub reference_prefers_first: bool,
}

/// Разошлись ли судья и эталон.
///
/// `None` означает «расхождения нет» — включая случаи, когда сравнивать
/// нечем: ничья у судьи, равный WER, отсутствие эталона. Молча назвать
/// такое согласием было бы неправдой в обе стороны.
///
/// **Решает эталон.** Судья мерит связность и не слышал звук: гладкая
/// выдумка выигрывает у корявой правды. Поэтому расхождение печатается
/// расхождением, а не усредняется.
pub fn divergence(
    first_wins: usize,
    second_wins: usize,
    first_wer: Option<f32>,
    second_wer: Option<f32>,
) -> Option<Divergence> {
    let (Some(first_wer), Some(second_wer)) = (first_wer, second_wer) else {
        return None;
    };
    if first_wins == second_wins || (first_wer - second_wer).abs() <= f32::EPSILON {
        return None;
    }
    let judge_prefers_first = first_wins > second_wins;
    let reference_prefers_first = first_wer < second_wer;
    if judge_prefers_first == reference_prefers_first {
        return None;
    }
    Some(Divergence {
        judge_prefers_first,
        reference_prefers_first,
    })
}

/// Ширина куска, на которые режутся расшифровки перед сравнением, мс.
///
/// Минута — столько, сколько человек может держать в голове, сравнивая
/// два текста. Целая встреча одной парой дала бы ответ «второй лучше» без
/// возможности сказать, где именно.
pub const CHUNK_MS: u64 = 60_000;

/// Собрать пары из двух прогонов: настоящие плюс контроли.
///
/// Контроли строятся **из того же материала**, что и настоящие пары, а не
/// из отдельной фикстуры: судья, которому контроли подсунули на чужом
/// тексте, проверен не на той задаче, которую решает.
pub fn build_pairs(
    first: &[(u64, String)],
    second: &[(u64, String)],
    audio_ms: u64,
    seed: u64,
) -> Vec<Pair> {
    let mut pairs = Vec::new();
    let mut state = seed | 1;
    let mut next_bit = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) & 1 == 1
    };

    let mut start = 0;
    while start < audio_ms {
        let end = start + CHUNK_MS;
        let left_text = join_chunk(first, start, end);
        let right_text = join_chunk(second, start, end);
        if !left_text.is_empty() && !right_text.is_empty() {
            // Сторону выбирает зерно: судья не должен угадывать движок по
            // тому, что тот всегда слева.
            let swap = next_bit();
            pairs.push(Pair {
                kind: Kind::Real,
                left: if swap {
                    right_text.clone()
                } else {
                    left_text.clone()
                },
                right: if swap {
                    left_text.clone()
                } else {
                    right_text.clone()
                },
                left_is_first: !swap,
                original: None,
            });
        }
        // Контроли — на тексте первого прогона того же отрезка.
        if !left_text.is_empty() {
            pairs.push(Pair {
                kind: Kind::SameText,
                left: left_text.clone(),
                right: left_text.clone(),
                left_is_first: true,
                original: None,
            });
            if shufflable(&left_text) {
                let shuffled = shuffle_words(&left_text, seed.wrapping_add(start));
                let swap = next_bit();
                pairs.push(Pair {
                    kind: Kind::Shuffled,
                    left: if swap {
                        shuffled.clone()
                    } else {
                        left_text.clone()
                    },
                    right: if swap { left_text.clone() } else { shuffled },
                    left_is_first: true,
                    original: Some(if swap { Side::Right } else { Side::Left }),
                });
            }
        }
        start = end;
    }
    pairs
}

/// Склеить текст прогона внутри отрезка.
fn join_chunk(segments: &[(u64, String)], from: u64, to: u64) -> String {
    segments
        .iter()
        .filter(|(start, _)| *start >= from && *start < to)
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Перемешать слова повторяемо.
///
/// Своё зерно, а не `rand`: контроль сегодня и завтра должен быть одним и
/// тем же случаем.
pub fn shuffle_words(text: &str, seed: u64) -> String {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 4 {
        // Три слова переставить так, чтобы вышло заметно бессвязнее,
        // нельзя. Такой кусок в контроль не годится, и молча подсунуть
        // его значило бы завысить оценку судьи.
        return text.to_string();
    }
    let mut state = seed | 1;
    for index in (1..words.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let pick = (state >> 33) as usize % (index + 1);
        words.swap(index, pick);
    }
    words.join(" ")
}

/// Годится ли кусок для контроля с перемешиванием.
pub fn shufflable(text: &str) -> bool {
    text.split_whitespace().count() >= 4
}

#[cfg(feature = "judge")]
/// Судья на локальной LLM.
///
/// Промпт требует **строгого** ответа, и неразобранный ответ едет
/// ошибкой, а не «ничьей»: сломанный разбор и осознанный отказ выбрать —
/// разные вещи, и слив их, мы спрятали бы поломку в результат.
pub struct LlmJudge<'a> {
    pub client: &'a dyn postcall::LlmClient,
}

#[cfg(feature = "judge")]
const SYSTEM: &str = "\
Ты сравниваешь две расшифровки одной и той же записи разговора. \
Обе сделаны машиной, имена систем тебе не сообщают. \
Оцени только связность и читаемость: целые ли фразы, на месте ли слова, \
нет ли обрывов и повторов. Ты не слышал запись — не суди о том, что в \
ней было сказано на самом деле. \
Если тексты неразличимы по связности, отвечай tie — это законный ответ. \
Ответь строго одним объектом JSON и ничем больше: \
{\"winner\": \"A\" | \"B\" | \"tie\", \"reason\": \"одна фраза\"}";

#[cfg(feature = "judge")]
impl LlmJudge<'_> {
    /// Разобрать ответ модели.
    ///
    /// Отдельной функцией, чтобы разбор проверялся тестом без LLM: он
    /// ломается первым, а чинится реже всего.
    pub fn parse(answer: &str) -> Result<Verdict, String> {
        // Модель почти всегда добавляет что-нибудь вокруг JSON, и
        // требовать чистоты от неё дороже, чем вырезать объект самим.
        let start = answer
            .find('{')
            .ok_or_else(|| format!("в ответе нет JSON: {answer}"))?;
        let end = answer
            .rfind('}')
            .ok_or_else(|| format!("в ответе нет JSON: {answer}"))?;
        if end <= start {
            return Err(format!("в ответе нет JSON: {answer}"));
        }
        serde_json::from_str::<Verdict>(&answer[start..=end])
            .map_err(|error| format!("ответ не разобран ({error}): {answer}"))
    }
}

#[cfg(feature = "judge")]
impl Judge for LlmJudge<'_> {
    fn compare(&self, left: &str, right: &str) -> Result<Verdict, String> {
        let user = format!("A:\n{left}\n\nB:\n{right}");
        let answer = self
            .client
            .complete(SYSTEM, &user)
            .map_err(|error| error.to_string())?;
        Self::parse(&answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HONEST: &str = "мы вынесли это в общий крейт и договорились о границе";

    fn pair(kind: Kind, left: &str, right: &str, original: Option<Side>) -> Pair {
        Pair {
            kind,
            left: left.to_string(),
            right: right.to_string(),
            left_is_first: true,
            original,
        }
    }

    /// Набор контролей: один A/A и один с перемешиванием.
    fn control_pairs() -> Vec<Pair> {
        let shuffled = shuffle_words(HONEST, 42);
        vec![
            pair(Kind::SameText, HONEST, HONEST, None),
            pair(Kind::Shuffled, HONEST, &shuffled, Some(Side::Left)),
        ]
    }

    struct AlwaysA;
    impl Judge for AlwaysA {
        fn compare(&self, _left: &str, _right: &str) -> Result<Verdict, String> {
            Ok(Verdict {
                winner: Winner::A,
                reason: "всегда левый".to_string(),
            })
        }
    }

    struct AlwaysTie;
    impl Judge for AlwaysTie {
        fn compare(&self, _left: &str, _right: &str) -> Result<Verdict, String> {
            Ok(Verdict {
                winner: Winner::Tie,
                reason: "всегда ничья".to_string(),
            })
        }
    }

    /// Судья, отвечающий верно. Слеп к виду пары, как и настоящий:
    /// узнаёт исходный текст, а не читает разметку.
    struct Honest;
    impl Judge for Honest {
        fn compare(&self, left: &str, right: &str) -> Result<Verdict, String> {
            if left == right {
                return Ok(Verdict {
                    winner: Winner::Tie,
                    reason: "тексты совпадают".to_string(),
                });
            }
            let winner = if left == HONEST { Winner::A } else { Winner::B };
            Ok(Verdict {
                winner,
                reason: "связнее".to_string(),
            })
        }
    }

    struct Broken;
    impl Judge for Broken {
        fn compare(&self, _left: &str, _right: &str) -> Result<Verdict, String> {
            Err("ответ не разобран".to_string())
        }
    }

    /// Судья, всегда выбирающий левое, ловится контролем A/A.
    #[test]
    fn a_judge_that_always_says_a_fails_the_same_text_control() {
        let report = evaluate(&AlwaysA, &control_pairs());
        let refused = report.refused.expect("слепой судья обязан быть отвергнут");
        assert!(refused.contains("A/A"), "{refused}");
    }

    /// Судья, всегда отвечающий «ничья», ловится контролем с
    /// перемешиванием — а контроль A/A он проходит.
    ///
    /// Пара к предыдущему тесту: поодиночке каждый контроль пропускает
    /// своего слепца.
    #[test]
    fn a_judge_that_always_ties_fails_the_shuffled_control() {
        let report = evaluate(&AlwaysTie, &control_pairs());
        let refused = report.refused.expect("обязан быть отвергнут");
        assert!(refused.contains("перемешив"), "{refused}");
        assert_eq!(report.same_text_tie_share, 1.0, "контроль A/A он прошёл");
    }

    /// А верный судья проходит оба — иначе контроль отвергал бы всех
    /// подряд и не значил бы ничего.
    #[test]
    fn an_honest_judge_passes_both_controls() {
        let report = evaluate(&Honest, &control_pairs());
        assert!(report.refused.is_none(), "{:?}", report.refused);
        assert_eq!(report.same_text_tie_share, 1.0);
        assert_eq!(report.shuffled_correct_share, 1.0);
    }

    /// Прогон **без контролей** — отказ, а не безупречный результат.
    ///
    /// Тот самый случай, который проходит на пустом входе: ни одного
    /// контроля не провалено, потому что ни одного и не было.
    #[test]
    fn a_run_without_controls_is_refused() {
        let pairs = vec![pair(Kind::Real, "первый текст", "второй текст", None)];
        let report = evaluate(&Honest, &pairs);
        let refused = report
            .refused
            .expect("прогон без контролей ничего не значит");
        assert!(refused.contains("контролей не было"), "{refused}");
    }

    /// Неразобранный ответ — ошибка пары, а не ничья.
    #[test]
    fn an_unparsed_answer_is_an_error_not_a_tie() {
        let mut pairs = control_pairs();
        pairs.push(pair(Kind::Real, "первый", "второй", None));
        let report = evaluate(&Broken, &pairs);
        assert_eq!(report.errors, 3);
        assert_eq!(report.ties, 0, "ошибка не должна выглядеть как ничья");
    }

    /// Победа возвращается тому прогону, который стоял на этой стороне,
    /// а не стороне.
    #[test]
    fn a_win_goes_to_the_run_not_to_the_side() {
        let mut swapped = pair(Kind::Real, "левый", "правый", None);
        swapped.left_is_first = false;
        let mut pairs = control_pairs();
        pairs.push(swapped);

        // `AlwaysA` выбирает левое; слева стоит **второй** прогон.
        let report = evaluate(&AlwaysA, &pairs);
        assert_eq!(report.second_wins, 1, "{report:?}");
        assert_eq!(report.first_wins, 0);
    }

    /// Перемешивание меняет порядок слов и сохраняет их состав.
    #[test]
    fn shuffling_keeps_the_words_and_changes_their_order() {
        let shuffled = shuffle_words(HONEST, 7);
        assert_ne!(shuffled, HONEST, "порядок обязан измениться");

        let mut before: Vec<&str> = HONEST.split_whitespace().collect();
        let mut after: Vec<&str> = shuffled.split_whitespace().collect();
        before.sort_unstable();
        after.sort_unstable();
        assert_eq!(before, after, "слова обязаны остаться те же");
    }

    /// Короткий кусок не перемешивается: контроль на нём был бы завышен.
    #[test]
    fn a_short_chunk_is_not_shuffled() {
        assert_eq!(shuffle_words("два слова", 1), "два слова");
        assert!(!shufflable("два слова"));
        assert!(shufflable("уже целых четыре слова"));
    }

    /// Ответ модели разбирается вместе с обёрткой вокруг JSON.
    #[cfg(feature = "judge")]
    #[test]
    fn a_verdict_is_parsed_out_of_the_models_chatter() {
        let verdict = LlmJudge::parse(
            "Конечно! Вот мой ответ:\n{\"winner\": \"B\", \"reason\": \"целые фразы\"}\nГотово.",
        )
        .expect("разобралось");
        assert_eq!(verdict.winner, Winner::B);
        assert_eq!(verdict.reason, "целые фразы");
    }

    /// Регистр победителя модели безразличен: промпт просит заглавные,
    /// и принимать надо оба написания.
    #[cfg(feature = "judge")]
    #[test]
    fn the_winner_is_parsed_in_either_case() {
        for answer in [
            r#"{"winner": "A", "reason": "x"}"#,
            r#"{"winner": "a", "reason": "x"}"#,
        ] {
            assert_eq!(
                LlmJudge::parse(answer).expect("разобралось").winner,
                Winner::A,
                "{answer}"
            );
        }
        assert_eq!(
            LlmJudge::parse(r#"{"winner": "Tie", "reason": "x"}"#)
                .expect("разобралось")
                .winner,
            Winner::Tie
        );
    }

    /// А ответ без JSON — ошибка, а не «ничья».
    #[cfg(feature = "judge")]
    #[test]
    fn an_answer_without_json_is_an_error() {
        assert!(LlmJudge::parse("думаю, второй лучше").is_err());
        assert!(LlmJudge::parse("").is_err());
    }

    /// Чужое значение победителя не превращается молча в ничью.
    #[cfg(feature = "judge")]
    #[test]
    fn an_unknown_winner_value_is_refused() {
        assert!(LlmJudge::parse(r#"{"winner": "оба", "reason": "не знаю"}"#).is_err());
    }

    /// В каждом отрезке появляются оба контроля, а стороны переставляются
    /// зерном.
    #[test]
    fn every_chunk_brings_its_own_controls() {
        let first = vec![(0u64, "мы вынесли это в общий крейт".to_string())];
        let second = vec![(0u64, "мы вынесли это в общий край".to_string())];
        let pairs = build_pairs(&first, &second, 30_000, 1);

        assert_eq!(pairs.iter().filter(|p| p.kind == Kind::Real).count(), 1);
        assert_eq!(pairs.iter().filter(|p| p.kind == Kind::SameText).count(), 1);
        assert_eq!(pairs.iter().filter(|p| p.kind == Kind::Shuffled).count(), 1);

        let shuffled = pairs.iter().find(|p| p.kind == Kind::Shuffled).unwrap();
        assert!(shuffled.original.is_some(), "у контроля должна быть правда");
    }

    /// Пустая сторона настоящей пары не даёт: сравнивать текст с пустотой
    /// значит спрашивать судью о том, чего он не решает.
    #[test]
    fn an_empty_side_makes_no_real_pair() {
        let first = vec![(0u64, "хоть что-то сказано".to_string())];
        let pairs = build_pairs(&first, &[], 30_000, 1);
        assert_eq!(pairs.iter().filter(|p| p.kind == Kind::Real).count(), 0);
        assert!(
            pairs.iter().any(|p| p.kind == Kind::SameText),
            "контроли при этом остаются"
        );
    }

    /// Судья и эталон разошлись — и это видно.
    #[test]
    fn a_disagreement_between_judge_and_reference_is_reported() {
        // Судья выбрал первый, эталон — второй (у него WER меньше).
        let found = divergence(3, 1, Some(0.20), Some(0.05)).expect("расхождение есть");
        assert!(found.judge_prefers_first);
        assert!(!found.reference_prefers_first);
    }

    /// А согласие расхождением не объявляется.
    ///
    /// Пара к предыдущему: проверка, срабатывающая всегда, прошла бы тот
    /// тест и не значила бы ничего.
    #[test]
    fn agreement_is_not_reported_as_a_disagreement() {
        assert_eq!(divergence(3, 1, Some(0.05), Some(0.20)), None);
    }

    /// Сравнивать нечем — значит расхождения нет, а не есть.
    #[test]
    fn nothing_to_compare_is_not_a_disagreement() {
        assert_eq!(
            divergence(2, 2, Some(0.1), Some(0.2)),
            None,
            "ничья у судьи"
        );
        assert_eq!(divergence(3, 1, Some(0.1), Some(0.1)), None, "равный WER");
        assert_eq!(divergence(3, 1, None, Some(0.2)), None, "эталона нет");
    }

    /// Одно зерно — один и тот же контроль. Иначе прогон сегодня и
    /// завтра сравнивают разное.
    #[test]
    fn the_same_seed_gives_the_same_shuffle() {
        assert_eq!(shuffle_words(HONEST, 3), shuffle_words(HONEST, 3));
        assert_ne!(shuffle_words(HONEST, 3), shuffle_words(HONEST, 4));
    }
}
