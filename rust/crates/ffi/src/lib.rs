//! UniFFI facade MeetingRaft: Swift ↔ session engine.

uniffi::setup_scaffolding!();

use std::sync::Mutex;
use std::time::Instant;

use domain::{CaptionPhase, LanguagePolicy, SessionState};
use session::MeetingSession;

/// Фаза caption для Swift.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiCaptionPhase {
    Partial,
    Final,
}

/// Caption event DTO для Swift.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCaptionEvent {
    pub id: String,
    pub text: String,
    pub phase: FfiCaptionPhase,
}

struct MeetingCoreInner {
    session: MeetingSession,
    started_at: Option<Instant>,
}

/// Фасад сессии для macOS shell.
#[derive(uniffi::Object)]
pub struct MeetingCore {
    inner: Mutex<MeetingCoreInner>,
}

#[uniffi::export]
impl MeetingCore {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: Mutex::new(MeetingCoreInner {
                session: MeetingSession::new(),
                started_at: None,
            }),
        })
    }

    /// Старт demo captions с политикой v1 (ru primary).
    pub fn start_demo(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        // После Ended разрешаем новый цикл: сбрасываем в Idle.
        if guard.session.state() == SessionState::Ended {
            guard.session = MeetingSession::new();
            guard.started_at = None;
        }
        let _ = guard.session.start(LanguagePolicy::default_v1());
        guard.started_at = Some(Instant::now());
    }

    /// Остановка demo.
    pub fn stop(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let _ = guard.session.stop();
        guard.started_at = None;
    }

    /// Состояние: idle | live | ended.
    pub fn state(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        match guard.session.state() {
            SessionState::Idle => "idle".to_string(),
            SessionState::Live => "live".to_string(),
            SessionState::Ended => "ended".to_string(),
        }
    }

    /// Слить накопившиеся caption events по elapsed time.
    pub fn drain_events(&self) -> Vec<FfiCaptionEvent> {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let Some(started) = guard.started_at else {
            return Vec::new();
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        guard
            .session
            .push_tick(elapsed_ms)
            .into_iter()
            .map(|event| FfiCaptionEvent {
                id: event.id,
                text: event.text,
                phase: match event.phase {
                    CaptionPhase::Partial => FfiCaptionPhase::Partial,
                    CaptionPhase::Final => FfiCaptionPhase::Final,
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn start_demo_drains_russian_caption() {
        let core = MeetingCore::new();
        assert_eq!(core.state(), "idle");
        core.start_demo();
        assert_eq!(core.state(), "live");
        let events = core.drain_events();
        assert!(!events.is_empty());
        assert_eq!(events[0].text, "Добро пожаловать");
        assert!(matches!(events[0].phase, FfiCaptionPhase::Partial));
        thread::sleep(Duration::from_millis(850));
        let next = core.drain_events();
        assert!(!next.is_empty());
        assert!(matches!(next[0].phase, FfiCaptionPhase::Final));
        core.stop();
        assert_eq!(core.state(), "ended");
    }
}
