//! Live caption events (отдельно от final transcript — ADR-002).

use crate::AudioChannel;

/// Фаза caption-события в live-режиме.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptionPhase {
    Partial,
    Final,
}

/// Событие live-субтитров.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionEvent {
    pub id: String,
    pub text: String,
    pub phase: CaptionPhase,
    /// Канал говорящего (ADR-009). Движки STT его не знают — работают с
    /// миксом; канал проставляет `LiveCaptionPipeline` по доминанту слотов.
    pub channel: AudioChannel,
}

impl CaptionEvent {
    /// Событие без атрибуции; канал проставляется выше по стеку.
    pub fn new(id: String, text: String, phase: CaptionPhase) -> Self {
        Self {
            id,
            text,
            phase,
            channel: AudioChannel::Mic,
        }
    }
}
