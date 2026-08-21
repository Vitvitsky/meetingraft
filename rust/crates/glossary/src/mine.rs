//! Добыча кандидатов в термины из готовой расшифровки.
//!
//! Правды о том, как слово пишется на самом деле, здесь нет и быть не
//! может: на входе только распознанное. Поэтому кандидат — подсказка, а
//! не замена, и решение всегда за человеком.
//!
//! Опора для «редкого» — частотная верхушка самой встречи, а не список
//! стоп-слов: в связном тексте верхние токены служебные по построению.
//! Это лучше угаданного списка и всё равно не безупречно, отчего правило
//! `Repeated` и меряется прибором отдельно от двух остальных.

use std::collections::{HashMap, HashSet};

use domain::{CandidateExample, CandidateRule, GlossaryTerm, TermCandidate};

/// Вход добычи.
///
/// Реплики приходят слайсом пар, а не типом из `postcall`: `glossary`
/// зависит только от `domain`, и обратная зависимость сломала бы граф.
pub struct MineInput<'a> {
    /// `(начало реплики в мс, текст)`.
    pub replicas: &'a [(u64, &'a str)],
    /// Уже заведённые термины: их не предлагают.
    pub known: &'a [GlossaryTerm],
    /// Отклонённые человеком.
    pub dismissed: &'a [String],
    /// Сколько раз слово должно прозвучать, чтобы пройти по правилу
    /// `Repeated`. На `Latin` и `Acronym` не влияет: там говорит форма.
    pub min_occurrences: u32,
    /// Размер частотной верхушки, которая правилом `Repeated` не
    /// рассматривается вовсе.
    pub frequent_head: usize,
}

/// Минимальная длина токена, который вообще рассматривается.
const MIN_TOKEN_CHARS: usize = 2;
/// Минимальная длина для правила `Repeated`: короткие слова служебные
/// почти всегда, а частотная верхушка ловит не все из них.
const MIN_REPEATED_CHARS: usize = 4;
/// Примеров на кандидата.
const MAX_EXAMPLES: usize = 2;

#[derive(Default)]
struct Occurrence {
    total: u32,
    /// Написания и сколько раз каждое встретилось.
    spellings: HashMap<String, u32>,
    examples: Vec<CandidateExample>,
}

/// Найти кандидатов в термины глоссария.
pub fn mine_candidates(input: MineInput<'_>) -> Vec<TermCandidate> {
    let excluded = excluded_keys(input.known, input.dismissed);
    let mut seen: HashMap<String, Occurrence> = HashMap::new();

    for (start_ms, text) in input.replicas {
        for token in tokenize(text) {
            if token.chars().count() < MIN_TOKEN_CHARS {
                continue;
            }
            let key = token.to_lowercase();
            let entry = seen.entry(key).or_default();
            entry.total += 1;
            *entry.spellings.entry(token.to_string()).or_default() += 1;
            if entry.examples.len() < MAX_EXAMPLES
                && !entry.examples.iter().any(|e| e.start_ms == *start_ms)
            {
                entry.examples.push(CandidateExample {
                    start_ms: *start_ms,
                    text: (*text).to_string(),
                });
            }
        }
    }

    // Частотная верхушка считается по всем токенам, включая исключённые:
    // это свойство текста, а не результата отбора.
    let head = frequent_head_keys(&seen, input.frequent_head);

    let mut candidates: Vec<TermCandidate> = seen
        .into_iter()
        .filter(|(key, _)| !excluded.contains(key))
        .filter_map(|(key, occurrence)| {
            let surface = commonest_spelling(&occurrence)?;
            let rule = classify(
                &surface,
                &key,
                occurrence.total,
                head.contains(&key),
                input.min_occurrences,
            )?;
            Some(TermCandidate {
                surface,
                rule,
                occurrences: occurrence.total,
                examples: occurrence.examples,
            })
        })
        .collect();

    // Порядок устойчивый: по убыванию частоты, затем по алфавиту. Без
    // второго ключа `HashMap` давал бы разный порядок от запуска к
    // запуску, и прибор печатал бы каждый раз своё.
    candidates.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then_with(|| a.surface.cmp(&b.surface))
    });
    candidates
}

