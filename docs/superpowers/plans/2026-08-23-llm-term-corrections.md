# Исправления от модели — план работ

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Получить от модели пары «как распознано → чем это, вероятно,
было» по всей расшифровке и **измерить долю порч** — предложений, где
исходное слово было верным, — до того, как строить очередь одобрения.

**Architecture:** Промпт, разбор и отсевы — чистые функции в
`postcall::corrections`: на входе сегменты Final и известные термины, на
выходе предложения с доказательством и отчёт об отброшенном. Ничего не
применяется и никуда не пишется. Прибор `fix-probe` ходит в живую модель
и печатает пары с контекстом; интерфейс и хранение не заводятся, пока нет
чисел.

**Tech Stack:** Rust (крейты `domain`, `postcall`), живая Ollama.
Swift здесь не участвует; числа приезжают с Мака.

Спека: `docs/superpowers/specs/2026-08-23-llm-term-corrections-design.md`.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; сообщения коммитов и
  тела PR — **по-английски** (`CLAUDE.md`).
- **Полный `cargo test` по workspace не влезает в память Linux-машины.**
  Гонять по крейтам: `cd rust && cargo test -p meetingraft-domain -p meetingraft-postcall`.
- Имена пакетов с префиксом: крейт `postcall` — пакет
  `meetingraft-postcall`. `-p postcall` молча не найдёт ничего.
- Перед коммитом: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- **Ни одного пути, где предложение применяется само.** Ни в пересборке
  Final, ни на входе артефакта, ни «только для уверенных».
- **Свои копии чужих шагов не пишутся.** Нумерация реплик берётся готовым
  `polish::format_batch`, и на этом же выходе строится разбор: своя копия
  мерила бы выдумку прибора, а не поведение продукта (урок `brief-probe`).
- **Хранения, границы UniFFI и интерфейса в этом плане нет.** Они едут
  вместе с очередью, после чисел.
- Ветка: `feat/llm-term-corrections`.

---

### Task 1: Предложение как тип

**Files:**
- Modify: `rust/crates/domain/src/glossary.rs`
- Modify: `rust/crates/domain/src/lib.rs` (реэкспорт)

**Interfaces:**
- Consumes: `AudioChannel` — уже есть.
- Produces: `TermFix`. Task 3 их строит, Task 4 печатает.

- [ ] **Step 1: Написать падающий тест**

В `rust/crates/domain/src/glossary.rs`, в существующий `mod tests`:

```rust
    /// Предложение неодобряемо без места и без реплики: одно ради ▶,
    /// другое ради глаз. Пара сама по себе — только догадка.
    #[test]
    fn a_fix_carries_the_place_you_can_listen_to() {
        let fix = TermFix {
            channel: AudioChannel::System,
            start_ms: 762_000,
            end_ms: 767_500,
            surface: "кобриаты".into(),
            canonical: "ковариаты".into(),
            reason: "рядом «регрессия» и «выборка»".into(),
            replica_text: "кобриаты в регрессии считаем по той же выборке".into(),
        };

        assert_eq!(fix.start_ms, 762_000);
        assert!(fix.replica_text.contains(&fix.surface));
    }
```

- [ ] **Step 2: Убедиться, что тест падает**

```
cd rust && cargo test -p meetingraft-domain glossary::
```

Ожидается ошибка сборки: `cannot find type TermFix in this scope`.

- [ ] **Step 3: Завести тип**

В `rust/crates/domain/src/glossary.rs` после `TermCandidate`:

```rust
/// Предложение модели: как слово стоит в расшифровке и чем оно, по её
/// мнению, было.
///
/// Отличие от [`TermCandidate`] — в одном поле и в одном праве. Добыча
/// верной формы не знает и потому её не называет; модель называет
/// (`canonical`), но **звука не слышала**: это априорная вероятность, а
/// не свидетельство. Свидетельством пара становится только после того,
/// как человек послушал фрагмент, — ради этого здесь лежит место
/// (`channel`, `start_ms`, `end_ms`), а не один текст.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermFix {
    pub channel: AudioChannel,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Форма **как в расшифровке**, а не как её написала модель.
    pub surface: String,
    /// Что предложила модель. Догадка.
    pub canonical: String,
    /// На что она опёрлась — соседние слова. Пустая опора сама по себе
    /// сигнал, и видит его человек, а не порог.
    pub reason: String,
    /// Реплика целиком: без неё пара неодобряема глазами.
    pub replica_text: String,
}
```

