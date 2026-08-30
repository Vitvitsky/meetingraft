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
//! ## Что означают тайм-коды. Тоже замер, а не догадка
//!
//! `timestamps` — время **выдачи каждого символа**, с шагом 40 мс:
//! на контрольной записи «ничьих» это `0.04 0.08 0.16 0.24 0.32 0.36`, а
//! пробел после него — `0.44`. То есть время слова лежит между первым и
//! последним его символом, и границы куска речи берутся с **первого**
//! символа, а не с конца первого слова.
//!
//! `durations` этот экспорт отдаёт **пустым списком** — не `None`, а
//! `Some([])`. Длительность символа отсюда не берётся ни в каком виде.
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
use crate::gigaam_path::{MODEL_ID, resolve_gigaam_models};
use crate::hypothesis::TransducerHypothesis;
use crate::local_agreement::words_from_char_tokens;

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
    /// Подробностей о недостающих файлах здесь нет намеренно: `model_id`
    /// — идентификатор, и `ffi::rebuild` показывает его человеку как
    /// «модель {model_id} не скачана». Целая жалоба резолвера внутри
    /// этой подстановки дала бы фразу, которую невозможно прочесть. Кому
    /// нужны подробности — зовёт [`resolve_gigaam_models`] сам; так и
    /// делает `stt-probe`.
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        Self::open_with(data_root, None)
    }

    /// То же, но с контекстным смещением под глоссарий.
    ///
    /// Токены у GigaAM посимвольные, поэтому `modeling_unit` — `cjkchar`:
    /// так sherpa режет фразу термина на буквы, а не ищет её в словаре
    /// целиком (целиком её там нет). Имя единицы «китайский символ»
    /// здесь сбивает с толку и означает ровно «по одному символу».
    pub fn open_with(
        data_root: impl AsRef<Path>,
        biasing: Option<&crate::hypothesis::Biasing>,
    ) -> Result<Self, BatchTranscribeError> {
        let models =
            resolve_gigaam_models(data_root).map_err(|_| BatchTranscribeError::ModelMissing {
                model_id: MODEL_ID.to_string(),
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

        if let Some(biasing) = biasing {
            // Без этого смещение принимается и не делает **ничего**:
            // жадный поиск лучей не держит.
            config.decoding_method = Some("modified_beam_search".to_string());
            config.hotwords_file = Some(biasing.hotwords.to_string_lossy().into_owned());
            config.hotwords_score = biasing.score;
            config.model_config.modeling_unit = Some("cjkchar".to_string());
        }

        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            BatchTranscribeError::ModelLoad(
                "sherpa-onnx не открыл модель GigaAM: проверь, что файлы докачались целиком"
                    .to_string(),
            )
        })?;
        Ok(Self { recognizer })
    }

    /// Один проход по куску аудио. PCM i16 mono.
    ///
    /// Пустой вход — законная пустая гипотеза. А вот **отказ движка —
    /// ошибка, а не тишина**: `get_result` отдаёт `None` и когда C-вызов
    /// вернул null, и когда разбор его JSON не удался (схема результата
    /// у sherpa между версиями менялась). Отдать на это пустой текст
    /// значило бы превратить сломанный проход в успешную пустую
    /// расшифровку — тот самый молчаливый отказ, который в этом проекте
    /// считается худшим исходом.
    pub fn transcribe(
        &self,
        pcm: &[i16],
        sample_rate: u32,
    ) -> Result<TransducerHypothesis, BatchTranscribeError> {
        if pcm.is_empty() || sample_rate == 0 {
            return Ok(TransducerHypothesis::default());
        }
        let audio: Vec<f32> = pcm.iter().map(|sample| *sample as f32 / 32768.0).collect();

        let stream: OfflineStream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, &audio);
        self.recognizer.decode(&stream);
        let result = stream.get_result().ok_or_else(|| {
            BatchTranscribeError::Decode(
                "sherpa-onnx не отдал результат распознавания: null или неразобранный JSON"
                    .to_string(),
            )
        })?;

        let times: Option<Vec<u64>> = result
            .timestamps
            .as_ref()
            // Длины разошлись — время не наше, и приписывать словам
            // чужие тайм-коды хуже, чем не приписывать никаких.
            .filter(|times| times.len() == result.tokens.len())
            .map(|times| {
                times
                    .iter()
                    .map(|seconds| (seconds.max(0.0) * 1000.0) as u64)
                    .collect()
            });

        let (words, speech_ms) = match times {
            Some(times) => {
                let tokens: Vec<(String, u64)> = result
                    .tokens
                    .iter()
                    .cloned()
                    .zip(times.iter().copied())
                    .collect();
                // Границы речи — по первому и последнему символу, а не по
                // словам: у слова здесь известен только конец.
                let bounds = match (times.first(), times.last()) {
                    (Some(first), Some(last)) => Some((*first, *last.max(first))),
                    _ => None,
                };
                (words_from_char_tokens(&tokens), bounds)
            }
            None => (Vec::new(), None),
        };

        Ok(TransducerHypothesis {
            text: result.text.trim().to_string(),
            words,
            speech_ms,
        })
    }
}

