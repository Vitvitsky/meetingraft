//! Автомат meeting session.

use crate::fake_captions::FakeCaptionProducer;
use domain::{CaptionEvent, LanguagePolicy, SessionState};

/// Ошибки переходов сессии.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    InvalidTransition,
}

/// Сессия встречи: Idle → Live → Ended.
pub struct MeetingSession {
    state: SessionState,
    policy: Option<LanguagePolicy>,
    producer: Option<FakeCaptionProducer>,
}

impl MeetingSession {
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
            policy: None,
            producer: None,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn policy(&self) -> Option<&LanguagePolicy> {
        self.policy.as_ref()
    }

    /// Старт live-сессии с языковой политикой.
    pub fn start(&mut self, policy: LanguagePolicy) -> Result<(), SessionError> {
        if self.state != SessionState::Idle {
            return Err(SessionError::InvalidTransition);
        }
        let producer = FakeCaptionProducer::for_language(policy.primary);
        self.policy = Some(policy);
        self.producer = Some(producer);
        self.state = SessionState::Live;
        Ok(())
    }

    /// Остановка live-сессии.
    pub fn stop(&mut self) -> Result<(), SessionError> {
        if self.state != SessionState::Live {
            return Err(SessionError::InvalidTransition);
        }
        self.producer = None;
        self.state = SessionState::Ended;
        Ok(())
    }

    /// Продвигает fake producer по `elapsed_ms` от старта сессии.
    pub fn push_tick(&mut self, elapsed_ms: u64) -> Vec<CaptionEvent> {
        if self.state != SessionState::Live {
            return Vec::new();
        }
        self.producer
            .as_mut()
            .map(|p| p.drain_due(elapsed_ms))
            .unwrap_or_default()
    }
}

impl Default for MeetingSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::CaptionPhase;

    #[test]
    fn start_moves_idle_to_live() {
        let mut session = MeetingSession::new();
        assert_eq!(session.state(), SessionState::Idle);
        session.start(LanguagePolicy::default_v1()).unwrap();
        assert_eq!(session.state(), SessionState::Live);
    }

    #[test]
    fn stop_from_live_ends() {
        let mut session = MeetingSession::new();
        session.start(LanguagePolicy::default_v1()).unwrap();
        session.stop().unwrap();
        assert_eq!(session.state(), SessionState::Ended);
    }

    #[test]
    fn cannot_start_twice() {
        let mut session = MeetingSession::new();
        session.start(LanguagePolicy::default_v1()).unwrap();
        assert_eq!(
            session.start(LanguagePolicy::default_v1()),
            Err(SessionError::InvalidTransition)
        );
    }

    #[test]
    fn tick_emits_partial_then_final() {
        let mut session = MeetingSession::new();
        session.start(LanguagePolicy::default_v1()).unwrap();
        let first = session.push_tick(0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].phase, CaptionPhase::Partial);
        assert_eq!(first[0].text, "Добро пожаловать");
        let second = session.push_tick(800);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].phase, CaptionPhase::Final);
    }

    #[test]
    fn english_policy_emits_english_demo() {
        let mut session = MeetingSession::new();
        session
            .start(LanguagePolicy::with_primary(domain::SpeechLanguage::En))
            .unwrap();
        let first = session.push_tick(0);
        assert_eq!(first[0].text, "Welcome");
    }
}