Реэкспорт в `rust/crates/domain/src/lib.rs` рядом с `TermCandidate`.

- [ ] **Step 4: Убедиться, что тест зелёный**

```
cd rust && cargo test -p meetingraft-domain glossary::
```

- [ ] **Step 5: Коммит**

```bash
git add rust/crates/domain
git commit -m "feat: a model's fix carries the place you can listen to"
```

---

### Task 2: Промпт, который просит термины, а не редактуру

**Files:**
- Create: `rust/crates/postcall/src/corrections.rs`
- Modify: `rust/crates/postcall/src/lib.rs` (модуль и реэкспорт)

**Interfaces:**
- Consumes: `polish::format_batch`, `domain::{FinalSegment, SpeechLanguage}`.
- Produces: `fix_prompts(segments, language) -> (String, String)`.

**Тесты здесь утверждают, что инструкция сказана, а не что она
выполнена.** Судить о поведении модели по тексту промпта нельзя — это
проверяется живым прогоном (Task 4).

- [ ] **Step 1: Написать падающие тесты**

В новом `rust/crates/postcall/src/corrections.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_asks_for_misheard_terms_and_forbids_editing() {
        let (system, _) = fix_prompts(&[segment(0, "текст")], SpeechLanguage::Ru);

        assert!(system.contains("misheard"), "{system}");
        // Полировка занимает соседнюю нишу, и залезать в неё нельзя:
        // «поправленная» грамматика — это порча по построению.
        assert!(system.contains("Do not fix grammar"), "{system}");
        assert!(system.contains("punctuation"), "{system}");
    }

    /// Смещение модели этим не убрать, но сказать это стоит ничего.
    #[test]
    fn the_prompt_warns_against_pulling_rare_words_to_common_ones() {
        let (system, _) = fix_prompts(&[segment(0, "текст")], SpeechLanguage::Ru);

        assert!(system.contains("rare"), "{system}");
        assert!(system.contains("do not replace"), "{system}");
    }

    /// Число уверенности — самоотчёт той же догадки в виде измерения.
    /// Спрашивается опора: на какие соседние слова модель оперлась.
    #[test]
    fn the_prompt_asks_for_evidence_not_for_a_confidence_number() {
        let (system, _) = fix_prompts(&[segment(0, "текст")], SpeechLanguage::Ru);

        assert!(!system.to_lowercase().contains("confidence"), "{system}");
        assert!(system.contains("which nearby words"), "{system}");
    }

    /// Молчание модели неотличимо от поломки разбора, если пустой ответ
    /// не сказан словом.
    #[test]
    fn an_empty_answer_has_to_be_spelled_out() {
        let (system, _) = fix_prompts(&[segment(0, "текст")], SpeechLanguage::Ru);

        assert!(system.contains(EMPTY_ANSWER), "{system}");
    }

    /// Тот же урок, что у брифа: инструкция, названная только до
    /// расшифровки, на длинном входе перестаёт действовать.
    #[test]
    fn the_task_is_restated_after_the_transcript() {
        let (_, user) = fix_prompts(&[segment(0, "длинная расшифровка")], SpeechLanguage::Ru);

        let closing = user
            .split("</transcript>")
            .nth(1)
            .expect("расшифровка обязана быть закрыта тегом");
        assert!(closing.contains("Now list"), "{user}");
        assert!(closing.trim().len() > 20, "{closing}");
    }

    /// Реплики нумеруются тем же кодом, что и в полировке: по номеру
    /// пара потом сверяется с текстом.
    #[test]
    fn replicas_are_numbered_from_one() {
        let (_, user) = fix_prompts(
            &[segment(0, "первая"), segment(1, "вторая")],
            SpeechLanguage::Ru,
        );

        assert!(user.contains("1. первая"), "{user}");
        assert!(user.contains("2. вторая"), "{user}");
    }
}
```

Плюс локальный хелпер `segment(index, text) -> FinalSegment` — взять
готовый из `mod tests` в `polish.rs`.