/// Пакетный проход по всей записи (ADR-011).
pub struct GigaamBatchTranscriber {
    recognizer: GigaamRecognizer,
}

impl GigaamBatchTranscriber {
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        Self::open_with(data_root, None)
    }

    /// То же, но с контекстным смещением под глоссарий.
    ///
    /// Токены у GigaAM посимвольные, поэтому `modeling_unit` — `cjkchar`:
    /// так sherpa режет фразу термина на буквы, а не ищет её в словаре
    /// целиком (целиком её там нет). Имя единицы «китайский символ»
    /// здесь сбивает с толку и означает ровно «по одному символу».
    pub fn open_with(
        data_root: impl AsRef<Path>,
        biasing: Option<&crate::hypothesis::Biasing>,
    ) -> Result<Self, BatchTranscribeError> {
        Ok(Self {
            recognizer: GigaamRecognizer::open_with(data_root, biasing)?,
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

            let hypothesis = self.recognizer.transcribe(window, sample_rate)?;
            if hypothesis.text.is_empty() {
                continue;
            }

            // Тайм-коды локальны для окна — сдвигаем к началу записи.
            let window_start_ms = (index * WINDOW_SECONDS * 1000) as u64;
            let window_end_ms = window_start_ms + (window.len() as u64 * 1000) / sample_rate as u64;
            // Один сегмент на окно: делить речь на реплики движок не
            // умеет, а делать это по паузам между словами — отдельное
            // решение, и принимать его вслепую здесь не за чем. Пока
            // это значит, что привязка спикеров по сегментам (Phase 11)
            // с этим движком грубее, чем с Whisper.
            let (start_ms, end_ms) =
                segment_bounds(hypothesis.speech_ms, window_start_ms, window_end_ms);
            segments.push(TranscriptSegment::new(start_ms, end_ms, hypothesis.text));
        }

        progress(1.0);
        Ok(normalize_segments(segments))
    }
}

/// Границы сегмента из границ речи внутри окна.
///
/// Отдельной функцией, потому что это единственное место здесь, которое
/// можно проверить тестом без модели, — а ошибиться в нём легче всего.
/// Первая версия брала началом конец первого слова, и на окне с одним
/// словом получался сегмент нулевой длины: `normalize_segments` такой
/// пропускает, а привязка спикеров по перекрытию времени не находит на
/// нём ничего.
fn segment_bounds(
    speech_ms: Option<(u64, u64)>,
    window_start_ms: u64,
    window_end_ms: u64,
) -> (u64, u64) {
    let Some((first, last)) = speech_ms else {
        // Времени нет — сегмент занимает всё окно. Это грубо, но честно:
        // речь в окне была, а где именно, движок не сказал.
        return (window_start_ms, window_end_ms);
    };
    let start = (window_start_ms + first).min(window_end_ms);
    let end = (window_start_ms + last).clamp(start, window_end_ms);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Одно слово в окне: сегмент обязан иметь длину, а не схлопнуться.
    #[test]
    fn a_single_word_window_is_not_zero_length() {
        // «да»: первый символ выдан на 640 мс, последний на 700.
        let (start, end) = segment_bounds(Some((640, 700)), 30_000, 60_000);
        assert_eq!(start, 30_640);
        assert!(end > start, "сегмент схлопнулся: {start}..{end}");
    }

    /// Начало сегмента — начало первого слова, а не его конец.
    #[test]
    fn the_segment_starts_where_the_first_character_was_emitted() {
        let (start, _) = segment_bounds(Some((40, 10_880)), 0, 30_000);
        assert_eq!(start, 40);
    }

    /// Без тайм-кодов сегмент занимает окно целиком — и это видно.
    #[test]
    fn without_timestamps_the_segment_covers_the_window() {
        assert_eq!(segment_bounds(None, 30_000, 60_000), (30_000, 60_000));
    }

    /// Тайм-код за пределами окна не выносит сегмент за его границу.
    #[test]
    fn bounds_never_leave_the_window() {
        let (start, end) = segment_bounds(Some((45_000, 90_000)), 30_000, 60_000);
        assert!(start <= end, "{start}..{end}");
        assert!(end <= 60_000, "сегмент вышел за окно: {start}..{end}");
    }
}
