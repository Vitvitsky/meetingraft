//! Пакетный Whisper для post-call прохода (Phase 10, ADR-005).
//!
//! Отличия от live-движка — все из-за снятого бюджета латентности:
//! beam search вместо greedy, температурный фолбэк, контекст между
//! окнами, сегментация самой модели вместо энергетического VAD.
//!
//! Модель берётся отдельная от live: post-call не обязан довольствоваться
//! тем, что успевает в реальном времени.

use std::path::Path;

use domain::{LanguagePolicy, TranscriptSegment};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    convert_integer_to_float_audio,
};

use crate::batch::{BatchTranscribeError, BatchTranscriber, normalize_segments};
use crate::is_whisper_hallucination;
use crate::model_path::resolve_whisper_model;

/// Ширина окна Whisper: модель декодирует по 30 с.
const WINDOW_SECONDS: usize = 30;
/// Размер луча; 5 — общепринятый компромисс качества и скорости.
const BEAM_SIZE: i32 = 5;
/// Сегмент с no_speech_prob выше порога отбрасываем.
const NO_SPEECH_PROB_MAX: f32 = 0.6;

pub struct WhisperBatchTranscriber {
    ctx: WhisperContext,
    policy: LanguagePolicy,
    initial_prompt: String,
}

impl WhisperBatchTranscriber {
    /// Открыть модель под post-call. Отсутствие файла — не сбой:
    /// модель качается по требованию, UI превращает это в предложение.
    pub fn open(
        data_root: impl AsRef<Path>,
        model_id: &str,
        policy: LanguagePolicy,
    ) -> Result<Self, BatchTranscribeError> {
        let path = resolve_whisper_model(data_root.as_ref(), Some(model_id)).ok_or_else(|| {
            BatchTranscribeError::ModelMissing {
                model_id: model_id.to_owned(),
            }
        })?;
        let ctx = WhisperContext::new_with_params(
            path.to_string_lossy().as_ref(),
            WhisperContextParameters::default(),
        )
        .map_err(|error| BatchTranscribeError::ModelLoad(error.to_string()))?;
        Ok(Self {
            ctx,
            policy,
            initial_prompt: String::new(),
        })
    }

    /// Термины глоссария для смещения декодирования.
    pub fn set_initial_prompt(&mut self, prompt: &str) {
        self.initial_prompt = prompt.to_owned();
    }
}

impl BatchTranscriber for WhisperBatchTranscriber {
    fn transcribe_all(
        &mut self,
        pcm: &[i16],
        sample_rate: u32,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<Vec<TranscriptSegment>, BatchTranscribeError> {
        if pcm.is_empty() || sample_rate == 0 {
            return Ok(Vec::new());
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|error| BatchTranscribeError::ModelLoad(error.to_string()))?;

        // Идём окнами: whisper всё равно режет по 30 с, но так у нас есть
        // точка отмены и осмысленный прогресс на часовой встрече.
        let window_frames = WINDOW_SECONDS * sample_rate as usize;
        let total_windows = pcm.len().div_ceil(window_frames);
        let mut segments = Vec::new();

        for (index, window) in pcm.chunks(window_frames).enumerate() {
            if !progress(index as f32 / total_windows as f32) {
                return Err(BatchTranscribeError::Cancelled);
            }

            let mut params = FullParams::new(SamplingStrategy::BeamSearch {
                beam_size: BEAM_SIZE,
                patience: 0.0,
            });
            params.set_language(Some(self.policy.primary.code()));
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            // Полное аудио: сегментирует сама модель, не энергетический VAD.
            params.set_single_segment(false);
            params.set_token_timestamps(true);
            // В отличие от live, контекст здесь безопасен: буфер длинный,
            // пересказывать промпт модели незачем (ADR-010, раздел «Откат»).
            params.set_no_context(false);
            params.set_suppress_blank(true);
            if !self.initial_prompt.is_empty() {
                params.set_initial_prompt(&self.initial_prompt);
            }

            let mut audio = vec![0.0f32; window.len()];
            convert_integer_to_float_audio(window, &mut audio)
                .map_err(|error| BatchTranscribeError::Decode(error.to_string()))?;
            state
                .full(params, &audio)
                .map_err(|error| BatchTranscribeError::Decode(error.to_string()))?;

            // Тайм-коды окна локальны — сдвигаем к началу записи.
            let offset_ms = (index * WINDOW_SECONDS * 1000) as u64;
            for segment_index in 0..state.full_n_segments() {
                let Some(segment) = state.get_segment(segment_index) else {
                    continue;
                };
                if segment.no_speech_probability() > NO_SPEECH_PROB_MAX {
                    continue;
                }
                let Ok(text) = segment.to_str() else {
                    continue;
                };
                let text = text.trim();
                if text.is_empty() || is_whisper_hallucination(text) {
                    continue;
                }
                // Тайм-коды whisper — в сотых долях секунды.
                segments.push(TranscriptSegment::new(
                    offset_ms + (segment.start_timestamp().max(0) as u64) * 10,
                    offset_ms + (segment.end_timestamp().max(0) as u64) * 10,
                    text,
                ));
            }
        }

        progress(1.0);
        Ok(normalize_segments(segments))
    }
}