- [ ] **Step 2: Убедиться, что тесты падают**

```
cd rust && cargo test -p meetingraft-postcall corrections::
```

- [ ] **Step 3: Написать промпт**

В начало `corrections.rs` — заголовок модуля, объясняющий несимметрию
(она не выводится из кода и стоила бы новому читателю недели):

```rust
//! Исправления, предложенные моделью по всей расшифровке (Phase 13).
//!
//! Модель видит весь текст сразу и потому берёт то, чего не может взять
//! декодер с окном в тридцать секунд: соседние реплики, повторяющиеся
//! темы, другие термины рядом.
//!
//! **Звука она при этом не слышит.** Термин восстанавливается из
//! испорченной строки плюс контекста, то есть априорной вероятностью, а
//! не свидетельством. Обычно попадает — и ошибается там, где ставки выше
//! всего: на редком верном слове. Фамилию, название продукта, внутренний
//! жаргон модель тянет к частому соседу и «исправляет» правильное в
//! неправильное, а отличить «услышано неверно» от «услышано верно,
//! просто слово редкое» не может в принципе.
//!
//! Отсюда всё устройство модуля: пара **предлагается** и не применяется
//! ничем; вместе с ней едет место, которое можно послушать, и реплика,
//! которую можно прочесть. Свидетельством догадка становится у человека
//! с кнопкой ▶, и больше нигде.

use domain::{AudioChannel, FinalSegment, GlossaryTerm, SpeechLanguage, TermFix};

use crate::polish::format_batch;

/// Чем модель обязана ответить, когда предлагать нечего.
///
/// Без явного слова «пусто» молчание модели неотличимо от поломки
/// разбора, а «исправлений нет» — от «прибор ослеп».
pub const EMPTY_ANSWER: &str = "НЕТ";

/// Инструкции для поиска неверно распознанных терминов.
pub fn fix_prompts(segments: &[FinalSegment], language: SpeechLanguage) -> (String, String) {
    let system = format!(
        "You are given a transcript of a meeting in language `{}`, produced by automatic \
         speech recognition, so some words may be misheard. Find words that were misheard: \
         names, product names, jargon, acronyms, technical terms. \
         Use the rest of the transcript as context — neighbouring replies, recurring topics, \
         other terms nearby. \
         Do not fix grammar, wording, punctuation or style: another step does that, and a \
         `corrected` word that was already right is the worst outcome here. \
         A rare or unusual word is usually a real term: do not replace it with a common \
         neighbour just because it looks odd. \
         For each fix return one line, exactly: \
         `<reply number> | <text exactly as it appears in the reply> | <what it should be> | \
         <which nearby words made you think so>`. \
         Keep both sides at most three words. \
         If you find nothing, answer with the single word `{}` and nothing else. \
         Return only these lines — no preamble, no explanation, no Markdown.",
        language.code(),
        EMPTY_ANSWER
    );
    let user = format!(
        "Transcript replies, numbered:\n\n<transcript>\n{}\n</transcript>\n\n\
         Now list the misheard terms as described above, or `{}` if there are none.",
        format_batch(segments),
        EMPTY_ANSWER
    );

    (system, user)
}
```

В `lib.rs` — `mod corrections;` и реэкспорт `EMPTY_ANSWER`, `fix_prompts`.

- [ ] **Step 4: Убедиться, что тесты зелёные**

```
cd rust && cargo test -p meetingraft-postcall corrections::
```

- [ ] **Step 5: Коммит**

```bash
git add rust/crates/postcall
git commit -m "feat: ask the model for misheard terms, not for an edit pass"
```

---

### Task 3: Разбор и отсевы — непонятое отбрасывается, а не додумывается

**Files:**
- Modify: `rust/crates/postcall/src/corrections.rs`
- Modify: `rust/crates/postcall/src/lib.rs` (реэкспорт)

**Interfaces:**
- Consumes: ответ модели, сегменты Final, активные термины глоссария.
- Produces: `parse_fixes(response) -> Result<ParsedFixes, String>`,
  `resolve_fixes(parsed, segments, known) -> (Vec<TermFix>, RejectReport)`.

- [ ] **Step 1: Написать падающие тесты**

Главные три — пятый, шестой и седьмой: они про способы соврать.

