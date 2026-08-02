//! UniFFI facade MeetingRaft: Swift ↔ session + recording + live STT.

uniffi::setup_scaffolding!();

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use domain::{AudioChannel, CaptionPhase, LanguagePolicy, SessionState};
use session::MeetingSession;
use storage::AudioManifestStore;
use stt::{LiveCaptionPipeline, SttBackendKind, models_dir, resolve_whisper_model};

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
    stt: Option<LiveCaptionPipeline>,
    stt_backend: String,
    pending_live_captions: VecDeque<FfiCaptionEvent>,
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
    std::env::temp_dir().join("meetingraft-default")
}

fn to_ffi(event: domain::CaptionEvent) -> FfiCaptionEvent {
    FfiCaptionEvent {
        id: event.id,
        text: event.text,
        phase: match event.phase {
            CaptionPhase::Partial => FfiCaptionPhase::Partial,
            CaptionPhase::Final => FfiCaptionPhase::Final,
        },
    }
}

#[uniffi::export]
impl MeetingCore {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        Self::with_data_root(default_data_root().to_string_lossy().into_owned())
    }

    #[uniffi::constructor]
    pub fn with_data_root(data_root: String) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: Mutex::new(MeetingCoreInner {
                session: MeetingSession::new(),
                started_at: None,
                store: None,
                recording_session_id: None,
                data_root: PathBuf::from(data_root),
                stt: None,
                stt_backend: "idle".to_string(),
                pending_live_captions: VecDeque::new(),
            }),
        })
    }

    /// Старт demo captions (scripted, без аудио).
    pub fn start_demo(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        if guard.session.state() == SessionState::Ended {
            guard.session = MeetingSession::new();
            guard.started_at = None;
        }
        let _ = guard.session.start(LanguagePolicy::default_v1());
        guard.started_at = Some(Instant::now());
    }

    pub fn stop(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let _ = guard.session.stop();
        guard.started_at = None;
    }

    pub fn state(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        match guard.session.state() {
            SessionState::Idle => "idle".to_string(),
            SessionState::Live => "live".to_string(),
            SessionState::Ended => "ended".to_string(),
        }
    }

    /// Demo script events (не STT).
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
            .map(to_ffi)
            .collect()
    }

    /// Live STT captions, накопленные после ingest.
    pub fn drain_live_captions(&self) -> Vec<FfiCaptionEvent> {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.pending_live_captions.drain(..).collect()
    }

    /// Recording + live STT (Whisper если модель есть и feature включён, иначе Mock).
    pub fn start_recording(&self, session_id: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let root = guard.data_root.clone();
        match AudioManifestStore::open(&root) {
            Ok(mut store) => {
                if let Err(err) = store.begin_session(&session_id, now_ms()) {
                    return err.to_string();
                }
                let pipeline =
                    LiveCaptionPipeline::from_data_root(&root, LanguagePolicy::default_v1());
                guard.stt_backend = match pipeline.backend() {
                    SttBackendKind::Mock => "mock".to_string(),
                    SttBackendKind::Whisper => "whisper".to_string(),
                };
                guard.store = Some(store);
                guard.recording_session_id = Some(session_id);
                guard.stt = Some(pipeline);
                guard.pending_live_captions.clear();
                String::new()
            }
            Err(err) => err.to_string(),
        }
    }

    /// `idle` | `mock` | `whisper`.
    pub fn stt_backend(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.stt_backend.clone()
    }

    /// Абсолютный путь к найденной ggml-модели или пустая строка.
    pub fn whisper_model_path(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        resolve_whisper_model(&guard.data_root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Каталог моделей: `{data_root}/models`.
    pub fn models_directory(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        models_dir(&guard.data_root).to_string_lossy().into_owned()
    }

    pub fn ingest_audio_chunk(
        &self,
        channel: FfiAudioChannel,
        pcm: Vec<u8>,
        sample_rate: u32,
        timestamp_ms: u64,
    ) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let domain_channel = match channel {
            FfiAudioChannel::Mic => AudioChannel::Mic,
            FfiAudioChannel::System => AudioChannel::System,
        };
        {
            let Some(store) = guard.store.as_mut() else {
                return "recording not started".to_string();
            };
            if let Err(err) = store.append_chunk(domain_channel, &pcm, sample_rate, timestamp_ms) {
                return err.to_string();
            }
        }

        if matches!(channel, FfiAudioChannel::Mic) {
            let session_id = guard.recording_session_id.clone();
            let events = guard
                .stt
                .as_mut()
                .map(|p| p.push_pcm_bytes(&pcm, sample_rate))
                .unwrap_or_default();
            if let Some(sid) = session_id {
                for event in events {
                    if let Some(store) = guard.store.as_mut() {
                        let _ = store.append_caption(&sid, &event, now_ms());
                    }
                    guard.pending_live_captions.push_back(to_ffi(event));
                }
            }
        }
        String::new()
    }

    pub fn stop_recording(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let sid = guard.recording_session_id.clone();
        let flushed = guard.stt.as_mut().map(|p| p.flush()).unwrap_or_default();
        if let Some(sid) = sid {
            for event in flushed {
                if let Some(store) = guard.store.as_mut() {
                    let _ = store.append_caption(&sid, &event, now_ms());
                }
                guard.pending_live_captions.push_back(to_ffi(event));
            }
        }
        if let Some(store) = guard.store.as_mut() {
            store.end_session();
        }
        guard.store = None;
        guard.recording_session_id = None;
        guard.stt = None;
        guard.stt_backend = "idle".to_string();
    }

    pub fn manifest_chunk_count(&self, session_id: String) -> u64 {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let Some(store) = guard.store.as_ref() else {
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

    /// Число сохранённых live captions.
    pub fn caption_event_count(&self, session_id: String) -> u64 {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let Some(store) = guard.store.as_ref() else {
            drop(guard);
            let root = self
                .inner
                .lock()
                .expect("meeting core poisoned")
                .data_root
                .clone();
            return AudioManifestStore::open(root)
                .and_then(|s| s.caption_count(&session_id))
                .unwrap_or(0);
        };
        store.caption_count(&session_id).unwrap_or(0)
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
        core.start_demo();
        let events = core.drain_events();
        assert!(!events.is_empty());
        assert_eq!(events[0].text, "Добро пожаловать");
        thread::sleep(Duration::from_millis(850));
        let next = core.drain_events();
        assert!(!next.is_empty());
        core.stop();
    }

    #[test]
    fn recording_ingests_mic_and_system_chunks() {
        let root = std::env::temp_dir().join(format!("mr-ffi-rec-{}", now_ms()));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(core.start_recording("rec-1".into()).is_empty());
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

    #[test]
    fn loud_mic_produces_live_captions() {
        let root = std::env::temp_dir().join(format!("mr-ffi-stt-{}", now_ms()));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(core.start_recording("live-1".into()).is_empty());
        let mut loud = Vec::new();
        for _ in 0..4000 {
            loud.extend_from_slice(&3000_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, loud, 16_000, 0)
                .is_empty()
        );
        let live = core.drain_live_captions();
        assert!(
            live.iter()
                .any(|e| matches!(e.phase, FfiCaptionPhase::Partial)),
            "expected partial, got {live:?}"
        );
        let mut silence = Vec::new();
        for _ in 0..6000 {
            silence.extend_from_slice(&0_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, silence, 16_000, 500)
                .is_empty()
        );
        let finals = core.drain_live_captions();
        assert!(
            finals
                .iter()
                .any(|e| matches!(e.phase, FfiCaptionPhase::Final))
        );
        assert!(core.caption_event_count("live-1".into()) >= 1);
        core.stop_recording();
        let _ = std::fs::remove_dir_all(&root);
    }
}
