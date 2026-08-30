//! Многоязычный распознаватель parakeet-tdt-0.6b-v3 через sherpa-onnx.
//!
//! FastConformer-TDT от NVIDIA под CC-BY-4.0, экспорт int8. Двадцать пять
//! языков, среди них все три из ADR-003 — русский, английский,
//! испанский, — с автоопределением языка, тайм-кодами слов и
//! пунктуацией.
//!
//! **Зачем третий движок.** GigaAM понимает только русский, Whisper
//! собирается только на Маке. Этот может закрыть ADR-003 один — но
//! «может» здесь ровно до чисел: стенд для того и написан
//! (`docs/superpowers/plans/2026-08-28-asr-bench.md`, задача 5).
//!
//! ## Тот же путь, что у GigaAM, и это факт, а не удобство
//!
//! Пример в документации крейта `sherpa-onnx` написан **буквально** на
//! этой модели с `model_type = "nemo_transducer"`. То есть подключение
//! отличается от соседа только именами файлов и разбором токенов.
//!
//! ## Токены: догадка была неверной, и её снял прогон
//!
//! `tokens.txt` здесь — BPE sentencepiece на 8193 позиции, и начало слова
//! в **файле словаря** помечено символом `▁` U+2581, а не пробелом.
//! Отсюда был написан третий сборщик слов, под этот маркер.
//!
//! **Прогон его не подтвердил.** sherpa-onnx отдаёт токены уже
//! нормализованными: `[" Н", "и", "ч", "ь", "и", "х", ",", " не", ...]` —
//! ведущий пробел, ровно как у Whisper. Третий сборщик увидел ноль
//! маркеров, склеил всё в одно слово, и `check_word_times` в `stt-probe`
//! это поймал: «слов с временем 1, а в тексте 26». Сборщик убран —
//! функция под опровергнутую догадку хуже отсутствующей.
//!
//! Поэтому здесь [`words_from_tokens`], тот же, что у Whisper. Ошибиться
//! в этом месте легко и **молча**: текст остаётся верным, WER не
//! шелохнётся, расходится только число слов с тайм-кодами — то есть
//! сборка сегментов. Ловится прибором, а не глазом.

use std::path::Path;

use domain::TranscriptSegment;
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineStream, OfflineTransducerModelConfig,
};

use crate::batch::{BatchTranscribeError, BatchTranscriber, normalize_segments};
use crate::hypothesis::TransducerHypothesis;
use crate::local_agreement::words_from_tokens;
use crate::parakeet_path::{PARAKEET_MODEL_ID, resolve_parakeet_models};

/// Ширина окна пакетного прохода.
///
/// Те же тридцать секунд, что у `gigaam` и `whisper_batch`, и по той же
/// причине: сравнивать движки проще, когда режут они одинаково. Своё
/// число этому параметру назначает стенд, а не догадка.
const WINDOW_SECONDS: usize = 30;

/// Сколько потоков отдать onnxruntime.
///
/// Переопределяется `MEETINGRAFT_PARAKEET_THREADS`: число участвует в
/// замере скорости напрямую, и менять его пересборкой неудобно.
fn num_threads() -> i32 {
    if let Some(value) = std::env::var_os("MEETINGRAFT_PARAKEET_THREADS")
        && let Some(parsed) = value.to_str().and_then(|text| text.parse::<i32>().ok())
        && parsed > 0
    {
        return parsed;
    }
    std::thread::available_parallelism()
        .map(|count| (count.get() as i32).min(4))
        .unwrap_or(2)
}

/// Открытый распознаватель. Держит модель в памяти: энкодер — 622 МБ, и
/// открывать его на каждый кусок значило бы читать их с диска заново.
pub struct ParakeetRecognizer {
    recognizer: OfflineRecognizer,
}