```rust
    #[test]
    fn an_explicit_no_is_emptiness_not_an_error() {
        let parsed = parse_fixes("НЕТ").expect("явное «пусто» — законный ответ");

        assert!(parsed.fixes.is_empty());
        assert_eq!(parsed.skipped_lines, 0);
    }

    /// Ответ не по формату — это «прибор ослеп», а не «исправлений нет».
    /// Слияние этих двух исходов уже стоило переписывания
    /// `EchoReport::empty()` (Epic 16).
    #[test]
    fn an_answer_off_format_is_an_error_not_emptiness() {
        let result = parse_fixes("Конечно! Вот исправленная расшифровка:\n\nМы обсудили релиз.");

        assert!(result.is_err(), "{result:?}");
    }

    /// Пара, которой в названной реплике нет, — выдумка целиком.
    #[test]
    fn a_pair_missing_from_its_replica_is_dropped() {
        let segments = [segment(0, "обсудили релиз в среду")];
        let parsed = parse_fixes("1 | кобриаты | ковариаты | контекст").unwrap();

        let (fixes, report) = resolve_fixes(&parsed, &segments, &[]);

        assert!(fixes.is_empty());
        assert_eq!(report.not_in_replica, 1);
    }

    /// Форма берётся из текста, а не из ответа: иначе в глоссарий уедет
    /// написание, которого в расшифровке не было ни разу.
    #[test]
    fn the_surface_comes_from_the_transcript_not_from_the_answer() {
        let segments = [segment(0, "Кобриаты в регрессии считаем по выборке")];
        let parsed = parse_fixes("1 | кобриаты | ковариаты | рядом «регрессия»").unwrap();

        let (fixes, _) = resolve_fixes(&parsed, &segments, &[]);

        assert_eq!(fixes[0].surface, "Кобриаты");
        assert_eq!(fixes[0].canonical, "ковариаты");
        assert_eq!(fixes[0].replica_text, segments[0].text);
    }

    /// Та же граница, что у `term_from_edit`: длинная замена не
    /// словарная, а смысловая, и в глоссарии она стала бы мусором,
    /// который через `initial_prompt` портит распознавание.
    #[test]
    fn a_long_replacement_is_not_a_term() {
        let segments = [segment(0, "давайте перенесём релиз на среду")];
        let parsed =
            parse_fixes("1 | перенесём релиз на среду | сдвинем выпуск на четверг | контекст")
                .unwrap();

        let (fixes, report) = resolve_fixes(&parsed, &segments, &[]);

        assert!(fixes.is_empty());
        assert_eq!(report.too_long, 1);
    }

    #[test]
    fn a_pair_already_in_the_glossary_is_not_proposed() {
        let segments = [segment(0, "смотри униффи завтра")];
        let known = [term("униффи", "UniFFI")];
        let parsed = parse_fixes("1 | униффи | UniFFI | название библиотеки").unwrap();

        let (fixes, report) = resolve_fixes(&parsed, &segments, &known);

        assert!(fixes.is_empty());
        assert_eq!(report.already_known, 1);
    }

    #[test]
    fn a_reply_number_out_of_range_is_dropped_and_counted() {
        let segments = [segment(0, "одна реплика")];
        let parsed = parse_fixes("7 | одна | одну | контекст").unwrap();

        let (fixes, report) = resolve_fixes(&parsed, &segments, &[]);

        assert!(fixes.is_empty());
        assert_eq!(report.out_of_range, 1);
    }

    /// Строки не по формату считаются, а не теряются молча: их число —
    /// показание о модели, и прибор его печатает.
    #[test]
    fn unparsable_lines_are_counted_not_silently_lost() {
        let parsed = parse_fixes("1 | а | б | опора\nвот что я нашёл:\n2 | в | г | опора").unwrap();

        assert_eq!(parsed.fixes.len(), 2);
        assert_eq!(parsed.skipped_lines, 1);
    }
```

- [ ] **Step 2: Убедиться, что тесты падают**

```
cd rust && cargo test -p meetingraft-postcall corrections::
```

- [ ] **Step 3: Написать разбор**

