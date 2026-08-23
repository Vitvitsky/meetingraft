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

use domain::{FinalSegment, SpeechLanguage};

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

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, SpeakerSource};

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
}
