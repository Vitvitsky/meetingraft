//! Live caption events (отдельно от final transcript — ADR-002).

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
}
