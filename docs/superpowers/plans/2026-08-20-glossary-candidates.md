# Кандидаты в глоссарий — план работ

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Добывать из Final термины, которых в глоссарии ещё нет, и мерить,
годятся ли они, — до того, как строить очередь одобрения.

**Architecture:** Добыча — чистая функция в `glossary::mine`: на входе
реплики, известные термины и отклонённые, на выходе кандидаты с
доказательством. Кандидат рождается только подсказкой. Прибор
`term-probe` считает числа на настоящей базе; интерфейс не заводится,
пока чисел нет.

**Tech Stack:** Rust (крейты `domain`, `glossary`, `storage`), SQLite.
Всё проверяется на этой машине — Swift здесь не участвует.

Спека: `docs/superpowers/specs/2026-08-20-glossary-candidates-design.md`.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; сообщения коммитов и
  тела PR — **по-английски** (`CLAUDE.md`).
- **Полный `cargo test` по workspace не влезает в память VPS.** Гонять по
  крейтам: `cd rust && cargo test -p meetingraft-glossary -p meetingraft-domain -p meetingraft-storage`.
- Имена пакетов с префиксом: крейт `glossary` — пакет
  `meetingraft-glossary`. `-p glossary` молча не найдёт ничего.
- Перед коммитом: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- **`glossary` зависит только от `domain`.** Реплики приходят слайсом
  `(start_ms, &str)`, а не типом из `postcall`: обратная зависимость
  сломала бы граф крейтов (`postcall` → `glossary`, не наоборот).
- **Кандидат — всегда `GlossaryKind::Hint`.** Ни одного пути, где добыча
  рождает замену.
- **Интерфейс в этот план не входит.** Он заводится после чисел прибора.
- Ветка: `feat/glossary-candidates`.

---

### Task 1: Кандидат как тип

**Files:**
- Modify: `rust/crates/domain/src/glossary.rs`
- Modify: `rust/crates/domain/src/lib.rs` (реэкспорт)

**Interfaces:**
- Consumes: `SpeechLanguage`, `GlossaryTerm` — уже есть.
- Produces: `CandidateRule`, `CandidateExample`, `TermCandidate`. Task 2
  строит их, Task 4 печатает.

- [ ] **Step 1: Написать падающий тест**

В `rust/crates/domain/src/glossary.rs`, в существующий `mod tests`:

```rust
    #[test]
    fn a_candidate_carries_its_evidence() {
        let candidate = TermCandidate {
            surface: "UniFFI".into(),
            rule: CandidateRule::Latin,
            occurrences: 4,
            examples: vec![CandidateExample {
                start_ms: 12_000,
                text: "посмотри UniFFI завтра".into(),
            }],
        };

        assert_eq!(candidate.rule, CandidateRule::Latin);
        assert_eq!(candidate.examples[0].start_ms, 12_000);
    }

    /// Правило `Repeated` держится на частоте, а не на форме, и потому
    /// названо сомнительным в спеке. Тест закрепляет только то, что
    /// правила различимы: по ним прибор разносит числа.
    #[test]
    fn rules_are_distinct() {
        assert_ne!(CandidateRule::Latin, CandidateRule::Acronym);
        assert_ne!(CandidateRule::Acronym, CandidateRule::Repeated);
    }
```

- [ ] **Step 2: Убедиться, что тест падает**

```
cd rust && cargo test -p meetingraft-domain glossary::
```

Ожидается ошибка сборки: `cannot find type TermCandidate in this scope`.

- [ ] **Step 3: Завести типы**

В `rust/crates/domain/src/glossary.rs` после `GlossaryTerm`:

