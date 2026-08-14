//! Разделение голосов внутри одной дорожки.
//!
//! Спека — `docs/superpowers/specs/2026-08-11-voice-clustering-design.md`,
//! план — `docs/superpowers/plans/2026-08-11-voice-clustering.md`.
//!
//! Здесь только **граница**: что такое отрезок речи с меткой голоса, что
//! отдаёт проход по дорожке и кто его делает. Модели нет ни одной, и это
//! не недоделка: какую связку тянуть в приложение, решает замер на Маке
//! (задача 3 плана), а не рассуждение. Крейт устроен так же, как `stt` до
//! whisper: без фичи отдаётся заглушка, и весь остальной код собирается и
//! проверяется на Linux.
//!
//! Имён здесь нет и не будет. Диаризация отвечает на вопрос «в этом куске
//! говорит другой человек», а не «кого зовут Пётр»: имя ставит человек
//! (задача 6 плана) либо слепок, включённый осознанно (задача 7).

mod enroll;
mod mock;
mod model_path;
mod voiceprint;

#[cfg(feature = "model")]
mod sherpa;

pub use enroll::{
    Assignment, DEFAULT_ACCEPT, DEFAULT_MARGIN, EnrollPlan, Reply, plan as plan_enrollment,
    plan_known,
};
pub use mock::MockDiarizer;
pub use model_path::{
    DiarizeModels, EMBEDDING_FILE, SEGMENTATION_FILE, diarize_models_dir, embedding_model_id,
    resolve_diarize_models,
};
pub use voiceprint::{Match, VoiceEmbedder, VoicePrint, best_match, build_print, similarity};

#[cfg(feature = "model")]
pub use sherpa::{
    SherpaDiarizer, SherpaEmbedder, requested_provider, threads_in_use, voice_embedder,
};

use std::path::Path;

/// Отрезок речи с меткой голоса.
///
/// Метка — номер кластера внутри одного прохода, а не идентификатор
/// человека: тот же голос в следующей встрече получит другой номер.
/// Связывать номера между встречами — работа слепков (задача 7 плана), и
/// она включается отдельно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceTurn {
    pub start_ms: u64,
    pub end_ms: u64,
    pub cluster: u32,
}

impl VoiceTurn {
    pub fn new(start_ms: u64, end_ms: u64, cluster: u32) -> Self {
        Self {
            start_ms,
            end_ms,
            cluster,
        }
    }

    /// Длительность отрезка; перевёрнутый отрезок даёт ноль, а не панику.
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Результат прохода по дорожке.
///
/// `refused` — отдельное поле, а не пустой список отрезков, и это несущая
/// деталь. Сломанный движок, не нашедший ни одной смены голоса, выглядит
/// ровно как честный проход по монологу; слить эти два ответа в один
/// значит получить прибор, который молчит там, где должен кричать. Та же
/// история, что с `EchoReport::empty()` в Epic 16: ноль стоял и когда
/// искать было нечем, и когда искали и не нашли.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiarizeReport {
    pub turns: Vec<VoiceTurn>,
    /// Сколько разных голосов нашлось. Считается по меткам, а не
    /// принимается снаружи: разойдясь с отрезками, это число врало бы
    /// молча.
    pub speakers_found: u32,
    pub refused: Option<String>,
}

impl DiarizeReport {
    /// Проход состоялся: отрезки такие, число голосов — по меткам.
    ///
    /// Пустой список здесь законен и означает «речи не нашли», а не
    /// отказ. Отказ — это `refused`.
    pub fn from_turns(turns: Vec<VoiceTurn>) -> Self {
        let mut labels: Vec<u32> = turns.iter().map(|turn| turn.cluster).collect();
        labels.sort_unstable();
        labels.dedup();
        Self {
            speakers_found: labels.len() as u32,
            turns,
            refused: None,
        }
    }

    /// Прохода не было. Причина обязательна и уезжает наверх текстом.
    pub fn refused(reason: impl Into<String>) -> Self {
        Self {
            turns: Vec::new(),
            speakers_found: 0,
            refused: Some(reason.into()),
        }
    }

    pub fn is_refused(&self) -> bool {
        self.refused.is_some()
    }

    /// Сколько всего времени занято речью — сумма отрезков.
    pub fn speech_ms(&self) -> u64 {
        self.turns.iter().map(VoiceTurn::duration_ms).sum()
    }
}