impl ParakeetRecognizer {
    /// Открыть модель из `<data_root>/models/parakeet/`.
    ///
    /// Модель качается руками, поэтому её отсутствие — ожидаемое
    /// состояние, а не сбой.
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        Self::open_with(data_root, None)
    }

    /// То же, но с контекстным смещением под глоссарий.
    ///
    /// Здесь словарь BPE, и `modeling_unit` — `bpe`. Это требует **файла
    /// словаря BPE** рядом с моделью; в экспорте, который качает наш
    /// скрипт, его нет (только `tokens.txt`). Работает ли смещение без
    /// него — вопрос замера, а не документации.
    pub fn open_with(
        data_root: impl AsRef<Path>,
        biasing: Option<&crate::hypothesis::Biasing>,
    ) -> Result<Self, BatchTranscribeError> {
        let models =
            resolve_parakeet_models(data_root).map_err(|_| BatchTranscribeError::ModelMissing {
                model_id: PARAKEET_MODEL_ID.to_string(),
            })?;

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
            // Словарь BPE — не украшение: без него sherpa отказывается
            // открывать распознаватель вовсе. Экспорт, который качает наш
            // скрипт, его не кладёт (только `tokens.txt`), и это
            // проверено прогоном.
            //
            // Отказ здесь **называет причину**. Первая версия пропускала
            // пустой путь дальше, sherpa падал на проверке конфига, а
            // наше сообщение говорило «проверь, что файлы докачались
            // целиком» — то есть винило закачку в том, чего она не
            // делала.
            let vocab = models.tokens.with_file_name("bpe.vocab");
            if !vocab.exists() {
                return Err(BatchTranscribeError::ModelLoad(format!(
                    "смещение под глоссарий требует словаря BPE, а {} нет: \
                     экспорт {PARAKEET_MODEL_ID} его не кладёт. \
                     Без глоссария движок работает",
                    vocab.display()
                )));
            }
            config.decoding_method = Some("modified_beam_search".to_string());
            config.hotwords_file = Some(biasing.hotwords.to_string_lossy().into_owned());
            config.hotwords_score = biasing.score;
            config.model_config.modeling_unit = Some("bpe".to_string());
            config.model_config.bpe_vocab = Some(vocab.to_string_lossy().into_owned());
        }

        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            BatchTranscribeError::ModelLoad(
                "sherpa-onnx не открыл модель parakeet: проверь, что файлы докачались целиком"
                    .to_string(),
            )
        })?;
        Ok(Self { recognizer })
    }

    /// Один проход по куску аудио. PCM i16 mono.
    ///
    /// Пустой вход — законная пустая гипотеза. Отказ движка — **ошибка**,
    /// а не тишина: `get_result` отдаёт `None` и когда C-вызов вернул
    /// null, и когда разбор его JSON не удался. Отдать на это пустой
    /// текст значило бы превратить сломанный проход в успешную пустую
    /// расшифровку.
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
            // Длины разошлись — время не наше, и приписывать словам чужие
            // тайм-коды хуже, чем не приписывать никаких.
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
                let bounds = match (times.first(), times.last()) {
                    (Some(first), Some(last)) => Some((*first, *last.max(first))),
                    _ => None,
                };
                (words_from_tokens(&tokens), bounds)
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
pub struct ParakeetBatchTranscriber {
    recognizer: ParakeetRecognizer,
}

impl ParakeetBatchTranscriber {
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        Self::open_with(data_root, None)
    }

    /// То же, но с контекстным смещением под глоссарий.
    ///
    /// Здесь словарь BPE, и `modeling_unit` — `bpe`. Это требует **файла
    /// словаря BPE** рядом с моделью; в экспорте, который качает наш
    /// скрипт, его нет (только `tokens.txt`). Работает ли смещение без
    /// него — вопрос замера, а не документации.
    pub fn open_with(
        data_root: impl AsRef<Path>,
        biasing: Option<&crate::hypothesis::Biasing>,
    ) -> Result<Self, BatchTranscribeError> {
        Ok(Self {
            recognizer: ParakeetRecognizer::open_with(data_root, biasing)?,
        })
    }
}

impl BatchTranscriber for ParakeetBatchTranscriber {
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

            let window_start_ms = (index * WINDOW_SECONDS * 1000) as u64;
            let window_end_ms = window_start_ms + (window.len() as u64 * 1000) / sample_rate as u64;
            let (start_ms, end_ms) = match hypothesis.speech_ms {
                Some((first, last)) => {
                    let start = (window_start_ms + first).min(window_end_ms);
                    (start, (window_start_ms + last).clamp(start, window_end_ms))
                }
                None => (window_start_ms, window_end_ms),
            };
            segments.push(TranscriptSegment::new(start_ms, end_ms, hypothesis.text));
        }

        progress(1.0);
        Ok(normalize_segments(segments))
    }
}