```rust
/// Чем кандидат обратил на себя внимание.
///
/// Правило хранится вместе с кандидатом, потому что прибор считает
/// числа по каждому отдельно: то, которое не окупается, выбрасывается
/// по числу, а не по ощущению.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRule {
    /// Латиница внутри русской реплики: `UniFFI`, `backend`.
    Latin,
    /// Две и более заглавных подряд: `API`, `СБП`.
    Acronym,
    /// Повторён достаточно раз и не входит в частотную верхушку самой
    /// встречи. Самое слабое из трёх: словаря, с которым сверяться, нет.
    Repeated,
}

/// Реплика, в которой кандидат встретился.
///
/// Без неё кандидат неодобряем: одна строка вне контекста человеку
/// ничего не говорит через неделю после встречи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateExample {
    pub start_ms: u64,
    pub text: String,
}

/// Кандидат в термины глоссария.
///
/// `surface` — форма **как распознано**. Верной формы не знает никто, в
/// том числе добыча, поэтому одобренный кандидат становится подсказкой
/// (`GlossaryKind::Hint`) и готовый текст не переписывает.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermCandidate {
    pub surface: String,
    pub rule: CandidateRule,
    pub occurrences: u32,
    /// Не более двух: больше человек не читает, а меньше не убеждает.
    pub examples: Vec<CandidateExample>,
}
```

В `rust/crates/domain/src/lib.rs` дописать к существующему реэкспорту
глоссария `CandidateExample, CandidateRule, TermCandidate`.

- [ ] **Step 4: Убедиться, что тест зелёный**

```
cd rust && cargo test -p meetingraft-domain glossary::
```

- [ ] **Step 5: Коммит**

```bash
git add rust/crates/domain/src/glossary.rs rust/crates/domain/src/lib.rs
git commit -m "feat: a candidate carries the rule that found it"
```

---

### Task 2: Добыча как чистая функция

Ядро работы. Пишется тестами вперёд, каждый тест обязан краснеть без
своей ветки — это проверяется снятием веток в шаге 6.

**Files:**
- Create: `rust/crates/glossary/src/mine.rs`
- Modify: `rust/crates/glossary/src/lib.rs` (`mod mine; pub use mine::...`)
- Test: тесты внутри `mine.rs` (в этом крейте они лежат и в `lib.rs`, и
  рядом с кодом; для нового модуля — рядом)

**Interfaces:**
- Consumes: `TermCandidate`, `CandidateRule`, `CandidateExample`,
  `GlossaryTerm` из Task 1.
- Produces:
  ```rust
  pub struct MineInput<'a> {
      pub replicas: &'a [(u64, &'a str)],
      pub known: &'a [GlossaryTerm],
      pub dismissed: &'a [String],
      pub min_occurrences: u32,
      pub frequent_head: usize,
  }
  pub fn mine_candidates(input: MineInput<'_>) -> Vec<TermCandidate>;
  ```
  Task 4 зовёт `mine_candidates`.

- [ ] **Step 1: Написать падающие тесты**

Создать `rust/crates/glossary/src/mine.rs` с одним только блоком тестов
(код появится шагом 3):

