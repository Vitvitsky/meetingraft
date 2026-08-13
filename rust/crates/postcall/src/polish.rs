//! Полировка сегментов Final через LLM (Phase 10, T5).
//!
//! Задача узкая: расставить пунктуацию и заглавные, разбить на читаемые
//! предложения. Не пересказывать, не сокращать, не додумывать. Границы
//! сегментов и тайм-коды не трогаются вовсе — полировка меняет текст
//! **внутри** сегмента, потому что на этих границах Phase 11 будет
//! держать спикеров.
//!
//! Тихого фолбэка нет: если LLM недоступен или ответил не по формату,
//! сегменты остаются как после re-ASR, а вызывающий узнаёт об этом из
//! `PolishReport` и говорит правду в provenance. Молча отдать
//! непричёсанный текст под видом отполированного — ровно та ошибка, из-за
//! которой нынешний Final выдавали за рефайнмент.

use domain::{FinalSegment, SpeechLanguage};

use crate::llm::{LlmClient, LlmError};

/// Сколько сегментов отдаём за один запрос.
///
/// Батч даёт модели контекст соседних реплик, но чем он больше, тем
/// дороже промах: при поломке формата откатывается весь батч.
pub const DEFAULT_BATCH_SIZE: usize = 12;

/// Что произошло с полировкой — вход для provenance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PolishReport {
    pub polished_batches: usize,
    /// Батчи, оставленные как после re-ASR.
    pub kept_batches: usize,
    /// Первая причина отказа; пусто, если всё прошло.
    pub error: Option<String>,
}

impl PolishReport {
    /// Полировка применена ко всему тексту без исключений.
    pub fn is_complete(&self) -> bool {
        self.kept_batches == 0 && self.error.is_none() && self.polished_batches > 0
    }
}

/// Отполировать сегменты, сохранив их количество и границы.
pub fn polish_segments(
    segments: Vec<FinalSegment>,
    language: SpeechLanguage,
    batch_size: usize,
    client: &dyn LlmClient,
) -> (Vec<FinalSegment>, PolishReport) {
    let mut report = PolishReport::default();
    if segments.is_empty() {
        return (segments, report);
    }
    let batch_size = batch_size.max(1);

    let mut out: Vec<FinalSegment> = Vec::with_capacity(segments.len());
    for batch in segments.chunks(batch_size) {
        match polish_batch(batch, language, client) {
            Ok(texts) => {
                report.polished_batches += 1;
                for (segment, text) in batch.iter().zip(texts) {
                    out.push(FinalSegment {
                        text,
                        ..segment.clone()
                    });
                }
            }
            Err(reason) => {
                report.kept_batches += 1;
                report.error.get_or_insert(reason);
                out.extend(batch.iter().cloned());
            }
        }
    }
    (out, report)
}

fn polish_batch(
    batch: &[FinalSegment],
    language: SpeechLanguage,
    client: &dyn LlmClient,
) -> Result<Vec<String>, String> {
    let (system, user) = polish_prompts(batch, language);
    let response = client
        .complete(&system, &user)
        .map_err(|error: LlmError| error.to_string())?;
    parse_polished(&response, batch.len())
}

/// Инструкции полировки. Нумерация в обе стороны — так ответ можно
/// сверить с запросом, а не доверять порядку строк.
pub fn polish_prompts(batch: &[FinalSegment], language: SpeechLanguage) -> (String, String) {
    let system = format!(
        "You clean up raw speech-to-text output in language `{}`. \
         For each numbered line: restore punctuation, capitalization and sentence breaks. \
         Never translate, summarize, reorder, merge, split or invent content. \
         Keep the wording as spoken. \
         Return exactly {} lines, each starting with its original number and a dot, \
         in the same order. Return nothing else.",
        language.code(),
        batch.len()
    );
    let user = format!("Clean up these lines:\n\n{}", format_batch(batch));
    (system, user)
}