/// Одно правило на кандидата, в порядке убывания надёжности.
///
/// Порядок важен: `API` подходит и под `Acronym`, и — будь он длиннее —
/// под `Repeated`. Кандидат, посчитанный дважды, сложил бы числа по
/// правилам в бессмыслицу.
fn classify(
    surface: &str,
    key: &str,
    occurrences: u32,
    in_frequent_head: bool,
    min_occurrences: u32,
) -> Option<CandidateRule> {
    if is_latin(surface) {
        return Some(CandidateRule::Latin);
    }
    if is_acronym(surface) {
        return Some(CandidateRule::Acronym);
    }
    if occurrences >= min_occurrences
        && !in_frequent_head
        && key.chars().count() >= MIN_REPEATED_CHARS
    {
        return Some(CandidateRule::Repeated);
    }
    None
}

/// Токен, все буквы которого латинские.
///
/// Чисто числовой токен латинским не считается: `2024` термином не
/// бывает, а под правило подошёл бы — букв, нарушающих условие, в нём
/// нет вовсе.
fn is_latin(token: &str) -> bool {
    let mut has_letter = false;
    for ch in token.chars() {
        if ch.is_alphabetic() {
            if !ch.is_ascii_alphabetic() {
                return false;
            }
            has_letter = true;
        }
    }
    has_letter
}

fn is_acronym(token: &str) -> bool {
    let letters: Vec<char> = token.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// Слова, разделённые всем, что не буква и не цифра.
///
/// Дефис и точка внутри слова не сохраняются намеренно: `intra.ru`
/// распадётся на два токена, зато `конце.Дальше` не слипнется в один.
/// Составные термины — работа для другого правила, и его здесь нет.
fn tokenize(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
}

fn commonest_spelling(occurrence: &Occurrence) -> Option<String> {
    occurrence
        .spellings
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(spelling, _)| spelling.clone())
}

fn excluded_keys(known: &[GlossaryTerm], dismissed: &[String]) -> HashSet<String> {
    let mut excluded = HashSet::new();
    for term in known {
        excluded.insert(term.surface.to_lowercase());
        excluded.insert(term.canonical.to_lowercase());
    }
    for entry in dismissed {
        excluded.insert(entry.to_lowercase());
    }
    excluded
}