```rust
#[cfg(test)]
mod tests {
    use domain::{CandidateRule, GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};

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

    fn surfaces(candidates: &[domain::TermCandidate]) -> Vec<&str> {
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
        assert!(found.iter().all(|c| c.rule == CandidateRule::Acronym));
    }

    /// Повтор считается по всей встрече, а не внутри одной реплики.
    #[test]
    fn a_repeated_rare_word_is_a_candidate() {
        let replicas = [
            (0, "нужен прескоринг для заявки"),
            (1_000, "прескоринг не проходит"),
            (2_000, "прескоринг снова"),
        ];

        let found = mine_candidates(input(&replicas));

        assert_eq!(surfaces(&found), vec!["прескоринг"]);
        assert_eq!(found[0].rule, CandidateRule::Repeated);
        assert_eq!(found[0].occurrences, 3);
    }

    #[test]
    fn a_word_below_the_repeat_floor_is_not_a_candidate() {
        let replicas = [
            (0, "нужен прескоринг для заявки"),
            (1_000, "прескоринг не проходит"),
        ];

        assert!(mine_candidates(input(&replicas)).is_empty());
    }

    /// Заведомо отрицательный случай номер один: служебные слова.
    ///
    /// Опора — частотная верхушка самой встречи, а не список из головы.
    #[test]
    fn the_frequent_head_of_the_meeting_is_never_a_candidate() {
        let replicas = [
            (0, "что это и как это"),
            (1_000, "это и то что это"),
            (2_000, "и это тоже что это"),
        ];

        let found = mine_candidates(MineInput {
            frequent_head: 2,
            ..input(&replicas)
        });

        assert!(
            !surfaces(&found).contains(&"это"),
            "самый частый токен встречи попал в кандидаты: {:?}",
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

        assert!(found.is_empty(), "предложен одобренный термин: {:?}", surfaces(&found));
    }

    #[test]
    fn a_dismissed_candidate_does_not_come_back() {
        let replicas = [(0, "опять этот Jira тикет")];
        let dismissed = ["jira".to_string()];

        let found = mine_candidates(MineInput {
            dismissed: &dismissed,
            ..input(&replicas)
        });

        assert!(found.is_empty(), "вернулся отклонённый: {:?}", surfaces(&found));
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

    /// Один кандидат — одно правило. Иначе `UniFFI` пришёл бы трижды и
    /// числа по правилам сложились бы в бессмыслицу.
    #[test]
    fn each_candidate_gets_exactly_one_rule() {
        let replicas = [
            (0, "API и API"),
            (1_000, "снова API"),
        ];

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

    #[test]
    fn nothing_to_mine_gives_nothing_and_does_not_panic() {
        assert!(mine_candidates(input(&[])).is_empty());
    }
}
```

- [ ] **Step 2: Убедиться, что тесты падают**

```
cd rust && cargo test -p meetingraft-glossary mine::
```

Ожидается ошибка сборки: `cannot find function mine_candidates`.

- [ ] **Step 3: Написать добычу**

В начало `rust/crates/glossary/src/mine.rs`, перед блоком тестов:

```rust
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

use std::collections::HashMap;

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
    /// Отклонённые человеком, в нижнем регистре.
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

/// Найти кандидатов в терминах глоссария.
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

    // Порядок устойчивый: по убыванию частоты, затем по алфавиту.
    // Без второго ключа `HashMap` давал бы разный порядок от запуска к
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

fn excluded_keys(known: &[GlossaryTerm], dismissed: &[String]) -> std::collections::HashSet<String> {
    let mut excluded = std::collections::HashSet::new();
    for term in known {
        excluded.insert(term.surface.to_lowercase());
        excluded.insert(term.canonical.to_lowercase());
    }
    for entry in dismissed {
        excluded.insert(entry.to_lowercase());
    }
    excluded
}

fn frequent_head_keys(
    seen: &HashMap<String, Occurrence>,
    size: usize,
) -> std::collections::HashSet<String> {
    let mut ranked: Vec<(&String, u32)> = seen.iter().map(|(k, v)| (k, v.total)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .into_iter()
        .take(size)
        .map(|(key, _)| key.clone())
        .collect()
}
```

В `rust/crates/glossary/src/lib.rs` дописать к существующим объявлениям:

```rust
mod mine;

pub use mine::{MineInput, mine_candidates};
```

- [ ] **Step 4: Убедиться, что тесты зелёные**

```
cd rust && cargo test -p meetingraft-glossary mine::
```

- [ ] **Step 5: Линт**