/// Пронумерованный список сегментов для запроса.
pub fn format_batch(batch: &[FinalSegment]) -> String {
    batch
        .iter()
        .enumerate()
        .map(|(index, segment)| format!("{}. {}", index + 1, segment.text.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Разобрать ответ и убедиться, что он соответствует запросу.
///
/// Проверяется именно соответствие, а не «похожесть»: несовпадение числа
/// строк или пропуск номера означает, что модель что-то склеила или
/// выдумала, и такой ответ применять к сегментам нельзя.
pub fn parse_polished(response: &str, expected: usize) -> Result<Vec<String>, String> {
    let mut slots: Vec<Option<String>> = vec![None; expected];

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((number, text)) = split_numbered(line) else {
            continue;
        };
        if number == 0 || number > expected {
            return Err(format!("LLM вернул строку с номером {number} вне запроса"));
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(format!("LLM вернул пустую строку {number}"));
        }
        if slots[number - 1].is_some() {
            return Err(format!("LLM продублировал строку {number}"));
        }
        slots[number - 1] = Some(text.to_string());
    }

    let mut out = Vec::with_capacity(expected);
    for (index, slot) in slots.into_iter().enumerate() {
        match slot {
            Some(text) => out.push(text),
            None => return Err(format!("LLM пропустил строку {}", index + 1)),
        }
    }
    Ok(out)
}

/// `"12. текст"` → `(12, "текст")`.
fn split_numbered(line: &str) -> Option<(usize, &str)> {
    let (number, rest) = line.split_once('.')?;
    let number = number.trim().parse::<usize>().ok()?;
    Some((number, rest))
}

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, SpeakerSource};

    use super::*;

    struct FixedLlm(Result<String, LlmError>);

    impl LlmClient for FixedLlm {
        fn complete(&self, _system: &str, _user: &str) -> Result<String, LlmError> {
            self.0.clone()
        }
    }

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
    fn polished_text_replaces_segment_text_only() {
        let segments = vec![segment(0, "привет команда"), segment(1, "начнём")];
        let llm = FixedLlm(Ok("1. Привет, команда.\n2. Начнём.".into()));

        let (out, report) = polish_segments(segments, SpeechLanguage::Ru, 12, &llm);

        assert_eq!(out[0].text, "Привет, команда.");
        assert_eq!(out[1].text, "Начнём.");
        // Границы и метаданные не тронуты.
        assert_eq!(out[0].start_ms, 0);
        assert_eq!(out[1].start_ms, 1000);
        assert_eq!(out[1].index, 1);
        assert_eq!(out[0].channel, AudioChannel::Mic);
        assert!(report.is_complete());
    }

    /// Недоступный LLM не должен терять транскрипт.
    #[test]
    fn llm_failure_keeps_original_text() {
        let segments = vec![segment(0, "как есть")];
        let llm = FixedLlm(Err(LlmError::NotConfigured));

        let (out, report) = polish_segments(segments, SpeechLanguage::Ru, 12, &llm);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "как есть");
        assert_eq!(report.kept_batches, 1);
        assert_eq!(report.polished_batches, 0);
        assert!(report.error.is_some());
        assert!(!report.is_complete(), "provenance не должен врать");
    }

    /// Склеенный ответ откатывает батч целиком, а не портит выравнивание.
    #[test]
    fn merged_response_is_rejected() {
        let segments = vec![segment(0, "раз"), segment(1, "два")];
        let llm = FixedLlm(Ok("1. Раз, два.".into()));

        let (out, report) = polish_segments(segments, SpeechLanguage::Ru, 12, &llm);

        assert_eq!(out[0].text, "раз");
        assert_eq!(out[1].text, "два");
        assert_eq!(report.kept_batches, 1);
    }

    #[test]
    fn segment_count_never_changes() {
        let segments: Vec<FinalSegment> = (0..25).map(|i| segment(i, "текст")).collect();
        let llm = FixedLlm(Err(LlmError::EmptyResponse));

        let (out, _) = polish_segments(segments, SpeechLanguage::Ru, 12, &llm);

        assert_eq!(out.len(), 25);
    }

    /// Один плохой батч не отменяет удачные — но и не даёт врать в provenance.
    #[test]
    fn batches_are_reported_separately() {
        let segments: Vec<FinalSegment> = (0..3).map(|i| segment(i, "текст")).collect();
        let llm = FixedLlm(Ok("1. Текст.".into()));

        let (out, report) = polish_segments(segments, SpeechLanguage::Ru, 1, &llm);

        assert_eq!(out.len(), 3);
        assert_eq!(report.polished_batches, 3);
        assert!(report.is_complete());
    }

    #[test]
    fn empty_input_is_not_an_error() {
        let llm = FixedLlm(Err(LlmError::NotConfigured));

        let (out, report) = polish_segments(Vec::new(), SpeechLanguage::Ru, 12, &llm);

        assert!(out.is_empty());
        assert_eq!(report, PolishReport::default());
        assert!(!report.is_complete(), "полировать было нечего");
    }

    #[test]
    fn parse_accepts_out_of_order_numbering() {
        let parsed = parse_polished("2. Второе.\n1. Первое.", 2).expect("порядок строк не важен");

        assert_eq!(parsed, vec!["Первое.", "Второе."]);
    }

    #[test]
    fn parse_ignores_chatter_around_the_list() {
        let parsed =
            parse_polished("Вот результат:\n1. Раз.\n\nГотово.", 1).expect("шум игнорируется");

        assert_eq!(parsed, vec!["Раз."]);
    }

    #[test]
    fn parse_rejects_missing_duplicate_and_out_of_range_lines() {
        assert!(parse_polished("1. Раз.", 2).is_err(), "пропущена строка");
        assert!(parse_polished("1. Раз.\n1. Ещё раз.", 1).is_err(), "дубль");
        assert!(
            parse_polished("3. Третье.", 2).is_err(),
            "номер вне запроса"
        );
        assert!(parse_polished("1.   ", 1).is_err(), "пустой текст");
    }

    #[test]
    fn format_batch_numbers_from_one() {
        let batch = vec![segment(5, " раз "), segment(6, "два")];

        assert_eq!(format_batch(&batch), "1. раз\n2. два");
    }
}