/// Движок разделения голосов.
///
/// Проход идёт по всей дорожке разом: живой диаризации в проекте нет и не
/// планируется — бюджет живого пути расписан по миллисекундам (ADR-010),
/// и второй проход в него не влезает.
pub trait Diarizer {
    fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport;

    /// Переставить порог, по которому голоса считаются разными.
    ///
    /// `false` — у этого движка порога нет вовсе, и это честный ответ, а
    /// не отказ: у заглушки его действительно нет. Умолчание врать не
    /// может по построению — соврало бы `true` без последствий, и
    /// развёртка по порогу печатала бы одно и то же число под разными
    /// заголовками.
    fn set_cluster_threshold(&mut self, _threshold: f32) -> bool {
        false
    }
}

/// Движок по сборке и по тому, что лежит на диске.
///
/// Устроено как `LiveCaptionPipeline::from_data_root`: выбор делает крейт,
/// а не вызывающий, — иначе каждый потребитель заводил бы свою ветку и
/// свою ошибку. Отличие одно, и оно намеренное: там отсутствие модели
/// молча подменяется мокой с текстом-подделкой, здесь — заглушкой, которая
/// **отказывает и говорит почему**. Подделка под текст безобидна, подделка
/// под разделение голосов неотличима от правды.
///
/// Ни одна ветка не паникует и не возвращает `Result`: причина уезжает
/// внутри отчёта, туда же, куда уедет отказ самого движка.
pub fn diarize_backend(data_root: impl AsRef<Path>) -> Box<dyn Diarizer> {
    let _ = data_root.as_ref();

    #[cfg(feature = "model")]
    {
        match resolve_diarize_models(data_root.as_ref()) {
            Ok(models) => match SherpaDiarizer::open(&models) {
                Ok(engine) => Box::new(engine),
                Err(error) => Box::new(MockDiarizer::because(error)),
            },
            Err(error) => Box::new(MockDiarizer::because(error)),
        }
    }
    #[cfg(not(feature = "model"))]
    {
        Box::new(MockDiarizer::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Движка нет — ни без фичи, ни с фичей и без моделей на диске, — и
    /// прибор обязан узнать об этом из отчёта, а не из пустого списка
    /// отрезков.
    ///
    /// Каталог заведомо пустой: с фичей это ветка «моделей нет», без
    /// фичи — «сборка без движка». Обе обязаны отказать с причиной.
    #[test]
    fn a_backend_without_an_engine_refuses_with_a_reason() {
        let empty = std::env::temp_dir().join(format!(
            "mr-diarize-no-models-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&empty);
        let mut engine = diarize_backend(&empty);

        let report = engine.diarize(&vec![0i16; 16_000], 16_000);

        let reason = report.refused.expect("заглушка обязана отказать");
        assert!(
            !reason.trim().is_empty(),
            "отказ без причины — то же молчание"
        );
        assert!(
            report.turns.is_empty(),
            "отказ не имеет права нести отрезки: их никто не считал"
        );
    }

    /// Число голосов считается по меткам. Повтор метки — тот же голос.
    #[test]
    fn speakers_are_counted_by_distinct_labels() {
        let report = DiarizeReport::from_turns(vec![
            VoiceTurn::new(0, 1_000, 0),
            VoiceTurn::new(1_000, 2_000, 1),
            VoiceTurn::new(2_000, 3_000, 0),
        ]);

        assert_eq!(report.speakers_found, 2);
        assert_eq!(report.speech_ms(), 3_000);
    }

    /// «Речи не нашли» и «проход не состоялся» — разные ответы, и
    /// различать их обязан тип, а не читатель.
    #[test]
    fn an_empty_pass_is_not_a_refusal() {
        let empty = DiarizeReport::from_turns(Vec::new());
        assert!(!empty.is_refused());
        assert_eq!(empty.speakers_found, 0);

        let refused = DiarizeReport::refused("движка нет");
        assert!(refused.is_refused());
        assert_eq!(refused.speakers_found, 0);
        assert_ne!(empty, refused, "два разных ответа слились в один");
    }

    /// Перевёрнутый отрезок даёт ноль: длительность считается вычитанием,
    /// и без насыщения проход по чужим данным падал бы паникой.
    #[test]
    fn a_reversed_turn_lasts_nothing() {
        assert_eq!(VoiceTurn::new(2_000, 1_000, 0).duration_ms(), 0);
    }
}