```
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 6: Проверить каждый тест снятием его ветки**

Зелёный тест ничего не значит, пока не показано, что он краснеет. Снять
по одной ветке, убедиться, что краснеет **свой** тест, вернуть:

| Снять | Обязан покраснеть |
|---|---|
| `if is_latin(surface) { … }` | `latin_inside_russian_speech_is_a_candidate` |
| `if is_acronym(surface) { … }` | `an_acronym_is_a_candidate_in_either_script` |
| `occurrences >= min_occurrences` | `a_word_below_the_repeat_floor_is_not_a_candidate` |
| `!in_frequent_head` | `the_frequent_head_of_the_meeting_is_never_a_candidate` |
| фильтр `!excluded.contains(key)` | `a_term_already_in_the_glossary_is_not_offered_again` и `a_dismissed_candidate_does_not_come_back` |
| `entry.examples.len() < MAX_EXAMPLES` | `a_candidate_carries_up_to_two_examples_with_timecodes` |

Покраснел не тот тест — значит тест проверяет не то, что написано в его
имени, и правится тест, а не код.

- [ ] **Step 7: Коммит**

```bash
git add rust/crates/glossary/src/mine.rs rust/crates/glossary/src/lib.rs
git commit -m "feat: mine term candidates from a finished transcript"
```

---

### Task 3: Отклонённое не возвращается

**Files:**
- Modify: `rust/crates/storage/src/migrations.rs` (шаг 31)
- Modify: `rust/crates/storage/src/audio_manifest.rs` (три метода)

**Interfaces:**
- Consumes: ничего.
- Produces: `AudioManifestStore::dismiss_candidate(&mut self, surface: &str) -> Result<(), AudioManifestError>`,
  `list_dismissed_candidates(&self) -> Result<Vec<String>, AudioManifestError>`,
  `restore_candidate(&mut self, surface: &str) -> Result<(), AudioManifestError>`.
  Task 4 зовёт `list_dismissed_candidates`.

- [ ] **Step 1: Написать падающий тест**

В `rust/crates/storage/src/audio_manifest.rs`, в блок тестов:

```rust
    #[test]
    fn a_dismissed_candidate_is_remembered_in_lower_case() {
        let dir = tempdir().expect("временный каталог");
        let mut store = AudioManifestStore::open(dir.path()).expect("база");

        store.dismiss_candidate("Jira").expect("отклонение");

        assert_eq!(
            store.list_dismissed_candidates().expect("список"),
            vec!["jira".to_string()]
        );
    }

    /// Отклонение дважды — не ошибка: человек мог нажать повторно на
    /// кандидате, пришедшем с другой встречи.
    #[test]
    fn dismissing_twice_is_not_an_error() {
        let dir = tempdir().expect("временный каталог");
        let mut store = AudioManifestStore::open(dir.path()).expect("база");

        store.dismiss_candidate("jira").expect("первое отклонение");
        store.dismiss_candidate("JIRA").expect("второе отклонение");

        assert_eq!(store.list_dismissed_candidates().expect("список").len(), 1);
    }

    /// Снятие отклонения существует потому, что автоудаления нет:
    /// молча терять решения человека нельзя, значит нужен способ их
    /// пересмотреть.
    #[test]
    fn a_restored_candidate_can_be_offered_again() {
        let dir = tempdir().expect("временный каталог");
        let mut store = AudioManifestStore::open(dir.path()).expect("база");
        store.dismiss_candidate("jira").expect("отклонение");

        store.restore_candidate("Jira").expect("снятие");

        assert!(store.list_dismissed_candidates().expect("список").is_empty());
    }
```

- [ ] **Step 2: Убедиться, что тест падает**

```
cd rust && cargo test -p meetingraft-storage dismissed
```

Ожидается ошибка сборки: `no method named dismiss_candidate`.

- [ ] **Step 3: Добавить шаг миграции**

В `rust/crates/storage/src/migrations.rs` в конец массива `STEPS`
добавить 31-й элемент:

```rust
    // Шаг 31: отклонённые кандидаты в термины.
    //
    // Без этой таблицы очередь предлагала бы один и тот же мусор после
    // каждой встречи и становилась бы непригодной за два захода.
    // Ключ в нижнем регистре: `Jira` и `jira` — одно решение человека.
    "
    CREATE TABLE IF NOT EXISTS dismissed_candidates (
        surface TEXT PRIMARY KEY NOT NULL,
        dismissed_at_ms INTEGER NOT NULL
    );
    ",
