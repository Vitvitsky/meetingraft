//! Очередь запросов для Apple / host bridge (без Cocoa в Rust).

use std::collections::VecDeque;

use domain::{AudioChannel, CaptionPhase, SpeechLanguage};
use uuid::Uuid;

/// DTO для Swift: перевести и вернуть через `complete_host_translation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTranslationRequest {
    pub id: String,
    pub text: String,
    pub source_code: String,
    pub target_code: String,
    pub phase_final: bool,
}

impl HostTranslationRequest {
    pub fn new(
        text: impl Into<String>,
        source: SpeechLanguage,
        target: SpeechLanguage,
        phase: CaptionPhase,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            text: text.into(),
            source_code: source.code().to_owned(),
            target_code: target.code().to_owned(),
            phase_final: matches!(phase, CaptionPhase::Final),
        }
    }
}

/// Pending host jobs + correlation for complete.
#[derive(Debug, Default)]
pub struct HostPendingQueue {
    requests: VecDeque<HostTranslationRequest>,
    /// id → (phase_final, канал говорящего) для complete.
    awaiting: std::collections::HashMap<String, (bool, AudioChannel)>,
}

impl HostPendingQueue {
    pub fn enqueue(
        &mut self,
        text: &str,
        source: SpeechLanguage,
        target: SpeechLanguage,
        phase: CaptionPhase,
        channel: AudioChannel,
    ) {
        let req = HostTranslationRequest::new(text, source, target, phase);
        self.awaiting
            .insert(req.id.clone(), (req.phase_final, channel));
        self.requests.push_back(req);
    }

    pub fn drain(&mut self) -> Vec<HostTranslationRequest> {
        self.requests.drain(..).collect()
    }

    /// Возвращает (phase_final, канал), если id известен.
    pub fn take_awaiting(&mut self, id: &str) -> Option<(bool, AudioChannel)> {
        self.awaiting.remove(id)
    }

    pub fn clear(&mut self) {
        self.requests.clear();
        self.awaiting.clear();
    }
}