```rust
/// Что разобралось из ответа, до сверки с текстом.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedFixes {
    pub fixes: Vec<RawFix>,
    /// Строки, которые разобрать не удалось. Не потеря, а показание:
    /// модель, отвечающая наполовину прозой, видна этим числом.
    pub skipped_lines: usize,
}

/// Строка ответа, ещё не сверенная с расшифровкой.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFix {
    pub reply: usize,
    pub surface: String,
    pub canonical: String,
    pub reason: String,
}

/// Почему предложения не дошли до человека.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RejectReport {
    pub out_of_range: usize,
    pub not_in_replica: usize,
    pub too_long: usize,
    pub already_known: usize,
    pub duplicates: usize,
}

/// Сколько слов с каждой стороны ещё считается термином
/// (та же величина, что в `term_from_edit`).
const MAX_WORDS: usize = 3;

/// Разобрать ответ модели.
///
/// `Err` означает «ответ не по формату», и это не то же самое, что
/// «исправлений нет»: пустота говорится словом [`EMPTY_ANSWER`].
pub fn parse_fixes(response: &str) -> Result<ParsedFixes, String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return Err("LLM вернул пустой ответ вместо слова «НЕТ»".into());
    }
    if trimmed.eq_ignore_ascii_case(EMPTY_ANSWER) {
        return Ok(ParsedFixes::default());
    }

    let mut parsed = ParsedFixes::default();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            Some(fix) => parsed.fixes.push(fix),
            None => parsed.skipped_lines += 1,
        }
    }

    if parsed.fixes.is_empty() {
        return Err(format!(
            "LLM ответил не по формату: ни одной разобранной строки из {}",
            parsed.skipped_lines
        ));
    }
    Ok(parsed)
}

fn parse_line(line: &str) -> Option<RawFix> {
    let mut parts = line.splitn(4, '|').map(str::trim);
    let reply = parts.next()?.parse::<usize>().ok()?;
    let surface = parts.next()?.to_string();
    let canonical = parts.next()?.to_string();
    // Опора может отсутствовать — это законно и само по себе сигнал.
    let reason = parts.next().unwrap_or_default().to_string();
    (reply > 0 && !surface.is_empty() && !canonical.is_empty()).then_some(RawFix {
        reply,
        surface,
        canonical,
        reason,
    })
}

/// Сверить предложения с расшифровкой и отсеять то, что термином быть не
/// может.
pub fn resolve_fixes(
    parsed: &ParsedFixes,
    segments: &[FinalSegment],
    known: &[GlossaryTerm],
) -> (Vec<TermFix>, RejectReport) {
    let mut report = RejectReport::default();
    let mut out: Vec<TermFix> = Vec::new();

    for raw in &parsed.fixes {
        let Some(segment) = segments.get(raw.reply - 1) else {
            report.out_of_range += 1;
            continue;
        };
        // Форма берётся из текста: модель пишет как придётся, а в
        // глоссарий должно уехать то, что действительно прозвучало.
        let Some(surface) = find_surface(&segment.text, &raw.surface) else {
            report.not_in_replica += 1;
            continue;
        };
        if surface == raw.canonical {
            report.not_in_replica += 1;
            continue;
        }
        if word_count(&surface) > MAX_WORDS || word_count(&raw.canonical) > MAX_WORDS {
            report.too_long += 1;
            continue;
        }
        if is_known(known, &surface, &raw.canonical) {
            report.already_known += 1;
            continue;
        }
        if out.iter().any(|fix| {
            fix.start_ms == segment.start_ms
                && fix.surface == surface
                && fix.canonical == raw.canonical
        }) {
            report.duplicates += 1;
            continue;
        }
        out.push(TermFix {
            channel: segment.channel,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            surface,
            canonical: raw.canonical.clone(),
            reason: raw.reason.clone(),
            replica_text: segment.text.clone(),
        });
    }

    (out, report)
}
```

`find_surface` ищет без учёта регистра и возвращает **подстроку из
текста**; `is_known` сравнивает пару с surface и canonical глоссария без
учёта регистра, как это делает `normalize`.

- [ ] **Step 4: Убедиться, что тесты зелёные**

```
cd rust && cargo test -p meetingraft-postcall corrections::
```

- [ ] **Step 5: Проверить каждый тест снятием его ветки**

