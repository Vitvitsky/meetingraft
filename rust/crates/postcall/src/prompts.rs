//! Инструкции модели, собирающей артефакты (Epic 8, задача 4).
//!
//! Модель получает не текст вообще, а **расшифровку встречи**, и знать
//! про это она должна из промпта: иначе абзацы с жирным префиксом
//! читаются как разметка, а не как имена говорящих, и бриф начинает
//! приписывать решения кому попало.
//!
//! Три вещи говорятся здесь и повторяются в обоих промптах:
//!
//! 1. **что это за текст** — расшифровка, полученная распознаванием, а
//!    значит слова в ней бывают услышаны неверно;
//! 2. **откуда берутся имена** — `**Имя:** реплика`; реплика без имени
//!    не приписана никому, и назначать ей автора нельзя;
//! 3. **чего делать нельзя** — придумывать факты, решения, поручения и
//!    сроки, которых в расшифровке нет.
//!
//! Четвёртое добавлено 2026-08-21, после первого живого прогона на
//! `gemma` 12B: модель **не сделала бриф вовсе**. Вместо него она выдала
//! отредактированную расшифровку с преамбулой «Ниже представлен
//! отредактированный текст…» и математической разметкой `$\rightarrow$`
//! в перечислении исправленных слов.
//!
//! Отсюда два правила и одна перестановка.
//!
//! **Задача повторяется после расшифровки.** Названная только до
//! тридцати тысяч знаков, она к концу перестаёт действовать: модель
//! помнит последнюю инструкцию лучше первой. Ради этого `</transcript>`
//! больше не конец сообщения.
//!
//! **Форма вывода описывается явно.** «Верни Markdown» ничего не
//! запрещает: ни преамбулы, ни рассказа о проделанной работе, ни LaTeX.
//! Промпт, не запрещающий лишнего, получает лишнее.
//!
//! Проверить этим кодом можно только наличие инструкций. Выполняет ли их
//! модель — вопрос к живому прогону (задача 5), а не к тесту.

use domain::SpeechLanguage;

/// Общая часть: что за текст перед моделью и как читать имена.
///
/// Одной строкой на оба промпта, потому что расходиться им нельзя: бриф
/// и follow-up собираются из одной расшифровки, и разное описание
/// источника означало бы, что один из них описан неверно.
fn transcript_preamble() -> &'static str {
    "You are given a transcript of a meeting produced by automatic speech recognition, \
     so some words may be misheard. Replies may be prefixed with the speaker name in bold \
     (`**Name:** reply`); a reply without a name was not attributed to anyone, and you must \
     not guess who said it."
}

/// Что запрещено в выводе, независимо от вида артефакта.
///
/// Отдельной строкой на оба промпта по той же причине, что и преамбула:
/// разойдись они, один из артефактов остался бы без запрета, и понять
/// это можно было бы только по испорченному выводу.
fn output_rules() -> &'static str {
    "Return only the document itself. Do not add a preamble, do not explain what you did, \
     do not comment on the transcript quality, and do not add closing remarks. \
     Do not reproduce or re-edit the transcript: summarise it. \
     Use plain Markdown only — no LaTeX, no math notation, no code fences around the answer."
}

/// Формирует инструкции для краткого итога встречи.
pub fn brief_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String) {
    let system = format!(
        "{} Create a concise meeting brief in language `{}`. \
         Structure it with these Markdown headings, in this order: \
         `## Кратко`, `## Решения`, `## Обсудили`. \
         Attribute each decision and proposal to the person who voiced it, using the name \
         as it appears in the transcript. \
         Do not invent facts, decisions, or speakers absent from the transcript. {}",
        transcript_preamble(),
        primary_lang.code(),
        output_rules()
    );
    // Задача названа и до расшифровки, и после неё. На длинном входе
    // первая инструкция теряется, и модель берётся за то, что показалось
    // ей уместным, — на живом прогоне за редактуру расшифровки.
    let user = format!(
        "Create the meeting brief from this final transcript:\n\n\
         <transcript>\n{final_md}\n</transcript>\n\n\
         Now write the brief described above. Output the brief only."
    );

    (system, user)
}

/// Формирует инструкции для follow-up письма по итогам встречи.
pub fn follow_up_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String) {
    let system = format!(
        "{} You are a meeting assistant. Draft a follow-up email in language `{}` as Markdown. \
         Start with the subject line in an HTML comment, then include a greeting, \
         a concise meeting summary, explicitly stated next steps, and a closing. \
         List a next step only if someone said it out loud, and name the person who committed \
         to it, using the name as it appears in the transcript; if the transcript does not say \
         who took it on, write that it is unassigned instead of guessing. \
         Keep a deadline only if it was spoken; do not turn a vague phrase into a date. \
         Do not invent facts, assignments, or deadlines absent from the transcript. {}",
        transcript_preamble(),
        primary_lang.code(),
        output_rules()
    );
    let user = format!(
        "Draft a follow-up email from this final transcript:\n\n\
         <transcript>\n{final_md}\n</transcript>\n\n\
         Now write the email described above. Output the email only."
    );

    (system, user)
}

