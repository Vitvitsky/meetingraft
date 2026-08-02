//! Mock STT: energy VAD → русские partial/final (для CI и без модели).

use domain::{CaptionEvent, CaptionPhase, LanguagePolicy};
use uuid::Uuid;

use crate::SttEngine;

/// Порог RMS для «есть речь».
const ENERGY_THRESHOLD: f32 = 200.0;
/// Тишина ~300 ms @ 16 kHz → finalize.
const SILENCE_FRAMES: usize = 16_000 * 3 / 10;
/// Минимум речи перед partial.
const MIN_SPEECH_FRAMES: usize = 16_000 / 5;

/// Тестовый/fallback движок без Whisper.
pub struct MockSttEngine {
    policy: LanguagePolicy,
    speech_frames: usize,
    silence_frames: usize,
    in_speech: bool,
    partial_emitted: bool,
}

impl Default for MockSttEngine {
    fn default() -> Self {
        Self {
            policy: LanguagePolicy::default_v1(),
            speech_frames: 0,
            silence_frames: 0,
            in_speech: false,
            partial_emitted: false,
        }
    }
}

impl MockSttEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn rms(pcm: &[i16]) -> f32 {
        if pcm.is_empty() {
            return 0.0;
        }
        let sum: f64 = pcm.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        ((sum / pcm.len() as f64).sqrt()) as f32
    }

    fn event(text: &str, phase: CaptionPhase) -> CaptionEvent {
        CaptionEvent {
            id: Uuid::new_v4().to_string(),
            text: text.to_string(),
            phase,
        }
    }
}

impl SttEngine for MockSttEngine {
    fn set_language_policy(&mut self, policy: LanguagePolicy) {
        self.policy = policy;
    }

    fn push_pcm(&mut self, pcm: &[i16], _sample_rate: u32) -> Vec<CaptionEvent> {
        let mut out = Vec::new();
        let energy = Self::rms(pcm);
        if energy >= ENERGY_THRESHOLD {
            self.in_speech = true;
            self.silence_frames = 0;
            self.speech_frames += pcm.len();
            if !self.partial_emitted && self.speech_frames >= MIN_SPEECH_FRAMES {
                let lang = self.policy.primary.code();
                out.push(Self::event(
                    &format!("[partial {lang}] речь…"),
                    CaptionPhase::Partial,
                ));
                self.partial_emitted = true;
            }
        } else if self.in_speech {
            self.silence_frames += pcm.len();
            if self.silence_frames >= SILENCE_FRAMES {
                out.extend(self.flush());
            }
        }
        out
    }

    fn flush(&mut self) -> Vec<CaptionEvent> {
        if !self.in_speech && !self.partial_emitted {
            return Vec::new();
        }
        let lang = self.policy.primary.code();
        let text = format!("[final {lang}] фрагмент речи");
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.partial_emitted = false;
        vec![Self::event(&text, CaptionPhase::Final)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loud(n: usize) -> Vec<i16> {
        vec![3000; n]
    }

    fn quiet(n: usize) -> Vec<i16> {
        vec![0; n]
    }

    #[test]
    fn loud_then_silence_emits_partial_and_final() {
        let mut engine = MockSttEngine::new();
        let mut events = engine.push_pcm(&loud(MIN_SPEECH_FRAMES), 16_000);
        assert!(events.iter().any(|e| e.phase == CaptionPhase::Partial));
        events = engine.push_pcm(&quiet(SILENCE_FRAMES), 16_000);
        assert!(events.iter().any(|e| e.phase == CaptionPhase::Final));
        assert!(events.iter().any(|e| e.text.contains("ru")));
    }
}
