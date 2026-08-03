//! Обёртка: PCM bytes → i16 → SttEngine; выбор Mock / Whisper.

use std::path::Path;

use domain::{CaptionEvent, LanguagePolicy};

use crate::{MockSttEngine, SttEngine, resolve_whisper_model};

#[cfg(feature = "whisper")]
use crate::WhisperSttEngine;

/// Какой движок активен в pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttBackendKind {
    Mock,
    Whisper,
}

/// Live pipeline над любым `SttEngine`.
pub struct LiveCaptionPipeline {
    engine: Box<dyn SttEngine>,
    backend: SttBackendKind,
}

impl LiveCaptionPipeline {
    pub fn mock(policy: LanguagePolicy) -> Self {
        let mut engine = MockSttEngine::new();
        engine.set_language_policy(policy);
        Self {
            engine: Box::new(engine),
            backend: SttBackendKind::Mock,
        }
    }

    /// Whisper, если feature + файл модели; иначе Mock (+ stderr warning).
    pub fn from_data_root(
        data_root: impl AsRef<Path>,
        policy: LanguagePolicy,
        preferred: Option<&str>,
    ) -> Self {
        match try_whisper(data_root.as_ref(), policy.clone(), preferred) {
            Some(pipeline) => pipeline,
            None => {
                eprintln!(
                    "meetingraft-stt: Whisper model not found under {:?}/models — using MockSttEngine",
                    data_root.as_ref()
                );
                Self::mock(policy)
            }
        }
    }

    pub fn backend(&self) -> SttBackendKind {
        self.backend
    }

    pub fn set_initial_prompt(&mut self, prompt: &str) {
        self.engine.set_initial_prompt(prompt);
    }

    pub fn set_language_policy(&mut self, policy: LanguagePolicy) {
        self.engine.set_language_policy(policy);
    }

    pub fn push_pcm_bytes(&mut self, pcm: &[u8], sample_rate: u32) -> Vec<CaptionEvent> {
        let samples = pcm_bytes_to_i16(pcm);
        self.engine.push_pcm(&samples, sample_rate)
    }

    pub fn flush(&mut self) -> Vec<CaptionEvent> {
        self.engine.flush()
    }
}

fn try_whisper(
    data_root: &Path,
    policy: LanguagePolicy,
    preferred: Option<&str>,
) -> Option<LiveCaptionPipeline> {
    let model = resolve_whisper_model(data_root, preferred)?;
    #[cfg(feature = "whisper")]
    {
        match WhisperSttEngine::open(&model) {
            Ok(mut engine) => {
                engine.set_language_policy(policy);
                eprintln!("meetingraft-stt: loaded Whisper model {}", model.display());
                Some(LiveCaptionPipeline {
                    engine: Box::new(engine),
                    backend: SttBackendKind::Whisper,
                })
            }
            Err(err) => {
                eprintln!("meetingraft-stt: Whisper load failed ({err}) — Mock fallback");
                None
            }
        }
    }
    #[cfg(not(feature = "whisper"))]
    {
        let _ = (model, policy);
        eprintln!(
            "meetingraft-stt: model present but crate built without `whisper` feature — Mock"
        );
        None
    }
}

fn pcm_bytes_to_i16(pcm: &[u8]) -> Vec<i16> {
    pcm.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::CaptionPhase;

    #[test]
    fn pipeline_mock_roundtrip() {
        let mut pipeline = LiveCaptionPipeline::mock(LanguagePolicy::default_v1());
        assert_eq!(pipeline.backend(), SttBackendKind::Mock);
        let mut pcm = Vec::new();
        for _ in 0..3200 {
            pcm.extend_from_slice(&3000_i16.to_le_bytes());
        }
        let partials = pipeline.push_pcm_bytes(&pcm, 16_000);
        assert!(partials.iter().any(|e| e.phase == CaptionPhase::Partial));
        let mut silence = Vec::new();
        for _ in 0..4800 {
            silence.extend_from_slice(&0_i16.to_le_bytes());
        }
        let finals = pipeline.push_pcm_bytes(&silence, 16_000);
        assert!(finals.iter().any(|e| e.phase == CaptionPhase::Final));
    }

    #[test]
    fn from_data_root_without_model_is_mock() {
        let root = std::env::temp_dir().join("mr-stt-no-model");
        let _ = std::fs::remove_dir_all(&root);
        let p = LiveCaptionPipeline::from_data_root(&root, LanguagePolicy::default_v1(), None);
        assert_eq!(p.backend(), SttBackendKind::Mock);
    }
}
