//! Пакетное распознавание для post-call прохода (Phase 10).
//!
//! Отдельный контракт от `SttEngine`, а не его расширение: у batch другой
//! режим работы. Нет потоковости и бюджета латентности, зато есть полное
//! аудио, тайм-коды и право на медленные настройки декодирования —
//! beam search вместо greedy, температурный фолбэк, контекст между
//! окнами. Смешивать это с live-движком значило бы тащить в него
//! неприменимые там компромиссы.

use domain::TranscriptSegment;

/// Почему пакетный проход не состоялся.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchTranscribeError {
    /// Модель не скачана. Качаем по требованию, поэтому это ожидаемое
    /// состояние, а не сбой: UI превращает его в предложение скачать.
    ModelMissing {
        model_id: String,
    },
    ModelLoad(String),
    Decode(String),
    /// Пользователь остановил проход.
    Cancelled,
}

impl std::fmt::Display for BatchTranscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelMissing { model_id } => write!(f, "model not downloaded: {model_id}"),
            Self::ModelLoad(message) => write!(f, "model load: {message}"),
            Self::Decode(message) => write!(f, "decode: {message}"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for BatchTranscribeError {}

/// Распознавание всего аудио сессии разом.
pub trait BatchTranscriber: Send {
    /// Прогресс от 0.0 до 1.0; возврат `false` останавливает проход.
    fn transcribe_all(
        &mut self,
        pcm: &[i16],
        sample_rate: u32,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<Vec<TranscriptSegment>, BatchTranscribeError>;
}

/// Привести сегменты в порядок: выбросить пустые, починить время.
///
/// Модель изредка отдаёт сегменты с нулевой или убывающей границей; без
/// нормализации это уехало бы в хранилище и сломало бы привязку спикеров
/// к сегментам в Phase 11.
pub fn normalize_segments(segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    let mut out: Vec<TranscriptSegment> = Vec::with_capacity(segments.len());
    let mut previous_end = 0;
    for mut segment in segments {
        segment.text = segment.text.trim().to_string();
        if segment.text.is_empty() {
            continue;
        }
        segment.start_ms = segment.start_ms.max(previous_end);
        segment.end_ms = segment.end_ms.max(segment.start_ms);
        previous_end = segment.end_ms;
        out.push(segment);
    }
    out
}

/// Тестовая реализация: режет поток по тишине, текст не распознаёт.
///
/// Нужна, чтобы весь путь post-call — чтение чанков, сборка, хранение,
/// полировка — проверялся без Whisper: он собирается только там, где есть
/// ускоритель.
pub struct MockBatchTranscriber {
    /// Порог RMS «есть речь», как у live-мока.
    threshold: f32,
    /// Минимальная тишина, закрывающая сегмент.
    silence_ms: u64,
}

impl Default for MockBatchTranscriber {
    fn default() -> Self {
        Self {
            threshold: 200.0,
            silence_ms: 300,
        }
    }
}

impl MockBatchTranscriber {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BatchTranscriber for MockBatchTranscriber {
    fn transcribe_all(
        &mut self,
        pcm: &[i16],
        sample_rate: u32,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<Vec<TranscriptSegment>, BatchTranscribeError> {
        if pcm.is_empty() || sample_rate == 0 {
            return Ok(Vec::new());
        }
        let frames_per_ms = (sample_rate as usize / 1000).max(1);
        let window = frames_per_ms * 100;
        let silence_windows = (self.silence_ms as usize / 100).max(1);

        let mut segments = Vec::new();
        let mut start: Option<usize> = None;
        let mut silence_run = 0;
        let total_windows = pcm.len().div_ceil(window);

        for (index, chunk) in pcm.chunks(window).enumerate() {
            if !progress(index as f32 / total_windows as f32) {
                return Err(BatchTranscribeError::Cancelled);
            }
            let loud = rms(chunk) >= self.threshold;
            if loud {
                silence_run = 0;
                start.get_or_insert(index * window);
            } else if let Some(from) = start {
                silence_run += 1;
                if silence_run >= silence_windows {
                    segments.push(segment(from, index * window, frames_per_ms));
                    start = None;
                    silence_run = 0;
                }
            }
        }
        if let Some(from) = start {
            segments.push(segment(from, pcm.len(), frames_per_ms));
        }
        progress(1.0);
        Ok(normalize_segments(segments))
    }
}

fn segment(from_frames: usize, to_frames: usize, frames_per_ms: usize) -> TranscriptSegment {
    let start_ms = (from_frames / frames_per_ms) as u64;
    let end_ms = (to_frames / frames_per_ms) as u64;
    TranscriptSegment::new(start_ms, end_ms, format!("[mock {start_ms}-{end_ms}]"))
}

fn rms(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    ((sum / pcm.len() as f64).sqrt()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(frames: usize) -> Vec<i16> {
        (0..frames)
            .map(|i| if i % 2 == 0 { 3000 } else { -3000 })
            .collect()
    }

    fn silence(frames: usize) -> Vec<i16> {
        vec![0; frames]
    }

    fn ignore_progress() -> impl FnMut(f32) -> bool {
        |_| true
    }

    #[test]
    fn normalize_drops_blank_segments() {
        let segments = vec![
            TranscriptSegment::new(0, 100, "   "),
            TranscriptSegment::new(100, 200, "текст"),
            TranscriptSegment::new(200, 300, ""),
        ];

        let out = normalize_segments(segments);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "текст");
    }

    /// Время не может идти назад: на нём держится привязка спикеров.
    #[test]
    fn normalize_enforces_monotonic_time() {
        let segments = vec![
            TranscriptSegment::new(0, 500, "раз"),
            TranscriptSegment::new(200, 100, "два"),
        ];

        let out = normalize_segments(segments);

        assert_eq!(out[0].end_ms, 500);
        assert_eq!(out[1].start_ms, 500);
        assert_eq!(out[1].end_ms, 500);
    }

    #[test]
    fn normalize_trims_text() {
        let out = normalize_segments(vec![TranscriptSegment::new(0, 10, "  привет  ")]);
        assert_eq!(out[0].text, "привет");
    }

    #[test]
    fn mock_splits_stream_on_silence() {
        let mut pcm = speech(16_000);
        pcm.extend(silence(16_000));
        pcm.extend(speech(16_000));
        let mut transcriber = MockBatchTranscriber::new();

        let segments = transcriber
            .transcribe_all(&pcm, 16_000, &mut ignore_progress())
            .expect("mock не должен падать");

        assert_eq!(segments.len(), 2, "{segments:?}");
        assert!(segments[0].end_ms <= segments[1].start_ms);
    }

    #[test]
    fn mock_on_empty_input_returns_no_segments() {
        let mut transcriber = MockBatchTranscriber::new();

        let segments = transcriber
            .transcribe_all(&[], 16_000, &mut ignore_progress())
            .expect("пустой вход — не ошибка");

        assert!(segments.is_empty());
    }

    #[test]
    fn progress_reaches_one_and_is_monotonic() {
        let pcm = speech(16_000 * 3);
        let mut seen: Vec<f32> = Vec::new();
        let mut transcriber = MockBatchTranscriber::new();

        transcriber
            .transcribe_all(&pcm, 16_000, &mut |value| {
                seen.push(value);
                true
            })
            .expect("mock не должен падать");

        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    /// Отмена должна прерывать проход, а не доводить его до конца.
    #[test]
    fn cancelling_progress_stops_the_pass() {
        let pcm = speech(16_000 * 10);
        let mut calls = 0;
        let mut transcriber = MockBatchTranscriber::new();

        let result = transcriber.transcribe_all(&pcm, 16_000, &mut |_| {
            calls += 1;
            calls < 3
        });

        assert_eq!(result, Err(BatchTranscribeError::Cancelled));
        assert_eq!(calls, 3, "проход не должен идти дальше отмены");
    }
}
