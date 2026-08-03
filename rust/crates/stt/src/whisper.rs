//! On-device Whisper (whisper-rs + Metal).

use domain::{CaptionEvent, CaptionPhase, LanguagePolicy};
use uuid::Uuid;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    convert_integer_to_float_audio,
};

use crate::is_whisper_hallucination;
use crate::local_agreement::{HypothesisWord, LocalAgreement, words_from_tokens};
use crate::{Stabilized, SttEngine};

/// Порог RMS для «есть речь» (выше → меньше галлюцинаций на тишине).
const ENERGY_THRESHOLD: f32 = 450.0;
const SILENCE_FRAMES: usize = 16_000 * 3 / 10;
const MIN_SPEECH_FRAMES: usize = 16_000 / 5;
/// Не гоняем Whisper чаще чем раз в ~1 с на partial.
const PARTIAL_MIN_FRAMES: usize = 16_000;
/// Сегмент с no_speech_prob выше порога отбрасываем.
const NO_SPEECH_PROB_MAX: f32 = 0.55;
/// Потолок неустойчивого хвоста: без него согласие может не наступить
/// на шумной речи и хвост будет расти бесконечно.
const MAX_PENDING_WORDS: usize = 24;
/// Потолок буфера — окно Whisper. Дальше режем принудительно.
const MAX_BUFFER_FRAMES: usize = 16_000 * 30;
/// Режем с запасом назад: тайм-коды на границе неточны, потерять контекст
/// дешевле, чем обрезать слово посередине.
const TRIM_GUARD_MS: u64 = 200;

/// Whisper STT с energy-VAD сегментацией.
pub struct WhisperSttEngine {
    ctx: WhisperContext,
    policy: LanguagePolicy,
    buffer: Vec<i16>,
    speech_frames: usize,
    silence_frames: usize,
    in_speech: bool,
    frames_since_partial: usize,
    initial_prompt: String,
    agreement: LocalAgreement,
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
            initial_prompt: String::new(),
            agreement: LocalAgreement::new(MAX_PENDING_WORDS),
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
        CaptionEvent::new(Uuid::new_v4().to_string(), text.to_string(), phase)
    }

    fn accept_text(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() || is_whisper_hallucination(trimmed) {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Гипотеза по буферу: слова с временем окончания.
    fn hypothesis(&self, pcm: &[i16]) -> Vec<HypothesisWord> {
        if pcm.len() < MIN_SPEECH_FRAMES / 2 {
            return Vec::new();
        }
        let Ok(mut state) = self.ctx.create_state() else {
            return Vec::new();
        };
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(self.policy.primary.code()));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Тайм-коды нужны для обрезки буфера, контекст — для связности
        // между итерациями (LocalAgreement, ADR-010).
        params.set_single_segment(false);
        params.set_no_context(false);
        params.set_token_timestamps(true);
        params.set_suppress_blank(true);
        params.set_temperature(0.0);
        // Документация whisper-rs: no_speech_thold historically stub — всё равно
        // фильтруем по segment.no_speech_probability() ниже.
        params.set_no_speech_thold(0.6);
        let prompt = self.decoding_prompt();
        if !prompt.is_empty() {
            params.set_initial_prompt(&prompt);
        }

        let mut audio = vec![0.0f32; pcm.len()];
        if convert_integer_to_float_audio(pcm, &mut audio).is_err() {
            return Vec::new();
        }
        if state.full(params, &audio).is_err() {
            return Vec::new();
        }

        let mut tokens: Vec<(String, u64)> = Vec::new();
        for index in 0..state.full_n_segments() {
            let Some(segment) = state.get_segment(index) else {
                continue;
            };
            if segment.no_speech_probability() > NO_SPEECH_PROB_MAX {
                continue;
            }
            if segment
                .to_str()
                .is_ok_and(|text| is_whisper_hallucination(text.trim()))
            {
                continue;
            }
            for token_index in 0..segment.n_tokens() {
                let Ok(text) = segment.get_token_text(token_index) else {
                    continue;
                };
                let Some(data) = segment.get_token_data(token_index) else {
                    continue;
                };
                // t1 в сотых долях секунды от начала буфера.
                tokens.push((text, (data.t1.max(0) as u64) * 10));
            }
        }
        words_from_tokens(&tokens)
    }

    /// Глоссарий плюс хвост зафиксированного текста.
    ///
    /// Порядок важен: термины должны пережить обрезку по длине промпта,
    /// поэтому контекст добавляется после них.
    fn decoding_prompt(&self) -> String {
        let tail = self.agreement.committed_tail();
        match (self.initial_prompt.is_empty(), tail.is_empty()) {
            (true, true) => String::new(),
            (true, false) => tail,
            (false, true) => self.initial_prompt.clone(),
            (false, false) => format!("{} {tail}", self.initial_prompt),
        }
    }

    /// Выбросить из буфера аудио до зафиксированной границы.
    fn trim_buffer(&mut self, until_ms: u64) {
        let cut_ms = until_ms.saturating_sub(TRIM_GUARD_MS);
        let frames = (cut_ms as usize) * 16;
        if frames == 0 || frames >= self.buffer.len() {
            return;
        }
        self.buffer.drain(0..frames);
        self.agreement.rebase(cut_ms);
    }

    /// События из результата стабилизации.
    fn events(stabilized: &Stabilized) -> Vec<CaptionEvent> {
        let mut out = Vec::new();
        if let Some(text) = Self::accept_text(&stabilized.committed_text) {
            out.push(Self::event(text, CaptionPhase::Final));
        }
        if let Some(text) = Self::accept_text(&stabilized.pending_text) {
            out.push(Self::event(text, CaptionPhase::Partial));
        }
        out
    }

    fn reset_segment(&mut self) {
        self.buffer.clear();
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.frames_since_partial = 0;
        self.agreement.reset();
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

            if self.buffer.len() > MAX_BUFFER_FRAMES {
                // Согласия долго нет: режем принудительно, иначе инференс
                // по растущему буферу съедает бюджет латентности.
                let overflow_ms = ((self.buffer.len() - MAX_BUFFER_FRAMES) / 16) as u64;
                self.trim_buffer(overflow_ms + TRIM_GUARD_MS);
            }

            if self.speech_frames >= MIN_SPEECH_FRAMES
                && self.frames_since_partial >= PARTIAL_MIN_FRAMES
            {
                let hypothesis = self.hypothesis(&self.buffer);
                let stabilized = self.agreement.push(hypothesis);
                out.extend(Self::events(&stabilized));
                if let Some(until_ms) = stabilized.committed_until_ms {
                    self.trim_buffer(until_ms);
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
        // Последняя гипотеза плюс принудительная фиксация остатка: контекста
        // больше не будет, ждать согласия не с чем.
        let hypothesis = self.hypothesis(&self.buffer);
        let mut out = Vec::new();
        let stabilized = self.agreement.push(hypothesis);
        if let Some(text) = Self::accept_text(&stabilized.committed_text) {
            out.push(Self::event(text, CaptionPhase::Final));
        }
        let tail = self.agreement.flush();
        if let Some(text) = Self::accept_text(&tail.committed_text) {
            out.push(Self::event(text, CaptionPhase::Final));
        }
        self.reset_segment();
        out
    }
}
