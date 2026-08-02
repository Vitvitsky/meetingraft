//! UniFFI facade MeetingRaft: Swift ↔ session engine + recording.

uniffi::setup_scaffolding!();

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use domain::{AudioChannel, CaptionPhase, LanguagePolicy, SessionState};
use session::MeetingSession;
use storage::AudioManifestStore;

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

/// Канал захвата для Swift (ADR-004).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiAudioChannel {
    Mic,
    System,
}

struct MeetingCoreInner {
    session: MeetingSession,
    started_at: Option<Instant>,
    store: Option<AudioManifestStore>,
    recording_session_id: Option<String>,
    data_root: PathBuf,
}

/// Фасад сессии для macOS shell.
#[derive(uniffi::Object)]
pub struct MeetingCore {
    inner: Mutex<MeetingCoreInner>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn default_data_root() -> PathBuf {
    // Тесты/CI: temp. Prod Swift передаёт путь через `with_data_root`.
    std::env::temp_dir().join("meetingraft-default")
}

#[uniffi::export]
impl MeetingCore {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        Self::with_data_root(default_data_root().to_string_lossy().into_owned())
    }

    /// Конструктор с корнем Application Support.
    #[uniffi::constructor]
    pub fn with_data_root(data_root: String) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: Mutex::new(MeetingCoreInner {
                session: MeetingSession::new(),
                started_at: None,
                store: None,
                recording_session_id: None,
                data_root: PathBuf::from(data_root),
            }),
        })
    }

    /// Старт demo captions с политикой v1 (ru primary).
    pub fn start_demo(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
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

    /// Начать recording: создаёт session + SQLite store. Пустая строка = ok, иначе ошибка.
    pub fn start_recording(&self, session_id: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let root = guard.data_root.clone();
        match AudioManifestStore::open(&root) {
            Ok(mut store) => {
                if let Err(err) = store.begin_session(&session_id, now_ms()) {
                    return err.to_string();
                }
                guard.store = Some(store);
                guard.recording_session_id = Some(session_id);
                String::new()
            }
            Err(err) => err.to_string(),
        }
    }

    /// Принять PCM i16 LE chunk.
    pub fn ingest_audio_chunk(
        &self,
        channel: FfiAudioChannel,
        pcm: Vec<u8>,
        sample_rate: u32,
        timestamp_ms: u64,
    ) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let Some(store) = guard.store.as_mut() else {
            return "recording not started".to_string();
        };
        let domain_channel = match channel {
            FfiAudioChannel::Mic => AudioChannel::Mic,
            FfiAudioChannel::System => AudioChannel::System,
        };
        match store.append_chunk(domain_channel, &pcm, sample_rate, timestamp_ms) {
            Ok(_) => String::new(),
            Err(err) => err.to_string(),
        }
    }

    /// Остановить recording.
    pub fn stop_recording(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        if let Some(store) = guard.store.as_mut() {
            store.end_session();
        }
        guard.store = None;
        guard.recording_session_id = None;
    }

    /// Число чанков в manifest для session.
    pub fn manifest_chunk_count(&self, session_id: String) -> u64 {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let Some(store) = guard.store.as_ref() else {
            // Store закрыт — открыть read-only счётчик.
            drop(guard);
            let root = self
                .inner
                .lock()
                .expect("meeting core poisoned")
                .data_root
                .clone();
            return AudioManifestStore::open(root)
                .and_then(|s| s.chunk_count(&session_id))
                .unwrap_or(0);
        };
        store.chunk_count(&session_id).unwrap_or(0)
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

    #[test]
    fn recording_ingests_mic_and_system_chunks() {
        let root = std::env::temp_dir().join(format!("mr-ffi-rec-{}", now_ms()));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let err = core.start_recording("rec-1".into());
        assert!(err.is_empty(), "{err}");
        let pcm = vec![1_u8, 0, 2, 0, 3, 0, 4, 0];
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, pcm.clone(), 16_000, 0)
                .is_empty()
        );
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::System, pcm, 16_000, 100)
                .is_empty()
        );
        assert_eq!(core.manifest_chunk_count("rec-1".into()), 2);
        core.stop_recording();
        let _ = std::fs::remove_dir_all(&root);
    }
}
