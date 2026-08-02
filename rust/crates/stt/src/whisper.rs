//! On-device Whisper (whisper-rs + Metal).

use domain::{CaptionEvent, CaptionPhase, LanguagePolicy};
use uuid::Uuid;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    convert_integer_to_float_audio,
};

use crate::SttEngine;
use crate::is_whisper_hallucination;

/// Порог RMS для «есть речь» (выше → меньше галлюцинаций на тишине).
const ENERGY_THRESHOLD: f32 = 450.0;
const SILENCE_FRAMES: usize = 16_000 * 3 / 10;
const MIN_SPEECH_FRAMES: usize = 16_000 / 5;
/// Не гоняем Whisper чаще чем раз в ~1 с на partial.
const PARTIAL_MIN_FRAMES: usize = 16_000;
/// Сегмент с no_speech_prob выше порога отбрасываем.
const NO_SPEECH_PROB_MAX: f32 = 0.55;

/// Whisper STT с energy-VAD сегментацией.
pub struct WhisperSttEngine {
    ctx: WhisperContext,
    policy: LanguagePolicy,
    buffer: Vec<i16>,
    speech_frames: usize,
    silence_frames: usize,
    in_speech: bool,
    frames_since_partial: usize,
    last_partial_text: String,
    initial_prompt: String,
}

impl WhisperSttEngine {
    pub fn open(model_path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let path = model_path.as_ref();
        let ctx = WhisperContext::new_with_params(
            path.to_string_lossy().as_ref(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("whisper load: {e}"))?;
        Ok(Self {
            ctx,
            policy: LanguagePolicy::default_v1(),
            buffer: Vec::new(),
            speech_frames: 0,
            silence_frames: 0,
            in_speech: false,
            frames_since_partial: 0,
            last_partial_text: String::new(),
            initial_prompt: String::new(),
        })
    }

    fn rms(pcm: &[i16]) -> f32 {
        if pcm.is_empty() {
            return 0.0;
        }
        let sum: f64 = pcm.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        ((sum / pcm.len() as f64).sqrt()) as f32
    }

    fn event(text: String, phase: CaptionPhase) -> CaptionEvent {
        CaptionEvent {
            id: Uuid::new_v4().to_string(),
            text,
            phase,
        }
    }

    fn accept_text(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() || is_whisper_hallucination(trimmed) {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn transcribe(&self, pcm: &[i16]) -> Option<String> {
        if pcm.len() < MIN_SPEECH_FRAMES / 2 {
            return None;
        }
        let mut state = self.ctx.create_state().ok()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(self.policy.primary.code()));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_single_segment(true);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_temperature(0.0);
        // Документация whisper-rs: no_speech_thold historically stub — всё равно
        // фильтруем по segment.no_speech_probability() ниже.
        params.set_no_speech_thold(0.6);
        if !self.initial_prompt.is_empty() {
            params.set_initial_prompt(&self.initial_prompt);
        }

        let mut audio = vec![0.0f32; pcm.len()];
        convert_integer_to_float_audio(pcm, &mut audio).ok()?;
        state.full(params, &audio).ok()?;
        let n = state.full_n_segments();
        let mut parts = Vec::new();
        for i in 0..n {
            if let Some(seg) = state.get_segment(i) {
                if seg.no_speech_probability() > NO_SPEECH_PROB_MAX {
                    continue;
                }
                if let Ok(t) = seg.to_str() {
                    let trimmed = t.trim();
                    if !trimmed.is_empty() && !is_whisper_hallucination(trimmed) {
                        parts.push(trimmed.to_string());
                    }
                }
            }
        }
        let text = parts.join(" ");
        Self::accept_text(&text)
    }

    fn reset_segment(&mut self) {
        self.buffer.clear();
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.frames_since_partial = 0;
        self.last_partial_text.clear();
    }
}

impl SttEngine for WhisperSttEngine {
    fn set_language_policy(&mut self, policy: LanguagePolicy) {
        self.policy = policy;
    }

    fn set_initial_prompt(&mut self, prompt: &str) {
        self.initial_prompt = prompt.to_owned();
    }

    fn push_pcm(&mut self, pcm: &[i16], _sample_rate: u32) -> Vec<CaptionEvent> {
        let mut out = Vec::new();
        let energy = Self::rms(pcm);
        if energy >= ENERGY_THRESHOLD {
            self.in_speech = true;
            self.silence_frames = 0;
            self.speech_frames += pcm.len();
            self.frames_since_partial += pcm.len();
            self.buffer.extend_from_slice(pcm);

            if self.speech_frames >= MIN_SPEECH_FRAMES
                && self.frames_since_partial >= PARTIAL_MIN_FRAMES
            {
                if let Some(text) = self.transcribe(&self.buffer)
                    && text != self.last_partial_text
                {
                    self.last_partial_text = text.clone();
                    out.push(Self::event(text, CaptionPhase::Partial));
                }
                self.frames_since_partial = 0;
            }
        } else if self.in_speech {
            self.silence_frames += pcm.len();
            self.buffer.extend_from_slice(pcm);
            if self.silence_frames >= SILENCE_FRAMES {
                out.extend(self.flush());
            }
        }
        out
    }

    fn flush(&mut self) -> Vec<CaptionEvent> {
        if self.buffer.is_empty() && !self.in_speech {
            return Vec::new();
        }
        let text = self
            .transcribe(&self.buffer)
            .or_else(|| Self::accept_text(&self.last_partial_text));
        self.reset_segment();
        match text {
            Some(t) => vec![Self::event(t, CaptionPhase::Final)],
            None => Vec::new(),
        }
    }
}
