//! Русский распознаватель GigaAM v3 через sherpa-onnx.
//!
//! Модель — Conformer-transducer (RNNT) от Sber под MIT, экспортированный
//! в ONNX для sherpa-onnx. Спека:
//! `docs/superpowers/specs/2026-08-26-gigaam-v3-russian-stt-design.md`.
//!
//! **Это не замена Whisper.** GigaAM понимает только русский, а ADR-003
//! требует ru/en/es и смешанные встречи. Здесь он живёт как второй
//! движок, и на этом шаге — только для post-call прохода (`ADR-011`):
//! живой путь с его local agreement, пейсингом и обрезкой буфера
//! настроен под поведение Whisper, и подставлять туда другой движок до
//! замеров значило бы менять две вещи разом.
//!
//! ## Что здесь проверено замером, а не рассуждением
//!
//! Первым в этом модуле стояла настройка `feat_config.feature_dim = 64`
//! с уверенным комментарием: у модели 64 мел-полосы (так и есть,
//! `test-onnx-rnnt.py` того же экспорта), умолчание крейта — 80, значит
//! без правки движок считает по неверным признакам.
//!
//! **Прогон это опроверг.** Подстановка 80 не изменила ни текста, ни
//! тайм-кодов; подстановка заведомо абсурдных 13 — тоже. Параметры
//! признаков sherpa-onnx берёт из метаданных ONNX, а `feat_config` для
//! этого экспорта не влияет ни на что. Настройки здесь поэтому нет:
//! строка, которая ничего не делает, но выглядит важной, — это ложная
//! уверенность, и стоит она дороже отсутствующей.
//!
//! Тем же прогоном снято ещё два свойства, оба полезные:
//!
//! - **Громкость движку безразлична.** Тот же звук, приглушённый в сто
//!   раз, распознан слово в слово: NeMo нормализует признаки. Тихий
//!   микрофон этот движок не ломает.
//! - **А частота — нет.** Заявленные 32 кГц вместо 16 (sherpa честно
//!   ресемплит к 16) дали пустую расшифровку. Ошибка в частоте — не
//!   «чуть хуже», а «ничего», и прибор её ловит.
//!
//! ## Два места, где ошибиться легко и молча
//!
//! 1. **Токены посимвольные.** Отсюда `words_from_char_tokens`, а не
//!    соседний `words_from_tokens`: тот рассчитан на subword-токены
//!    Whisper и склеит фразу в одно слово (см. тест рядом с ним).
//! 2. **Пунктуации движок не ставит.** Вариант экспорта `e2e` её ставит,
//!    но её же ставит наш `postcall::polish`, и брать оба разом — значит
//!    развести две правды об одном.

use std::path::Path;

use domain::TranscriptSegment;
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineStream, OfflineTransducerModelConfig,
};

use crate::batch::{BatchTranscribeError, BatchTranscriber, normalize_segments};
use crate::gigaam_path::resolve_gigaam_models;
use crate::local_agreement::{HypothesisWord, words_from_char_tokens};

/// Ширина окна пакетного прохода.
///
/// Модель офлайновая и длинное аудио держит, но час встречи одним куском
/// — это непредсказуемая память на энкодере и ни одной точки отмены.
/// Тридцать секунд взяты не из свойств GigaAM, а ради одинаковости с
/// `whisper_batch`: сравнивать два движка проще, когда режут они
/// одинаково. Шов на границе окна тот же, что и у соседа: слово,
/// попавшее на стык, может потеряться.
const WINDOW_SECONDS: usize = 30;

/// Сколько потоков отдать onnxruntime.
///
/// Переопределяется `MEETINGRAFT_GIGAAM_THREADS`: число в замере скорости
/// участвует напрямую, и менять его пересборкой было бы неудобно.
fn num_threads() -> i32 {
    if let Some(value) = std::env::var_os("MEETINGRAFT_GIGAAM_THREADS")
        && let Some(parsed) = value.to_str().and_then(|text| text.parse::<i32>().ok())
        && parsed > 0
    {
        return parsed;
    }
    std::thread::available_parallelism()
        .map(|count| (count.get() as i32).min(4))
        .unwrap_or(2)
}

/// Что движок услышал в куске аудио.
#[derive(Debug, Clone, Default)]
pub struct GigaamHypothesis {
    /// Текст целиком, как его собрал sherpa-onnx.
    pub text: String,
    /// Слова с временем окончания от начала поданного куска.
    ///
    /// Пусто, если модель не отдала тайм-кодов: поле `timestamps` у
    /// результата — `Option`, и притворяться, что время известно, хуже,
    /// чем сказать, что его нет.
    pub words: Vec<HypothesisWord>,
}

/// Открытый распознаватель. Держит модель в памяти; открывать на каждый
/// проход — 225 МБ чтения с диска.
pub struct GigaamRecognizer {
    recognizer: OfflineRecognizer,
}

