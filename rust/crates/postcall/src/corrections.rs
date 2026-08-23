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

use domain::{FinalSegment, GlossaryTerm, SpeechLanguage, TermFix};

use crate::polish::format_batch;

/// Чем модель обязана ответить, когда предлагать нечего.
///
/// Без явного слова «пусто» молчание модели неотличимо от поломки
/// разбора, а «исправлений нет» — от «прибор ослеп».
pub const EMPTY_ANSWER: &str = "НЕТ";

/// Сколько слов с каждой стороны ещё считается термином.
///
/// Та же величина, что в [`crate::term_from_edit`], и по той же причине:
/// длинная замена — правка смысла, а не словарная, и в глоссарии она
/// стала бы мусором, который через `initial_prompt` портит распознавание.
/// Разбор ответа проверяет это сам (модель просьбу нарушает), но сказать
/// границу вслух дешевле, чем потом её сторожить.
const MAX_WORDS: usize = 3;

/// Инструкции для поиска неверно распознанных терминов.
///
/// Расшифровка идёт целиком, а не батчами: батч отдал бы модели ровно
/// тот узкий контекст, ради избавления от которого всё и затевается.
pub fn fix_prompts(segments: &[FinalSegment], language: SpeechLanguage) -> (String, String) {
    let system = format!(
        "You are given a transcript of a meeting in language `{}`, produced by automatic \
         speech recognition, so some words may be misheard. Find words that were misheard: \
         names, product names, in-house jargon, acronyms, technical terms. \
         Use the rest of the transcript as context — neighbouring replies, recurring topics, \
         other terms nearby. \
         Do not fix grammar, wording, punctuation or style: another step does that, and a \
         word you `correct` that was already right is the worst outcome here. \
         A rare or unusual word is usually a real term: do not replace it with a common \
         neighbour just because it looks odd. \
         For each fix return one line, exactly: \
         `<reply number> | <text exactly as it appears in that reply> | <what it should be> | \
         <which nearby words made you think so>`. \
         Keep each side to at most {} words. \
         If you find nothing, answer with the single word `{}` and nothing else. \
         Return only those lines — no preamble, no explanation, no Markdown, no code fences.",
        language.code(),
        MAX_WORDS,
        EMPTY_ANSWER
    );
    // Задача названа и до расшифровки, и после неё: на длинном входе
    // первая инструкция теряется, и модель берётся за то, что показалось
    // ей уместным (Epic 8, задача 4 — там это была редактура расшифровки
    // вместо брифа).
    let user = format!(
        "Transcript replies, numbered:\n\n<transcript>\n{}\n</transcript>\n\n\
         Now list the misheard terms as described above, one per line, \
         or the single word `{}` if there are none.",
        format_batch(segments),
        EMPTY_ANSWER
    );

    (system, user)
}

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
///
/// Считается всё отброшенное, а не только принятое: модель, у которой
/// девять предложений из десяти не находятся в тексте, выглядит по
/// одному числу принятых точно так же, как аккуратная.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RejectReport {
    pub out_of_range: usize,
    pub not_in_replica: usize,
    pub too_long: usize,
    pub already_known: usize,
    pub duplicates: usize,
}

impl RejectReport {
    /// Сколько предложений отброшено всего.
    pub fn total(&self) -> usize {
        self.out_of_range
            + self.not_in_replica
            + self.too_long
            + self.already_known
            + self.duplicates
    }
}

/// Разобрать ответ модели.
///
/// `Err` означает «ответ не по формату», и это **не** то же самое, что
/// «исправлений нет»: пустота говорится словом [`EMPTY_ANSWER`]. Слияние
/// этих исходов уже стоило переписывания `EchoReport::empty()` (Epic 16),
/// и здесь оно опаснее: прозаический ответ читался бы как чистая
/// расшифровка.
pub fn parse_fixes(response: &str) -> Result<ParsedFixes, String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "LLM вернул пустой ответ вместо слова «{EMPTY_ANSWER}»"
        ));
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

/// `"3 | кобриаты | ковариаты | рядом «регрессия»"` → [`RawFix`].
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
///
/// Ничего не применяет и не пишет: на выходе — то, что можно показать
/// человеку, и счёт того, что показывать нельзя.
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
                && fix.channel == segment.channel
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