fn frequent_head_keys(seen: &HashMap<String, Occurrence>, size: usize) -> HashSet<String> {
    let mut ranked: Vec<(&String, u32)> = seen.iter().map(|(k, v)| (k, v.total)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .into_iter()
        .take(size)
        .map(|(key, _)| key.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage, TermCandidate};

    use super::*;

    fn input<'a>(replicas: &'a [(u64, &'a str)]) -> MineInput<'a> {
        MineInput {
            replicas,
            known: &[],
            dismissed: &[],
            min_occurrences: 3,
            frequent_head: 20,
        }
    }

    fn surfaces(candidates: &[TermCandidate]) -> Vec<&str> {
        candidates.iter().map(|c| c.surface.as_str()).collect()
    }

    #[test]
    fn latin_inside_russian_speech_is_a_candidate() {
        let replicas = [(0, "давай посмотрим UniFFI на этой неделе")];

        let found = mine_candidates(input(&replicas));

        assert_eq!(surfaces(&found), vec!["UniFFI"]);
        assert_eq!(found[0].rule, CandidateRule::Latin);
    }

    #[test]
    fn an_acronym_is_a_candidate_in_either_script() {
        let replicas = [(0, "оплата через СБП и дальше по API")];

        let found = mine_candidates(input(&replicas));

        let mut got = surfaces(&found);
        got.sort_unstable();
        assert_eq!(got, vec!["API", "СБП"]);
        assert!(found.iter().all(|c| c.rule != CandidateRule::Repeated));
    }

    /// Наполнитель: длинные служебные слова, звучащие десятки раз.
    ///
    /// Длиннее четырёх букв — то есть порог длины они проходят, — и
    /// повторены двадцать раз, то есть проходят и порог повторов.
    /// Отсечь их может только частотная верхушка, и в этом весь смысл
    /// наполнителя.
    const FILLER: &str = "чтобы этого было тогда потому что этого хотелось";
    const FILLER_REPLICAS: u64 = 20;

    /// Текст с частотной структурой настоящей расшифровки: служебные
    /// слова звучат десятки раз, термин — единицы.
    ///
    /// Первая версия этих тестов брала три реплики, и в них термин сам
    /// оказывался самым частым словом текста, то есть попадал в
    /// частотную верхушку. Тест на порог повторов при этом проходил —
    /// но по неверной причине: слово отсеивалось верхушкой, а не
    /// порогом. Синтетика обязана иметь ту же структуру, что материал,
    /// иначе она проверяет не то.
    fn with_filler(term_replicas: &[(u64, &'static str)]) -> Vec<(u64, &'static str)> {
        let mut replicas: Vec<(u64, &'static str)> =
            (0..FILLER_REPLICAS).map(|i| (i * 1_000, FILLER)).collect();
        replicas.extend_from_slice(term_replicas);
        replicas
    }

    /// Размер верхушки, ровно накрывающей наполнитель.
    ///
    /// Считается из самого наполнителя, а не вписывается числом. Число
    /// пришлось бы подбирать под ответ — тот самый способ получить
    /// зелёный тест, ничего не проверяющий, — и молча ломалось бы от
    /// правки строки выше.
    fn filler_vocabulary() -> usize {
        FILLER
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<HashSet<_>>()
            .len()
    }

    /// Повтор считается по всей встрече, а не внутри одной реплики.
    #[test]
    fn a_repeated_rare_word_is_a_candidate() {
        let replicas = with_filler(&[
            (100_000, "нужен прескоринг для заявки"),
            (101_000, "прескоринг не проходит"),
            (102_000, "прескоринг снова"),
        ]);

        let found = mine_candidates(MineInput {
            frequent_head: filler_vocabulary(),
            ..input(&replicas)
        });

        assert_eq!(surfaces(&found), vec!["прескоринг"]);
        assert_eq!(found[0].rule, CandidateRule::Repeated);
        assert_eq!(found[0].occurrences, 3);
    }

    #[test]
    fn a_word_below_the_repeat_floor_is_not_a_candidate() {
        let replicas = with_filler(&[
            (100_000, "нужен прескоринг для заявки"),
            (101_000, "прескоринг не проходит"),
        ]);

        let found = mine_candidates(MineInput {
            frequent_head: filler_vocabulary(),
            ..input(&replicas)
        });

        assert!(
            found.is_empty(),
            "слово с двумя повторами прошло порог: {:?}",
            surfaces(&found)
        );
    }

    /// Заведомо отрицательный случай номер один: служебные слова.
    ///
    /// Опора — частотная верхушка самой встречи, а не список из головы.
    /// Слова наполнителя длиннее четырёх букв и повторены двадцать раз,
    /// то есть проходят и порог длины, и порог повторов: отсечь их может
    /// только верхушка.
    #[test]
    fn the_frequent_head_of_the_meeting_is_never_a_candidate() {
        let replicas = with_filler(&[]);

        let found = mine_candidates(MineInput {
            frequent_head: filler_vocabulary(),
            ..input(&replicas)
        });

        assert!(
            found.is_empty(),
            "частотная верхушка встречи попала в кандидаты: {:?}",
            surfaces(&found)
        );
    }

    /// Заведомо отрицательный случай номер два, и он важнее первого:
    /// очередь, предлагающая уже одобренное, читается как поломка.
    #[test]
    fn a_term_already_in_the_glossary_is_not_offered_again() {
        let replicas = [(0, "снова этот UniFFI")];
        let known = [GlossaryTerm {
            id: "1".into(),
            surface: "униффи".into(),
            canonical: "UniFFI".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Global,
            kind: GlossaryKind::Hint,
        }];

        let found = mine_candidates(MineInput {
            known: &known,
            ..input(&replicas)
        });

        assert!(
            found.is_empty(),
            "предложен одобренный термин: {:?}",
            surfaces(&found)
        );
    }

    #[test]
    fn a_dismissed_candidate_does_not_come_back() {
        let replicas = [(0, "опять этот Jira тикет")];
        let dismissed = ["jira".to_string()];

        let found = mine_candidates(MineInput {
            dismissed: &dismissed,
            ..input(&replicas)
        });

        assert!(
            found.is_empty(),
            "вернулся отклонённый: {:?}",
            surfaces(&found)
        );
    }

    #[test]
    fn a_candidate_carries_up_to_two_examples_with_timecodes() {
        let replicas = [
            (0, "первый UniFFI"),
            (5_000, "второй UniFFI"),
            (9_000, "третий UniFFI"),
        ];

        let found = mine_candidates(input(&replicas));

        assert_eq!(found[0].occurrences, 3);
        assert_eq!(found[0].examples.len(), 2);
        assert_eq!(found[0].examples[0].start_ms, 0);
        assert_eq!(found[0].examples[1].start_ms, 5_000);
    }

    /// Один кандидат — одно правило. Иначе `API` пришёл бы дважды и
    /// числа по правилам сложились бы в бессмыслицу.
    #[test]
    fn each_candidate_gets_exactly_one_rule() {
        let replicas = [(0, "СБП и СБП"), (1_000, "снова СБП")];

        let found = mine_candidates(input(&replicas));

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, CandidateRule::Acronym);
    }

    /// Регистр объединяется, а показывается самая частая форма: человеку
    /// незачем видеть `uniffi` и `UniFFI` двумя строками.
    #[test]
    fn case_variants_collapse_into_the_commonest_spelling() {
        let replicas = [
            (0, "UniFFI здесь"),
            (1_000, "UniFFI тут"),
            (2_000, "uniffi там"),
        ];

        let found = mine_candidates(input(&replicas));

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].surface, "UniFFI");
        assert_eq!(found[0].occurrences, 3);
    }

    /// Год термином не бывает, а под правило латиницы подошёл бы: букв,
    /// нарушающих условие, в нём нет вовсе.
    #[test]
    fn a_bare_number_is_not_a_latin_candidate() {
        let replicas = [(0, "перенесли на 2026 год"), (1_000, "именно 2026")];

        let found = mine_candidates(input(&replicas));

        assert!(
            !surfaces(&found).contains(&"2026"),
            "число попало в кандидаты: {:?}",
            surfaces(&found)
        );
    }

    /// Утверждение о свойстве, а не о значении: правильный размер
    /// верхушки не знает никто, его меряет прибор. Но каким бы он ни
    /// был, расширение верхушки обязано только убирать кандидатов.
    #[test]
    fn widening_the_frequent_head_never_adds_candidates() {
        let replicas = with_filler(&[
            (100_000, "нужен прескоринг для заявки"),
            (101_000, "прескоринг не проходит"),
            (102_000, "прескоринг снова"),
        ]);

        let mut previous = usize::MAX;
        for head in 0..=filler_vocabulary() + 2 {
            let count = mine_candidates(MineInput {
                frequent_head: head,
                ..input(&replicas)
            })
            .iter()
            .filter(|c| c.rule == CandidateRule::Repeated)
            .count();

            assert!(
                count <= previous,
                "верхушка {head} дала больше кандидатов, чем предыдущая: {count} против {previous}"
            );
            previous = count;
        }

        // Заведомо положительный случай для самого свойства: при нулевой
        // верхушке кандидаты по этому правилу вообще есть. Без него
        // монотонность выполнялась бы на сплошных нулях.
        let at_zero = mine_candidates(MineInput {
            frequent_head: 0,
            ..input(&replicas)
        })
        .iter()
        .filter(|c| c.rule == CandidateRule::Repeated)
        .count();
        assert!(
            at_zero > 0,
            "при нулевой верхушке не нашлось ни одного кандидата"
        );
    }

    #[test]
    fn nothing_to_mine_gives_nothing_and_does_not_panic() {
        assert!(mine_candidates(input(&[])).is_empty());
    }
}