Зелёный тест ничего не значит, пока не показано, что он умеет краснеть.
По очереди убрать проверку — `not_in_replica`, `too_long`,
`already_known`, ветку `Err` в `parse_fixes` — и убедиться, что падает
именно тот тест, который её сторожит. Ветки вернуть.

Особенно `an_answer_off_format_is_an_error_not_emptiness`: сними ветку
`Err`, и прозаический ответ станет «исправлений нет».

- [ ] **Step 6: Линт**

```
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 7: Коммит**

```bash
git add rust/crates/postcall
git commit -m "feat: a fix that is not in the transcript is an invention, not a fix"
```

---

### Task 4: Прибор, который решает судьбу очереди

**Files:**
- Create: `rust/crates/fix-probe/Cargo.toml`
- Create: `rust/crates/fix-probe/src/main.rs`
- Modify: `rust/Cargo.toml` (members)

**Interfaces:**
- Consumes: `postcall::{fix_prompts, parse_fixes, resolve_fixes, OllamaNativeClient}`,
  `storage::AudioManifestStore`.
- Produces: числа и пары с контекстом. Ничего не пишет.

Устройство — по образцу `brief-probe`: сперва прибор, потом данные.

- [ ] **Step 1: Завести крейт**

`meetingraft-fix-probe`, седьмой в ряду с `echo-probe`, `gate-probe`,
`dup-probe`, `term-probe`, `diarize-probe`, `brief-probe`. Зависимости:
`domain`, `postcall`, `storage`.

- [ ] **Step 2: Написать самопроверку и тесты к ней**

Три запроса, как у `brief-probe`, и каждый закрывает свой способ соврать:

1. подложенная порча редкого слова, чья верная форма однозначна из
   соседних реплик, — **прогон первый**;
2. тот же вход — **прогон второй**: два ответа обязаны совпасть дословно;
3. чистая расшифровка без единой порчи — обязан прийти `НЕТ`.

Третий здесь главный. Модель, предлагающая что-нибудь всегда, — генератор
порч, и её вывод на настоящих данных не значит ничего.

```rust
    /// Заведомо положительный и заведомо отрицательный случаи сошлись.
    #[test]
    fn a_model_that_finds_the_planted_fix_and_stays_quiet_on_clean_text_is_fit() {
        let client = scripted([PLANTED, PLANTED, "НЕТ"]);

        assert!(blind_reason(self_check(&client)).is_none());
    }

    /// То, на чём сорвался первый заход Epic 8.
    #[test]
    fn a_model_that_answers_differently_twice_is_blind() {
        let client = scripted([PLANTED, "1 | кобриаты | кубраты | иначе", "НЕТ"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("неповторяем"), "{reason}");
    }

    /// Главный отрицательный контроль: предложение там, где портить
    /// нечего. Без него «сорок исправлений» читались бы как успех.
    #[test]
    fn a_model_that_proposes_something_on_clean_text_is_blind() {
        let client = scripted([PLANTED, PLANTED, "1 | среду | среду вечером | контекст"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("на чистом тексте"), "{reason}");
    }

    #[test]
    fn a_model_that_misses_the_planted_fix_is_blind() {
        let client = scripted(["НЕТ", "НЕТ", "НЕТ"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("подложенн"), "{reason}");
    }

    #[test]
    fn an_unreachable_model_is_blind_not_silent() {
        let client = ScriptedClient::new(vec![Err(LlmError::Transport("refused".into()))]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("недоступна"), "{reason}");
    }
```

`ScriptedClient`, `scripted`, `blind_reason` — взять из `brief-probe`
(там они уже написаны и проверены).

- [ ] **Step 3: Написать режимы отказа и печать**

Три исхода различаются и не сливаются:

- **прибор слеп** — модель неповторяема, недоступна, не нашла
  подложенное или предлагает на чистом тексте;
- **сравнивать нечего** — нет Final, нет сегментов (версия собрана из
  live-субтитров), модель ответила `НЕТ` на настоящей встрече;
- **посмотрели и вот что нашли**.

Печатается на каждую пару: номер реплики, тайм-код `мм:сс` и канал
(**ради ▶** — послушать в приложении), `surface → canonical`, опора и
реплика целиком. Плюс шапка: длина входа в символах, время ответа, и
отчёт об отброшенном (`RejectReport` + `skipped_lines`).

Хвост — то, чего прибор сделать не может, и он обязан это сказать вслух:

```rust
    println!(
        "\nДальше глазами и ухом. Прибор не различает три кучи и \
         различить не может:\n  1. верное исправление — ради чего всё;\n  \
         2. промах — безобиден: не одобрил, и всё;\n  \
         3. ПОРЧА: исходное слово было верным. Опасна: одобренная, она \
         уедет в глоссарий\n     и начнёт переписывать будущие \
         расшифровки.\n\nСчитается третья. «Модель предложила сорок \
         исправлений» звучит прекрасно\nи может означать двадцать пять \
         порч."
    );
```

- [ ] **Step 4: Собрать и прогнать самопроверку**

```
cd rust && cargo test -p meetingraft-fix-probe
cd rust && cargo run -p meetingraft-fix-probe
```

Без аргументов — подсказка по применению, а не паника.

- [ ] **Step 5: Убедиться, что самопроверка умеет краснеть**

Временно заменить ожидание третьего запроса с «`НЕТ`» на «что угодно» —
тест `a_model_that_proposes_something_on_clean_text_is_blind` обязан
упасть. Вернуть.

- [ ] **Step 6: Линт**

```
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 7: Коммит**

```bash
git add rust/crates/fix-probe rust/Cargo.toml
git commit -m "feat: count the corruptions before trusting the corrections"
```

---

### Task 5: Документы

**Files:**
- Modify: `docs/backlog.md` (Epic 7 и отметка в Epic 8)
- Modify: `docs/roadmap.md` (Phase 13)
- Modify: `docs/mac-verification.md` (новый раздел)

**Interfaces:**
- Consumes: результаты Task 1–4.
- Produces: ничего.

- [ ] **Step 1: Записать источник в Epic 7**

Рядом с кандидатами формой и частотой — второй источник: модель по всей
расшифровке. Записать несимметрию (ошибается на редком верном слове),
правило «предлагает, но не правит» и то, что судьбу очереди решает доля
порч, а не доля верных.

- [ ] **Step 2: Отметить в Epic 8, что отклонение LLM снято**

Спека кандидатов отклонила LLM как ненастроенную. Записать, что довод
снят не наличием Ollama, а **воспроизводимостью на `temperature 0`**,
проверенной 2026-08-17: без неё любое измерение бессмысленно.

- [ ] **Step 3: Обновить Phase 13**

Статус: добыча формой и частотой готова, источник из модели готов, **обе
очереди ждут чисел**. Так видно, что незакрытая часть фазы у них общая —
интерфейс одобрения, а не два разных экрана.

- [ ] **Step 4: Записать сценарий прогона**

В `docs/mac-verification.md` — раздел «Исправления от модели (Phase 13)»:
поднять Ollama (ссылка на шаг 1 раздела Epic 8), команда прогона, что
печатается, и главное — **разметить три кучи по печати прибора, а
сомнительные послушать по тайм-коду**. Записать заранее сформулированное
решение по числам: порч примерно столько же, сколько верных, — очередь не
строится вовсе.

- [ ] **Step 5: Коммит**

```bash
git add docs/backlog.md docs/roadmap.md docs/mac-verification.md
git commit -m "docs: the second source of terms waits for the same numbers"
```

---

## Что этот план сознательно не содержит

- **Очереди одобрения, ▶ и любого интерфейса.** Заводятся после чисел,
  вместе с очередью кандидатов: экран у них общий.
- **Хранения предложений и памяти об отклонённых.** До очереди хранить
  нечего, а таблица без читателя — мёртвая схема.
- **Границы UniFFI.** `Ffi`-запись без экрана, который её читает, — код,
  ломающий каждый конструктор в тестах Swift, ради ничего.
- **Автоприменения в любом виде**, включая «применять только уверенные»:
  уверенность здесь — самоотчёт догадки.
- **Батчей и скользящего окна.** Расшифровка идёт целиком: батч отдал бы
  модели ровно тот узкий контекст, ради избавления от которого всё и
  затевается. Если длинная встреча не влезет, это будет видно числом из
  Task 4, и тогда у работы появится своя цена и свой план.