```

- [ ] **Step 4: Добавить три метода**

В `rust/crates/storage/src/audio_manifest.rs` рядом с методами глоссария:

```rust
    /// Больше этого кандидата не предлагать.
    ///
    /// Ключ приводится к нижнему регистру: решение человека принято про
    /// слово, а не про его написание в конкретной реплике.
    pub fn dismiss_candidate(&mut self, surface: &str) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "INSERT INTO dismissed_candidates (surface, dismissed_at_ms)
             VALUES (?1, ?2)
             ON CONFLICT(surface) DO UPDATE SET dismissed_at_ms = excluded.dismissed_at_ms",
            params![surface.to_lowercase(), now_ms()],
        )?;
        Ok(())
    }

    /// Снять отклонение: кандидат снова может быть предложен.
    pub fn restore_candidate(&mut self, surface: &str) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "DELETE FROM dismissed_candidates WHERE surface = ?1",
            params![surface.to_lowercase()],
        )?;
        Ok(())
    }

    pub fn list_dismissed_candidates(&self) -> Result<Vec<String>, AudioManifestError> {
        let mut stmt = self
            .conn
            .prepare("SELECT surface FROM dismissed_candidates ORDER BY surface")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
```

Если в файле нет свободной функции `now_ms()`, взять тот же способ
получения времени, что используют соседние методы (например
`upsert_glossary_term`), и не заводить второй.

- [ ] **Step 5: Убедиться, что тесты зелёные**

```
cd rust && cargo test -p meetingraft-storage dismissed
```

- [ ] **Step 6: Проверить, что миграция не ломает существующую базу**

```
cd rust && cargo test -p meetingraft-storage migrations::
```

Все 11 тестов миграций обязаны остаться зелёными: `migrate_is_idempotent`
и `migrate_legacy_database_keeps_rows` — главные.

- [ ] **Step 7: Коммит**

```bash
git add rust/crates/storage/src/migrations.rs rust/crates/storage/src/audio_manifest.rs
git commit -m "feat: a dismissed candidate stays dismissed"
```

---

### Task 4: Прибор, который решает судьбу очереди

**Files:**
- Create: `rust/crates/term-probe/Cargo.toml`
- Create: `rust/crates/term-probe/src/main.rs`
- Modify: `rust/Cargo.toml` (`members`)

**Interfaces:**
- Consumes: `mine_candidates`, `MineInput` (Task 2),
  `AudioManifestStore::list_dismissed_candidates`, `list_glossary_terms`,
  `list_meeting_summaries`, `list_final_transcripts`, `list_final_segments`.
- Produces: ничего для кода; производит числа.

- [ ] **Step 1: Завести крейт**

`rust/crates/term-probe/Cargo.toml`:

```toml
[package]
name = "meetingraft-term-probe"
version.workspace = true
edition.workspace = true

[[bin]]
name = "term-probe"
path = "src/main.rs"

[dependencies]
domain = { path = "../domain", package = "meetingraft-domain" }
glossary = { path = "../glossary", package = "meetingraft-glossary" }
storage = { path = "../storage", package = "meetingraft-storage" }
```

Сверить имена и способ объявления зависимостей с
`rust/crates/dup-probe/Cargo.toml` — там тот же набор, и расхождение в
формате поймает `cargo fmt`.

В `rust/Cargo.toml` дописать в `members`: `"crates/term-probe",`.

- [ ] **Step 2: Написать самопроверку и режимы**

`rust/crates/term-probe/src/main.rs`:

```rust
//! Прибор для кандидатов в глоссарий (Phase 13).
//!
//! Отвечает на вопрос, от которого зависит, строить ли очередь
//! одобрения вообще: **сколько кандидатов даёт каждое правило и сколько
//! из них мусор**. Если из полусотни сорок пять — шум, очередь станет
//! работой для человека вместо помощи, и делать её не надо.
//!
//! Каждый запуск начинается с заведомо положительного и заведомо
//! отрицательного случая. Ноль кандидатов от слепого прибора выглядит
//! ровно так же, как встреча без терминов.

use std::path::Path;
use std::process::ExitCode;

use domain::CandidateRule;
use glossary::{MineInput, mine_candidates};
use storage::AudioManifestStore;

const USAGE: &str = "\
Прибор для кандидатов в термины глоссария.

    term-probe <путь-к-данным> [id-встречи]

Без id считает по всем встречам, у которых есть Final.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Сперва прибор, потом данные.
    if !self_check() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            ExitCode::SUCCESS
        }
        [root] => run(Path::new(root), None),
        [root, meeting] => run(Path::new(root), Some(meeting.as_str())),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Проверка прибора на синтетике, до всякой базы.