#[cfg(test)]
mod tests {
    use domain::SpeechLanguage;

    use super::{brief_prompts, follow_up_prompts};

    /// Тесты здесь утверждают, что инструкция **сказана**, а не что она
    /// выполнена. Выполнение проверяется живым прогоном на своей записи
    /// (задача 5 плана): по тексту промпта о поведении модели судить
    /// нельзя, и делать вид, что можно, — то же самое, что верить
    /// непроверенному прибору.
    #[test]
    fn brief_prompts_request_markdown_in_primary_language() {
        let (system, user) =
            brief_prompts("# Итоговый транскрипт\nОбсудили релиз.", SpeechLanguage::Ru);

        assert!(system.contains("Markdown"));
        assert!(system.contains("ru"));
        assert!(user.contains("# Итоговый транскрипт\nОбсудили релиз."));
    }

    #[test]
    fn both_prompts_say_what_kind_of_text_this_is() {
        // Без этого жирный префикс читается как разметка, а не как имя
        // говорящего, и решения расходятся не тем людям.
        for (system, _) in [
            brief_prompts("текст", SpeechLanguage::Ru),
            follow_up_prompts("текст", SpeechLanguage::Ru),
        ] {
            assert!(system.contains("transcript of a meeting"), "{system}");
            assert!(system.contains("speech recognition"), "{system}");
            assert!(system.contains("**Name:**"), "{system}");
            assert!(
                system.contains("not attributed"),
                "реплика без имени не должна получать автора: {system}"
            );
        }
    }

    #[test]
    fn the_brief_attributes_decisions_to_the_person_who_voiced_them() {
        let (system, _) = brief_prompts("текст", SpeechLanguage::Ru);
        assert!(system.contains("Attribute each decision"), "{system}");
        assert!(system.contains("Do not invent"), "{system}");
    }

    #[test]
    fn follow_up_prompts_request_email_with_expected_structure() {
        let (system, user) =
            follow_up_prompts("# Final transcript\nSchedule the demo.", SpeechLanguage::En);

        assert!(system.contains("Markdown"));
        assert!(system.contains("en"));
        assert!(system.contains("follow-up email"));
        assert!(system.contains("subject"));
        assert!(system.contains("greeting"));
        assert!(system.contains("summary"));
        assert!(system.contains("closing"));
        assert!(!system.contains("follow-up actions"));
        assert!(user.contains("Draft a follow-up email"));
        assert!(user.contains("# Final transcript\nSchedule the demo."));
    }

    /// Первый живой прогон дал не бриф, а отредактированную расшифровку
    /// с преамбулой и `$\rightarrow$` в тексте. Промпт, не запрещающий
    /// лишнего, получает лишнее.
    #[test]
    fn both_prompts_forbid_preamble_and_math_notation() {
        for (system, _) in [
            brief_prompts("текст", SpeechLanguage::Ru),
            follow_up_prompts("текст", SpeechLanguage::Ru),
        ] {
            assert!(
                system.contains("Return only the document itself"),
                "{system}"
            );
            assert!(system.contains("Do not add a preamble"), "{system}");
            assert!(system.contains("no LaTeX"), "{system}");
            assert!(
                system.contains("Do not reproduce or re-edit the transcript"),
                "{system}"
            );
        }
    }

    /// Задача обязана стоять и после расшифровки: названная только до
    /// неё, на длинном входе она перестаёт действовать.
    #[test]
    fn both_prompts_restate_the_task_after_the_transcript() {
        for (_, user) in [
            brief_prompts("длинная расшифровка", SpeechLanguage::Ru),
            follow_up_prompts("длинная расшифровка", SpeechLanguage::Ru),
        ] {
            let closing = user
                .split("</transcript>")
                .nth(1)
                .expect("расшифровка обязана быть закрыта тегом");
            assert!(
                closing.contains("Now write"),
                "после расшифровки нет повтора задачи: {user}"
            );
            assert!(
                closing.trim().len() > 20,
                "повтор задачи слишком короток, чтобы что-то значить: {closing}"
            );
        }
    }

    #[test]
    fn a_next_step_carries_the_name_of_whoever_took_it_on() {
        // Обещание без имени — половина обещания: письмо, в котором
        // непонятно, кто что делает, хуже, чем письмо без этой строки.
        let (system, _) = follow_up_prompts("текст", SpeechLanguage::Ru);
        assert!(system.contains("name the person who committed"), "{system}");
        assert!(
            system.contains("unassigned"),
            "неназванного исполнителя нельзя выдумывать: {system}"
        );
        assert!(
            system.contains("Keep a deadline only if it was spoken"),
            "{system}"
        );
    }
}
