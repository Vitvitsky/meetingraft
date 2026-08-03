//! Обёртка: PCM bytes → i16 → SttEngine; выбор Mock / Whisper.

use std::path::Path;

use domain::{AudioChannel, CaptionEvent, CaptionPhase, LanguagePolicy};

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
///
/// Движок работает с миксом каналов и не знает, кто говорит, поэтому
/// pipeline считает, сколько кадров с последнего события пришло с каждого
/// канала, и ставит мажоритарный канал на выданные события (ADR-009).
pub struct LiveCaptionPipeline {
    engine: Box<dyn SttEngine>,
    backend: SttBackendKind,
    mic_frames: u32,
    system_frames: u32,
}

impl LiveCaptionPipeline {
    pub fn mock(policy: LanguagePolicy) -> Self {
        let mut engine = MockSttEngine::new();
        engine.set_language_policy(policy);
        Self::wrap(Box::new(engine), SttBackendKind::Mock)
    }

    fn wrap(engine: Box<dyn SttEngine>, backend: SttBackendKind) -> Self {
        Self {
            engine,
            backend,
            mic_frames: 0,
            system_frames: 0,
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

    /// Кадр микса с известным доминирующим каналом.
    pub fn push_frame(
        &mut self,
        pcm: &[i16],
        sample_rate: u32,
        dominant: AudioChannel,
    ) -> Vec<CaptionEvent> {
        match dominant {
            AudioChannel::Mic => self.mic_frames += 1,
            AudioChannel::System => self.system_frames += 1,
        }
        let events = self.engine.push_pcm(pcm, sample_rate);
        self.attribute(events)
    }

    /// Путь без микшера: сырые байты считаются микрофонными.
    pub fn push_pcm_bytes(&mut self, pcm: &[u8], sample_rate: u32) -> Vec<CaptionEvent> {
        let samples = pcm_bytes_to_i16(pcm);
        self.push_frame(&samples, sample_rate, AudioChannel::Mic)
    }

    pub fn flush(&mut self) -> Vec<CaptionEvent> {
        let events = self.engine.flush();
        self.attribute(events)
    }

    /// Проставить канал и сбросить счётчики после завершённого сегмента.
    fn attribute(&mut self, mut events: Vec<CaptionEvent>) -> Vec<CaptionEvent> {
        if events.is_empty() {
            return events;
        }
        let channel = self.majority_channel();
        for event in &mut events {
            event.channel = channel;
        }
        if events.iter().any(|e| e.phase == CaptionPhase::Final) {
            self.mic_frames = 0;
            self.system_frames = 0;
        }
        events
    }

    fn majority_channel(&self) -> AudioChannel {
        if self.system_frames > self.mic_frames {
            AudioChannel::System
        } else {
            AudioChannel::Mic
        }
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
                Some(LiveCaptionPipeline::wrap(
                    Box::new(engine),
                    SttBackendKind::Whisper,
                ))
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

/// Декодирование PCM-байтов из Swift: i16 little-endian.
pub fn pcm_bytes_to_i16(pcm: &[u8]) -> Vec<i16> {
    pcm.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Громкий кадр: Mock-движок считает это речью.
    fn speech(frames: usize) -> Vec<i16> {
        (0..frames)
            .map(|i| if i % 2 == 0 { 3000 } else { -3000 })
            .collect()
    }

    fn silence(frames: usize) -> Vec<i16> {
        vec![0; frames]
    }

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
    fn events_take_majority_channel_of_the_segment() {
        let mut pipeline = LiveCaptionPipeline::mock(LanguagePolicy::default_v1());

        // Сегмент, где системный канал доминировал в большинстве кадров.
        // Mock отдаёт partial, накопив 3200 кадров речи, — до этого момента
        // нужно успеть набрать перевес.
        pipeline.push_frame(&speech(1000), 16_000, AudioChannel::Mic);
        pipeline.push_frame(&speech(1000), 16_000, AudioChannel::System);
        pipeline.push_frame(&speech(1000), 16_000, AudioChannel::System);
        let events = pipeline.push_frame(&speech(1000), 16_000, AudioChannel::System);

        assert!(!events.is_empty(), "ожидался partial");
        assert!(events.iter().all(|e| e.channel == AudioChannel::System));
    }

    #[test]
    fn tally_resets_after_final_so_next_segment_is_attributed_separately() {
        let mut pipeline = LiveCaptionPipeline::mock(LanguagePolicy::default_v1());

        pipeline.push_frame(&speech(3200), 16_000, AudioChannel::System);
        let finals = pipeline.push_frame(&silence(4800), 16_000, AudioChannel::System);
        assert!(finals.iter().any(|e| e.phase == CaptionPhase::Final));
        assert!(finals.iter().all(|e| e.channel == AudioChannel::System));

        // Новый сегмент целиком с микрофона: прошлый перевес не наследуется.
        let events = pipeline.push_frame(&speech(3200), 16_000, AudioChannel::Mic);

        assert!(!events.is_empty());
        assert!(events.iter().all(|e| e.channel == AudioChannel::Mic));
    }

    #[test]
    fn byte_path_without_mixer_is_mic() {
        let mut pipeline = LiveCaptionPipeline::mock(LanguagePolicy::default_v1());
        let mut pcm = Vec::new();
        for sample in speech(3200) {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }

        let events = pipeline.push_pcm_bytes(&pcm, 16_000);

        assert!(!events.is_empty());
        assert!(events.iter().all(|e| e.channel == AudioChannel::Mic));
    }

    #[test]
    fn from_data_root_without_model_is_mock() {
        let root = std::env::temp_dir().join("mr-stt-no-model");
        let _ = std::fs::remove_dir_all(&root);
        let p = LiveCaptionPipeline::from_data_root(&root, LanguagePolicy::default_v1(), None);
        assert_eq!(p.backend(), SttBackendKind::Mock);
    }
}