fn self_check() -> bool {
    println!("=== Самопроверка ===");

    // Заведомо положительный: по одному подлогу на каждое правило.
    let replicas = [
        (0u64, "давай посмотрим UniFFI на неделе"),
        (1_000, "оплата пойдёт через СБП"),
        (2_000, "прескоринг заявки не проходит"),
        (3_000, "прескоринг снова упал"),
        (4_000, "и прескоринг опять"),
    ];
    let found = mine_candidates(MineInput {
        replicas: &replicas,
        known: &[],
        dismissed: &[],
        min_occurrences: 3,
        frequent_head: 5,
    });

    let has = |rule: CandidateRule| found.iter().any(|c| c.rule == rule);
    let latin = has(CandidateRule::Latin);
    let acronym = has(CandidateRule::Acronym);
    let repeated = has(CandidateRule::Repeated);
    println!("  положительный контроль: Latin {latin}, Acronym {acronym}, Repeated {repeated}");
    if !(latin && acronym && repeated) {
        println!("\n  Подложенный термин не найден. Числа ниже были бы числами прибора.");
        return false;
    }

    // Заведомо отрицательный: служебное слово из верхушки.
    let noise = [
        (0u64, "это и что это"),
        (1_000, "и это что это"),
        (2_000, "что это и это"),
    ];
    let junk = mine_candidates(MineInput {
        replicas: &noise,
        known: &[],
        dismissed: &[],
        min_occurrences: 2,
        frequent_head: 3,
    });
    println!("  отрицательный контроль: кандидатов из служебных слов {}", junk.len());
    if !junk.is_empty() {
        println!("\n  Отбор принимает служебные слова. Прибору верить нельзя.");
        return false;
    }

    println!("  прибор годен\n");
    true
}

fn run(root: &Path, meeting_filter: Option<&str>) -> ExitCode {
    let store = match AudioManifestStore::open(root) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("База не открылась: {error}");
            return ExitCode::FAILURE;
        }
    };

    let known = store.list_glossary_terms().unwrap_or_default();
    let dismissed = store.list_dismissed_candidates().unwrap_or_default();
    println!("Глоссарий: {} терминов, отклонено: {}", known.len(), dismissed.len());

    let meetings = store.list_meeting_summaries().unwrap_or_default();
    let mut judged = 0usize;
    let mut totals = [0usize; 3];

    for meeting in &meetings {
        if let Some(filter) = meeting_filter {
            if meeting.id != filter {
                continue;
            }
        }
        let segments = store.list_final_segments(&meeting.id, None).unwrap_or_default();
        if segments.is_empty() {
            continue;
        }
        judged += 1;

        let replicas: Vec<(u64, &str)> = segments
            .iter()
            .map(|segment| (segment.start_ms, segment.text.as_str()))
            .collect();
        let found = mine_candidates(MineInput {
            replicas: &replicas,
            known: &known,
            dismissed: &dismissed,
            min_occurrences: 3,
            frequent_head: 40,
        });

        println!("\n=== {} · реплик {} ===", meeting.id, segments.len());
        for rule in [CandidateRule::Latin, CandidateRule::Acronym, CandidateRule::Repeated] {
            let group: Vec<_> = found.iter().filter(|c| c.rule == rule).collect();
            totals[rule_index(rule)] += group.len();
            println!("  {rule:?}: {}", group.len());
            for candidate in group {
                let example = candidate
                    .examples
                    .first()
                    .map(|e| e.text.as_str())
                    .unwrap_or("—");
                println!(
                    "    {:>3}×  {:<24} «{}»",
                    candidate.occurrences,
                    candidate.surface,
                    shorten(example, 60)
                );
            }
        }
    }

    if judged == 0 {
        println!(
            "\nСравнивать нечего: встреч с Final не найдено{}.",
            meeting_filter.map(|f| format!(" (фильтр {f})")).unwrap_or_default()
        );
        println!("Это не «кандидатов нет» — это отсутствие материала.");
        return ExitCode::FAILURE;
    }

    println!("\n=== Итог по {judged} встречам ===");
    println!("  Latin {} · Acronym {} · Repeated {}", totals[0], totals[1], totals[2]);
    println!(
        "\nДальше глазами: пройти по списку и отметить, сколько строк —\n\
         настоящие термины. Правило, у которого доля мусора велика,\n\
         выбрасывается; очередь строится только если числа её оправдали."
    );
    ExitCode::SUCCESS
}