/// Найти в реплике то, что назвала модель, и вернуть форму **из текста**.
///
/// Регистр при поиске не учитывается — модель приводит слово к своему —
/// а возвращается написание расшифровки: одобренная пара уедет в
/// глоссарий, и surface, которого в тексте не было, там ничего не найдёт.
///
/// Совпадение обязано стоять на границах слов. Кусок слова термином быть
/// не может: `term_from_edit` разбирает правку по словам и такой пары не
/// увидит, а замена по куску переписывала бы чужие слова целиком.
fn find_surface(text: &str, needle: &str) -> Option<String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let needle_lower = needle.to_lowercase();
    let needle_len = needle_lower.chars().count();

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for start in 0..chars.len() {
        let end = start + needle_len;
        if end > chars.len() {
            break;
        }
        let byte_start = chars[start].0;
        let byte_end = chars.get(end).map_or(text.len(), |(index, _)| *index);
        let candidate = &text[byte_start..byte_end];
        if candidate.to_lowercase() != needle_lower {
            continue;
        }
        let before = start.checked_sub(1).map(|index| chars[index].1);
        let after = chars.get(end).map(|(_, symbol)| *symbol);
        if is_word_edge(before) && is_word_edge(after) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Символ на границе совпадения: его отсутствие или не-буква.
fn is_word_edge(symbol: Option<char>) -> bool {
    match symbol {
        None => true,
        Some(symbol) => !symbol.is_alphanumeric(),
    }
}

/// Пара уже заведена: предлагать её значит показывать человеку работу,
/// которую он однажды сделал.
fn is_known(known: &[GlossaryTerm], surface: &str, canonical: &str) -> bool {
    known.iter().any(|term| {
        term.surface.to_lowercase() == surface.to_lowercase()
            && term.canonical.to_lowercase() == canonical.to_lowercase()
    })
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, GlossaryKind, GlossaryScope, GlossaryTerm, SpeakerSource};

    use super::*;

    fn segment(index: u32, text: &str) -> FinalSegment {
        FinalSegment {
            index,
            start_ms: u64::from(index) * 1000,
            end_ms: u64::from(index) * 1000 + 900,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_source: SpeakerSource::None,
            text: text.to_string(),
            text_edited: false,
            original_text: String::new(),
        }
    }

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
        let (system, user) = fix_prompts(&[segment(0, "текст")], SpeechLanguage::Ru);

        assert!(system.contains(EMPTY_ANSWER), "{system}");
        // И после расшифровки тоже: на длинном входе действует последнее.
        assert!(user.contains(EMPTY_ANSWER), "{user}");
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
        assert!(
            closing.trim().len() > 20,
            "повтор задачи слишком короток, чтобы что-то значить: {closing}"
        );
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

    /// Язык встречи доезжает до промпта: расшифровка бывает не русской,
    /// и просить термины «как в языке `ru`» на испанской встрече значит
    /// звать модель переводить.
    #[test]
    fn the_meeting_language_reaches_the_prompt() {
        let (system, _) = fix_prompts(&[segment(0, "texto")], SpeechLanguage::Es);

        assert!(system.contains("`es`"), "{system}");
    }

    fn term(surface: &str, canonical: &str) -> GlossaryTerm {
        GlossaryTerm {
            id: format!("t-{surface}"),
            surface: surface.to_string(),
            canonical: canonical.to_string(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Global,
            kind: GlossaryKind::Replacement,
        }
    }

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

    /// Пустой ответ — тоже поломка, а не пустота: сказать «нечего
    /// предложить» модель обязана словом.
    #[test]
    fn silence_is_not_an_empty_answer() {
        assert!(parse_fixes("   \n\n").is_err());
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

        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].surface, "Кобриаты");
        assert_eq!(fixes[0].canonical, "ковариаты");
        assert_eq!(fixes[0].replica_text, segments[0].text);
        // Место едет вместе с парой: без него нечего слушать.
        assert_eq!(fixes[0].start_ms, segments[0].start_ms);
        assert_eq!(fixes[0].channel, segments[0].channel);
    }

    /// Совпадение внутри слова термином быть не может: `term_from_edit`
    /// разбирает правку по словам и такой пары не увидит вовсе, а замена
    /// по куску слова переписывала бы чужие слова целиком.
    #[test]
    fn a_match_inside_a_longer_word_is_not_a_term() {
        let segments = [segment(0, "переносим релиз на среду")];
        let parsed = parse_fixes("1 | рели | релиз | контекст").unwrap();

        let (fixes, report) = resolve_fixes(&parsed, &segments, &[]);

        assert!(fixes.is_empty());
        assert_eq!(report.not_in_replica, 1);
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

    /// Регистр здесь значащий: «униффи» → «UniFFI» отличается только им
    /// и остаётся настоящим исправлением.
    #[test]
    fn a_pair_that_changes_only_case_is_still_a_fix() {
        let segments = [segment(0, "смотри униффи завтра")];
        let parsed = parse_fixes("1 | униффи | UniFFI | название библиотеки").unwrap();

        let (fixes, _) = resolve_fixes(&parsed, &segments, &[]);

        assert_eq!(fixes.len(), 1);
    }

    /// А пара, не меняющая ничего, — не исправление.
    #[test]
    fn a_pair_that_changes_nothing_is_dropped() {
        let segments = [segment(0, "смотри релиз завтра")];
        let parsed = parse_fixes("1 | релиз | релиз | так и есть").unwrap();

        let (fixes, report) = resolve_fixes(&parsed, &segments, &[]);

        assert!(fixes.is_empty());
        assert_eq!(report.not_in_replica, 1);
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

    /// Опора может не прийти. Это законно и само по себе сигнал —
    /// сигнал человеку, а не порогу.
    #[test]
    fn a_missing_reason_does_not_lose_the_pair() {
        let parsed = parse_fixes("1 | кобриаты | ковариаты").unwrap();

        assert_eq!(parsed.fixes.len(), 1);
        assert!(parsed.fixes[0].reason.is_empty());
    }

    /// Одну и ту же пару на одном месте модель повторяет; человеку она
    /// нужна один раз.
    #[test]
    fn the_same_pair_in_the_same_place_is_offered_once() {
        let segments = [segment(0, "смотри униффи завтра")];
        let parsed = parse_fixes("1 | униффи | UniFFI | опора\n1 | униффи | UniFFI | опора")
            .expect("две законные строки");

        let (fixes, report) = resolve_fixes(&parsed, &segments, &[]);

        assert_eq!(fixes.len(), 1);
        assert_eq!(report.duplicates, 1);
    }
}