impl GigaamRecognizer {
    /// Открыть модель из `<data_root>/models/gigaam/`.
    ///
    /// Модель качается руками (`scripts/fetch-gigaam-models.sh`), поэтому
    /// её отсутствие — ожидаемое состояние, а не сбой.
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        let models = resolve_gigaam_models(data_root).map_err(|message| {
            BatchTranscribeError::ModelMissing {
                model_id: format!("gigaam-v3: {message}"),
            }
        })?;

        // Параметры признаков не задаются: они приезжают из метаданных
        // модели, и `feat_config` на этот экспорт не влияет (проверено —
        // см. шапку модуля).
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(models.encoder.to_string_lossy().into_owned()),
            decoder: Some(models.decoder.to_string_lossy().into_owned()),
            joiner: Some(models.joiner.to_string_lossy().into_owned()),
        };
        config.model_config.tokens = Some(models.tokens.to_string_lossy().into_owned());
        // Без этого sherpa-onnx не знает, что перед ним NeMo-transducer:
        // у него другой порядок входов декодера.
        config.model_config.model_type = Some("nemo_transducer".to_string());
        config.model_config.num_threads = num_threads();

        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            BatchTranscribeError::ModelLoad(
                "sherpa-onnx не открыл модель GigaAM: проверь, что файлы докачались целиком"
                    .to_string(),
            )
        })?;
        Ok(Self { recognizer })
    }

    /// Один проход по куску аудио. PCM i16 mono.
    pub fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> GigaamHypothesis {
        if pcm.is_empty() || sample_rate == 0 {
            return GigaamHypothesis::default();
        }
        let audio: Vec<f32> = pcm.iter().map(|sample| *sample as f32 / 32768.0).collect();

        let stream: OfflineStream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, &audio);
        self.recognizer.decode(&stream);
        let Some(result) = stream.get_result() else {
            return GigaamHypothesis::default();
        };

        let words = match &result.timestamps {
            Some(times) if times.len() == result.tokens.len() => {
                let tokens: Vec<(String, u64)> = result
                    .tokens
                    .iter()
                    .zip(times.iter())
                    .map(|(token, seconds)| (token.clone(), (seconds.max(0.0) * 1000.0) as u64))
                    .collect();
                words_from_char_tokens(&tokens)
            }
            // Длины разошлись — время не наше, и приписывать словам
            // чужие тайм-коды хуже, чем не приписывать никаких.
            _ => Vec::new(),
        };

        GigaamHypothesis {
            text: result.text.trim().to_string(),
            words,
        }
    }
}

/// Пакетный проход по всей записи (ADR-011).
pub struct GigaamBatchTranscriber {
    recognizer: GigaamRecognizer,
}

impl GigaamBatchTranscriber {
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        Ok(Self {
            recognizer: GigaamRecognizer::open(data_root)?,
        })
    }
}

impl BatchTranscriber for GigaamBatchTranscriber {
    fn transcribe_all(
        &mut self,
        pcm: &[i16],
        sample_rate: u32,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<Vec<TranscriptSegment>, BatchTranscribeError> {
        if pcm.is_empty() || sample_rate == 0 {
            return Ok(Vec::new());
        }

        let window_frames = WINDOW_SECONDS * sample_rate as usize;
        let total_windows = pcm.len().div_ceil(window_frames);
        let mut segments = Vec::new();

        for (index, window) in pcm.chunks(window_frames).enumerate() {
            if !progress(index as f32 / total_windows as f32) {
                return Err(BatchTranscribeError::Cancelled);
            }

            let hypothesis = self.recognizer.transcribe(window, sample_rate);
            if hypothesis.text.is_empty() {
                continue;
            }

            // Тайм-коды слов локальны для окна — сдвигаем к началу записи.
            let window_start_ms = (index * WINDOW_SECONDS * 1000) as u64;
            let window_end_ms = window_start_ms + (window.len() as u64 * 1000) / sample_rate as u64;
            // Один сегмент на окно: делить речь на реплики движок не
            // умеет, а делать это по паузам между словами — отдельное
            // решение, и принимать его вслепую здесь не за чем. Пока
            // это значит, что привязка спикеров по сегментам (Phase 11)
            // с этим движком грубее, чем с Whisper.
            let start_ms = hypothesis
                .words
                .first()
                .map(|word| window_start_ms + word.end_ms)
                .unwrap_or(window_start_ms)
                .min(window_end_ms);
            let end_ms = hypothesis
                .words
                .last()
                .map(|word| window_start_ms + word.end_ms)
                .unwrap_or(window_end_ms)
                .min(window_end_ms);
            segments.push(TranscriptSegment::new(
                start_ms,
                end_ms.max(start_ms),
                hypothesis.text,
            ));
        }

        progress(1.0);
        Ok(normalize_segments(segments))
    }
}