fn rule_index(rule: CandidateRule) -> usize {
    match rule {
        CandidateRule::Latin => 0,
        CandidateRule::Acronym => 1,
        CandidateRule::Repeated => 2,
    }
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}
```

Сигнатуры `list_final_segments`, `list_meeting_summaries` и поля
`FinalSegment` сверить с `rust/crates/storage/src/audio_manifest.rs`
перед сборкой: если у сегмента поле называется иначе, чем `start_ms` или
`text`, править вызов, а не структуру.

- [ ] **Step 3: Собрать и прогнать самопроверку**

```
cd rust && cargo run -p meetingraft-term-probe
```

Ожидается: три строки самопроверки, все контроли пройдены, затем usage.

- [ ] **Step 4: Убедиться, что самопроверка умеет краснеть**

Временно, **без коммита**, снять в `mine.rs` ветку `is_latin`. Прибор
обязан напечатать `Latin false` и выйти с ошибкой, не дойдя до базы.
Вернуть.

- [ ] **Step 5: Линт**

```
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 6: Коммит**

```bash
git add rust/crates/term-probe rust/Cargo.toml
git commit -m "feat: measure candidate quality before building a queue for it"
```

---

### Task 5: Документы

**Files:**
- Modify: `docs/backlog.md` (Epic 7)
- Modify: `docs/roadmap.md` (Phase 13)
- Modify: `docs/mac-verification.md` (новый раздел)

**Interfaces:**
- Consumes: результаты Task 1–4.
- Produces: ничего.

- [ ] **Step 1: Отметить пункт Epic 7**

Пункт «Post-call mining of candidates … with approval queue» разделить:
добыча сделана и меряется прибором, очередь ждёт чисел. Записать, что
добыча рождает только подсказки и почему.

- [ ] **Step 2: Обновить Phase 13**

Записать, что половина петли была замкнута до начала работы
(`refresh_glossary` → `build_whisper_prompt` → `initial_prompt`), поэтому
фаза свелась к добыче и очереди; статус — **добыча готова, очередь ждёт
чисел прибора**.

- [ ] **Step 3: Записать сценарий прогона**

В `docs/mac-verification.md` добавить раздел «Кандидаты в глоссарий»:
команда `cargo run -p meetingraft-term-probe -- ~/Library/Application\ Support/meetingraft`,
что печатается, и главное — **что от человека требуется отметить долю
мусора по каждому правилу**, потому что прибор её не знает.

- [ ] **Step 4: Коммит**

```bash
git add docs/backlog.md docs/roadmap.md docs/mac-verification.md
git commit -m "docs: mining is done, the queue waits for numbers"
```

---

## Что этот план сознательно не содержит

- **Очереди одобрения и любого интерфейса.** Строить экран под
  неизмеренное качество отбора — та же ошибка, что переключатель за
  неизмеренным порогом в Epic 8.
- **Границы UniFFI.** Она заводится вместе с очередью: `Ffi`-запись без
  экрана, который её читает, — мёртвый код на границе, ломающий каждый
  конструктор в тестах Swift.
- **Корпусной редкости.** Ложится поверх той же добычи, когда встреч
  наберётся.
- **Составных терминов.** `tokenize` рвёт `pull request` на два токена
  намеренно; фразовые кандидаты — отдельная работа с отдельной ценой.
