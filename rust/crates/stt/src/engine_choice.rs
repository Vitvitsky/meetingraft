//! Каким движком распознавать запись в пост-обработке.
//!
//! Правило живёт здесь, а не в `ffi`, по той же причине, по которой там
//! не живёт ничего доменного: выбор между движками — это знание о
//! движках, и проверяется оно обычным `cargo test` без Мака и без
//! скачанных моделей.
//!
//! ## Почему `Auto` смотрит на primary, а не на allowed
//!
//! `LanguagePolicy.allowed` по ADR-003 **всегда** `{ru, en, es}`:
//! «встреча только по-русски» в модели не выражается вовсе. Значит
//! единственный сигнал — `primary`, и это не приближение к желаемому, а
//! всё, что есть. Отсюда же цена автоматики, названная прямо:
//!
//! **В словаре GigaAM нет латиницы.** Английский термин в русской
//! встрече он не выдаст ни в каком виде — ни правильно, ни узнаваемо
//! испорченным. Поэтому `Auto` включает его только там, где человек
//! **сам скачал модель** отдельным скриптом: другого признака согласия
//! у нас нет.
//!
//! ## Почему явный выбор не подменяется молча
//!
//! Выбран GigaAM, а модели нет — это ошибка, видимая человеку, а не
//! тихий откат на Whisper. Расшифровка, сделанная не тем движком, о
//! котором просили, ничем себя не выдаёт: текст на месте, и разница
//! видна только тому, кто помнит, что настраивал.

use std::path::Path;

use domain::{PostCallRecognizer, SpeechLanguage};

use crate::gigaam_path::resolve_gigaam_models;

/// Движок, которым в итоге пойдёт проход. `Auto` сюда не попадает: это
/// правило, а не движок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchEngine {
    Whisper,
    Gigaam,
}

/// Что выбрано и почему. Причина едет в provenance расшифровки: человек
/// обязан видеть, чем распознана его встреча, не открывая настроек.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineDecision {
    pub engine: BatchEngine,
    pub reason: String,
}

/// Готов ли GigaAM к работе **на этой сборке и на этой машине**.
///
/// Два условия, и оба обязательны: фича собрана и файлы модели на месте.
/// Без первого движка нет в бинаре вовсе, без второго — нечего грузить.
pub fn gigaam_ready(data_root: impl AsRef<Path>) -> bool {
    cfg!(feature = "gigaam") && resolve_gigaam_models(data_root).is_ok()
}

/// Выбрать движок по настройке и языку сессии.
///
/// `Err` — только там, где просьбу нельзя выполнить: явный GigaAM без
/// модели. Всё остальное решается и объясняется.
pub fn decide_batch_engine(
    preference: PostCallRecognizer,
    primary: SpeechLanguage,
    gigaam_ready: bool,
) -> Result<EngineDecision, String> {
    let whisper = |reason: String| {
        Ok(EngineDecision {
            engine: BatchEngine::Whisper,
            reason,
        })
    };

    match preference {
        PostCallRecognizer::Whisper => whisper("выбран вручную".to_string()),

        PostCallRecognizer::Gigaam if !gigaam_ready => Err(
            "выбран русский движок GigaAM, но его модели нет: скачать — \
             scripts/fetch-gigaam-models.sh <каталог-данных>"
                .to_string(),
        ),
        PostCallRecognizer::Gigaam => Ok(EngineDecision {
            engine: BatchEngine::Gigaam,
            // На нерусской сессии это законный выбор человека, но
            // сказать о нём надо громко: латиницы у движка нет.
            reason: if primary == SpeechLanguage::Ru {
                "выбран вручную".to_string()
            } else {
                format!(
                    "выбран вручную, хотя язык сессии — {}, а движок знает только русский",
                    primary.code()
                )
            },
        }),

        PostCallRecognizer::Auto if primary != SpeechLanguage::Ru => whisper(format!(
            "по языку сессии: {} — русский движок не подходит",
            primary.code()
        )),
        PostCallRecognizer::Auto if !gigaam_ready => {
            whisper("по языку сессии подошёл бы GigaAM, но его модель не скачана".to_string())
        }
        PostCallRecognizer::Auto => Ok(EngineDecision {
            engine: BatchEngine::Gigaam,
            reason: "по языку сессии: русский".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(
        preference: PostCallRecognizer,
        primary: SpeechLanguage,
        ready: bool,
    ) -> EngineDecision {
        decide_batch_engine(preference, primary, ready).expect("выбор состоялся")
    }

    #[test]
    fn a_russian_session_with_the_model_downloaded_goes_to_gigaam() {
        let decision = decide(PostCallRecognizer::Auto, SpeechLanguage::Ru, true);
        assert_eq!(decision.engine, BatchEngine::Gigaam);
        assert!(decision.reason.contains("язык"), "{decision:?}");
    }

    /// Модель качается руками. Нет её — правило молчит и уступает
    /// Whisper, но причина остаётся в тексте: иначе непонятно, почему
    /// «авто» выбрало не то, что обещало.
    #[test]
    fn without_the_model_auto_falls_back_and_says_so() {
        let decision = decide(PostCallRecognizer::Auto, SpeechLanguage::Ru, false);
        assert_eq!(decision.engine, BatchEngine::Whisper);
        assert!(decision.reason.contains("не скачана"), "{decision:?}");
    }

    /// Английская и испанская сессии на русский движок не уходят даже с
    /// готовой моделью.
    #[test]
    fn other_languages_stay_on_whisper_even_when_gigaam_is_ready() {
        for language in [SpeechLanguage::En, SpeechLanguage::Es] {
            let decision = decide(PostCallRecognizer::Auto, language, true);
            assert_eq!(decision.engine, BatchEngine::Whisper, "{language:?}");
            assert!(
                decision.reason.contains(language.code()),
                "причина не назвала язык: {decision:?}"
            );
        }
    }

    /// Явный Whisper сильнее правила: русская сессия с готовой моделью
    /// всё равно идёт на Whisper.
    #[test]
    fn an_explicit_choice_overrides_the_rule() {
        let decision = decide(PostCallRecognizer::Whisper, SpeechLanguage::Ru, true);
        assert_eq!(decision.engine, BatchEngine::Whisper);
    }

    /// Просили GigaAM, модели нет — отказ, а не тихая подмена движка.
    #[test]
    fn an_explicit_gigaam_without_the_model_is_refused_out_loud() {
        let error = decide_batch_engine(PostCallRecognizer::Gigaam, SpeechLanguage::Ru, false)
            .expect_err("модели нет");
        assert!(error.contains("fetch-gigaam-models"), "{error}");
    }

    /// Явный GigaAM на английской сессии выполняется — это решение
    /// человека, — но причина обязана назвать несоответствие. Молчаливое
    /// исполнение здесь означало бы расшифровку без единого английского
    /// слова и никакого объяснения почему.
    #[test]
    fn an_explicit_gigaam_on_a_non_russian_session_is_obeyed_but_flagged() {
        let decision = decide(PostCallRecognizer::Gigaam, SpeechLanguage::En, true);
        assert_eq!(decision.engine, BatchEngine::Gigaam);
        assert!(decision.reason.contains("только русский"), "{decision:?}");
    }
}
