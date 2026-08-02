//! UniFFI facade MeetingRaft: Swift ↔ session + recording + live STT.

uniffi::setup_scaffolding!();

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use domain::{
    Artifact, ArtifactKind, AudioChannel, CaptionPhase, FinalTranscript, GlossaryScope,
    GlossaryTerm, LanguagePolicy, MeetingSummary, SessionState, SpeechLanguage,
};
use glossary::{GlossaryEngine, active_terms, parse_csv};
use postcall::{assemble_final, make_artifact, render_brief, render_follow_up};
use session::MeetingSession;
use storage::{AudioManifestError, AudioManifestStore};
use stt::{LiveCaptionPipeline, SttBackendKind, models_dir, resolve_whisper_model};
use translate::{
    EffectiveBackend, HostPendingQueue, TranslationBackendKind, TranslationPolicy,
    resolve_effective, translate_now,
};
use uuid::Uuid;

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

/// Область действия термина для Swift.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiGlossaryScope {
    Global,
    Meeting,
}

/// Термин глоссария для Swift.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGlossaryTerm {
    pub id: String,
    pub surface: String,
    pub canonical: String,
    pub language: String,
    pub scope: FfiGlossaryScope,
    pub meeting_id: String,
}

/// Результат CSV-импорта глоссария.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGlossaryImportResult {
    pub imported: u32,
    pub skipped: u32,
    pub error: String,
}

/// Краткая запись встречи для списка истории.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMeetingSummary {
    pub id: String,
    pub started_at_ms: u64,
    pub has_final: bool,
    pub artifact_count: u64,
}

/// Финальная версия транскрипта; пустой `meeting_id` означает отсутствие.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFinalTranscript {
    pub meeting_id: String,
    pub version: u32,
    pub body_markdown: String,
    pub created_at_ms: u64,
}

/// Вид локального post-call артефакта.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiArtifactKind {
    Brief,
    FollowUp,
}

/// Сохранённый post-call артефакт.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiArtifact {
    pub id: String,
    pub meeting_id: String,
    pub kind: FfiArtifactKind,
    pub template_id: String,
    pub body_markdown: String,
    pub created_at_ms: u64,
}

/// Результат генерации без исключения через границу UniFFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGenerateArtifactResult {
    pub artifact: FfiArtifact,
    pub error: String,
}

/// Запрос на host (Apple) перевод — Swift drain → complete.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHostTranslationRequest {
    pub id: String,
    pub text: String,
    pub source_code: String,
    pub target_code: String,
    pub phase: FfiCaptionPhase,
}

struct MeetingCoreInner {
    session: MeetingSession,
    started_at: Option<Instant>,
    store: Option<AudioManifestStore>,
    recording_session_id: Option<String>,
    data_root: PathBuf,
    stt: Option<LiveCaptionPipeline>,
    stt_backend: String,
    glossary: GlossaryEngine,
    pending_live_captions: VecDeque<FfiCaptionEvent>,
    /// Язык распознавания (captions); не путать с целевым языком перевода.
    language_policy: LanguagePolicy,
    translation_policy: TranslationPolicy,
    /// Swift зарегистрировал Apple / host bridge.
    host_translation_available: bool,
    host_translation_queue: HostPendingQueue,
    pending_translations: VecDeque<FfiCaptionEvent>,
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

fn utc_date_label(timestamp_ms: u64) -> String {
    let days_since_epoch = (timestamp_ms / 86_400_000) as i64;
    let shifted_days = days_since_epoch + 719_468;
    let era = shifted_days / 146_097;
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}")
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

/// Captions + опциональный отдельный translation event (не подменяет caption).
fn enqueue_caption(inner: &mut MeetingCoreInner, event: domain::CaptionEvent) {
    maybe_enqueue_translation(inner, &event);
    inner.pending_live_captions.push_back(to_ffi(event));
}

fn maybe_enqueue_translation(inner: &mut MeetingCoreInner, event: &domain::CaptionEvent) {
    if !inner.translation_policy.enabled {
        return;
    }
    let target = inner.translation_policy.target;
    let source = inner.language_policy.primary;
    if target == source {
        return;
    }
    let effective = resolve_effective(
        &inner.translation_policy,
        inner.host_translation_available,
    );
    match effective {
        EffectiveBackend::Off => {}
        EffectiveBackend::AppleHost => {
            inner
                .host_translation_queue
                .enqueue(&event.text, source, target, event.phase);
        }
        other => match translate_now(
            other,
            &inner.translation_policy,
            &event.text,
            source,
            target,
        ) {
            Ok(text) => {
                let translated = domain::CaptionEvent {
                    id: Uuid::new_v4().to_string(),
                    text,
                    phase: event.phase,
                };
                inner.pending_translations.push_back(to_ffi(translated));
            }
            Err(_) => {
                // Молча пропускаем битый translate — captions остаются.
            }
        },
    }
}

fn glossary_term_to_ffi(term: GlossaryTerm) -> FfiGlossaryTerm {
    let (scope, meeting_id) = match term.scope {
        GlossaryScope::Global => (FfiGlossaryScope::Global, String::new()),
        GlossaryScope::Meeting { meeting_id } => (FfiGlossaryScope::Meeting, meeting_id),
    };
    FfiGlossaryTerm {
        id: term.id,
        surface: term.surface,
        canonical: term.canonical,
        language: term.language.code().to_owned(),
        scope,
        meeting_id,
    }
}

fn glossary_term_from_ffi(term: FfiGlossaryTerm) -> Result<GlossaryTerm, String> {
    let surface = term.surface.trim();
    if surface.is_empty() {
        return Err("surface не может быть пустым".into());
    }
    let canonical = term.canonical.trim();
    if canonical.is_empty() {
        return Err("canonical не может быть пустым".into());
    }
    let language = match term.language.as_str() {
        "ru" => SpeechLanguage::Ru,
        "en" => SpeechLanguage::En,
        "es" => SpeechLanguage::Es,
        value => return Err(format!("unsupported glossary language: {value}")),
    };
    let scope = match term.scope {
        FfiGlossaryScope::Global => GlossaryScope::Global,
        FfiGlossaryScope::Meeting if !term.meeting_id.is_empty() => GlossaryScope::Meeting {
            meeting_id: term.meeting_id,
        },
        FfiGlossaryScope::Meeting => return Err("meeting glossary term requires meeting_id".into()),
    };
    Ok(GlossaryTerm {
        id: term.id,
        surface: surface.to_owned(),
        canonical: canonical.to_owned(),
        language,
        scope,
    })
}

fn meeting_summary_to_ffi(summary: MeetingSummary) -> FfiMeetingSummary {
    FfiMeetingSummary {
        id: summary.id,
        started_at_ms: summary.started_at_ms,
        has_final: summary.has_final,
        artifact_count: summary.artifact_count,
    }
}

fn final_transcript_to_ffi(transcript: FinalTranscript) -> FfiFinalTranscript {
    FfiFinalTranscript {
        meeting_id: transcript.meeting_id,
        version: transcript.version,
        body_markdown: transcript.body_markdown,
        created_at_ms: transcript.created_at_ms,
    }
}

fn empty_final_transcript() -> FfiFinalTranscript {
    FfiFinalTranscript {
        meeting_id: String::new(),
        version: 0,
        body_markdown: String::new(),
        created_at_ms: 0,
    }
}

fn artifact_to_ffi(artifact: Artifact) -> FfiArtifact {
    FfiArtifact {
        id: artifact.id,
        meeting_id: artifact.meeting_id,
        kind: match artifact.kind {
            ArtifactKind::Brief => FfiArtifactKind::Brief,
            ArtifactKind::FollowUp => FfiArtifactKind::FollowUp,
        },
        template_id: artifact.template_id,
        body_markdown: artifact.body_markdown,
        created_at_ms: artifact.created_at_ms,
    }
}

fn empty_artifact() -> FfiArtifact {
    FfiArtifact {
        id: String::new(),
        meeting_id: String::new(),
        kind: FfiArtifactKind::Brief,
        template_id: String::new(),
        body_markdown: String::new(),
        created_at_ms: 0,
    }
}

fn read_store<T>(
    inner: &MeetingCoreInner,
    read: impl FnOnce(&AudioManifestStore) -> Result<T, AudioManifestError>,
) -> Result<T, AudioManifestError> {
    if let Some(store) = inner.store.as_ref() {
        read(store)
    } else {
        let store = AudioManifestStore::open(&inner.data_root)?;
        read(&store)
    }
}

fn write_store<T>(
    inner: &mut MeetingCoreInner,
    write: impl FnOnce(&mut AudioManifestStore) -> Result<T, AudioManifestError>,
) -> Result<T, AudioManifestError> {
    if let Some(store) = inner.store.as_mut() {
        write(store)
    } else {
        let mut store = AudioManifestStore::open(&inner.data_root)?;
        write(&mut store)
    }
}

fn assemble_and_store_final(
    store: &mut AudioManifestStore,
    meeting_id: &str,
) -> Result<FinalTranscript, AudioManifestError> {
    let captions = store.list_captions(meeting_id)?;
    let terms = store.list_glossary_terms()?;
    let glossary = GlossaryEngine::from_terms(active_terms(&terms, Some(meeting_id)));
    let transcript = assemble_final(
        meeting_id,
        &captions,
        |text| glossary.normalize_caption(text),
        now_ms(),
    );
    store.upsert_final_transcript(&transcript)?;
    Ok(transcript)
}

fn list_glossary_terms(inner: &MeetingCoreInner) -> Result<Vec<GlossaryTerm>, AudioManifestError> {
    if let Some(store) = inner.store.as_ref() {
        store.list_glossary_terms()
    } else {
        AudioManifestStore::open(&inner.data_root)?.list_glossary_terms()
    }
}

fn refresh_glossary(inner: &mut MeetingCoreInner) -> Result<(), String> {
    let terms = list_glossary_terms(inner).map_err(|error| error.to_string())?;
    inner.glossary =
        GlossaryEngine::from_terms(active_terms(&terms, inner.recording_session_id.as_deref()));
    if inner.stt_backend == "whisper" {
        let prompt = inner.glossary.build_whisper_prompt(800);
        if let Some(pipeline) = inner.stt.as_mut() {
            pipeline.set_initial_prompt(&prompt);
        }
    }
    Ok(())
}

fn mutate_glossary(
    inner: &mut MeetingCoreInner,
    mutate: impl FnOnce(&mut AudioManifestStore) -> Result<(), AudioManifestError>,
) -> Result<(), String> {
    if let Some(store) = inner.store.as_mut() {
        mutate(store).map_err(|error| error.to_string())?;
    } else {
        let mut store =
            AudioManifestStore::open(&inner.data_root).map_err(|error| error.to_string())?;
        mutate(&mut store).map_err(|error| error.to_string())?;
    }
    refresh_glossary(inner)
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
                glossary: GlossaryEngine::from_terms(Vec::new()),
                pending_live_captions: VecDeque::new(),
                language_policy: LanguagePolicy::default_v1(),
                translation_policy: TranslationPolicy::disabled(),
                host_translation_available: false,
                host_translation_queue: HostPendingQueue::default(),
                pending_translations: VecDeque::new(),
            }),
        })
    }

    /// Primary язык распознавания (`ru` | `en` | `es`). Не включает перевод.
    pub fn set_session_language(&self, primary_code: String) -> String {
        let Some(primary) = SpeechLanguage::from_code(&primary_code) else {
            return format!("unsupported language: {primary_code}");
        };
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let policy = LanguagePolicy::with_primary(primary);
        guard.language_policy = policy.clone();
        if let Some(pipeline) = guard.stt.as_mut() {
            pipeline.set_language_policy(policy);
        }
        String::new()
    }

    pub fn session_language(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.language_policy.primary.code().to_owned()
    }

    /// Включить/выключить sync-перевод и задать target (`ru`|`en`|`es`).
    pub fn set_live_translation(&self, enabled: bool, target_code: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        if !enabled {
            guard.translation_policy.enabled = false;
            guard.translation_policy.backend = TranslationBackendKind::Off;
            guard.pending_translations.clear();
            guard.host_translation_queue.clear();
            return String::new();
        }
        let Some(target) = SpeechLanguage::from_code(&target_code) else {
            return format!("unsupported translation target: {target_code}");
        };
        if target == guard.language_policy.primary {
            return "translation target must differ from session language".to_string();
        }
        if !guard.language_policy.is_allowed(target) {
            return format!("translation target not allowed: {target_code}");
        }
        guard.translation_policy.enabled = true;
        guard.translation_policy.target = target;
        if matches!(
            guard.translation_policy.backend,
            TranslationBackendKind::Off
        ) {
            guard.translation_policy.backend = TranslationBackendKind::Auto;
        }
        String::new()
    }

    pub fn live_translation_target(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        if guard.translation_policy.enabled {
            guard.translation_policy.target.code().to_owned()
        } else {
            String::new()
        }
    }

    /// Backend: `off` | `auto` | `stub` | `apple` | `backend` | `local_llm`.
    /// `base_url` используется для `backend` / `auto→backend`.
    pub fn set_translation_backend(&self, kind_code: String, base_url: String) -> String {
        let Some(kind) = TranslationBackendKind::from_code(&kind_code) else {
            return format!("unsupported translation backend: {kind_code}");
        };
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.translation_policy.backend = kind;
        let trimmed = base_url.trim();
        guard.translation_policy.backend_base_url = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        };
        if matches!(kind, TranslationBackendKind::Off) {
            guard.translation_policy.enabled = false;
            guard.pending_translations.clear();
            guard.host_translation_queue.clear();
        }
        String::new()
    }

    pub fn translation_backend(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.translation_policy.backend.code().to_owned()
    }

    /// Фактический backend после резолва `auto` (для UI/debug).
    pub fn effective_translation_backend(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        resolve_effective(
            &guard.translation_policy,
            guard.host_translation_available,
        )
        .code()
        .to_owned()
    }

    /// Swift: host bridge готов (Apple Translation или stub).
    pub fn set_host_translation_available(&self, available: bool) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.host_translation_available = available;
    }

    pub fn drain_host_translation_requests(&self) -> Vec<FfiHostTranslationRequest> {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard
            .host_translation_queue
            .drain()
            .into_iter()
            .map(|req| FfiHostTranslationRequest {
                id: req.id,
                text: req.text,
                source_code: req.source_code,
                target_code: req.target_code,
                phase: if req.phase_final {
                    FfiCaptionPhase::Final
                } else {
                    FfiCaptionPhase::Partial
                },
            })
            .collect()
    }

    /// Ответ host bridge → в translation stream.
    pub fn complete_host_translation(&self, id: String, translated_text: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let Some(phase_final) = guard.host_translation_queue.take_awaiting(&id) else {
            return format!("unknown host translation id: {id}");
        };
        let text = translated_text.trim();
        if text.is_empty() {
            return String::new();
        }
        guard.pending_translations.push_back(FfiCaptionEvent {
            id: Uuid::new_v4().to_string(),
            text: text.to_owned(),
            phase: if phase_final {
                FfiCaptionPhase::Final
            } else {
                FfiCaptionPhase::Partial
            },
        });
        String::new()
    }

    /// Старт demo captions (scripted, без аудио).
    pub fn start_demo(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        if guard.session.state() == SessionState::Ended {
            guard.session = MeetingSession::new();
            guard.started_at = None;
        }
        let policy = guard.language_policy.clone();
        let _ = guard.session.start(policy);
        guard.started_at = Some(Instant::now());
        guard.pending_translations.clear();
        guard.host_translation_queue.clear();
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
        let events = guard.session.push_tick(elapsed_ms);
        let mut out = Vec::with_capacity(events.len());
        for event in events {
            maybe_enqueue_translation(&mut guard, &event);
            out.push(to_ffi(event));
        }
        out
    }

    /// Live STT captions, накопленные после ingest.
    pub fn drain_live_captions(&self) -> Vec<FfiCaptionEvent> {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.pending_live_captions.drain(..).collect()
    }

    /// Отдельный поток sync-перевода (demo + live); пусто если выключен.
    pub fn drain_live_translations(&self) -> Vec<FfiCaptionEvent> {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.pending_translations.drain(..).collect()
    }

    pub fn list_glossary_terms(&self) -> Vec<FfiGlossaryTerm> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        list_glossary_terms(&guard)
            .unwrap_or_default()
            .into_iter()
            .map(glossary_term_to_ffi)
            .collect()
    }

    pub fn upsert_glossary_term(&self, term: FfiGlossaryTerm) -> String {
        let term = match glossary_term_from_ffi(term) {
            Ok(term) => term,
            Err(error) => return error,
        };
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        mutate_glossary(&mut guard, |store| {
            store.upsert_glossary_term(&term, now_ms())
        })
        .err()
        .unwrap_or_default()
    }

    pub fn delete_glossary_term(&self, id: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        mutate_glossary(&mut guard, |store| store.delete_glossary_term(&id))
            .err()
            .unwrap_or_default()
    }

    pub fn import_glossary_csv(&self, csv: String) -> FfiGlossaryImportResult {
        let (terms, skipped) = match parse_csv(&csv) {
            Ok(result) => result,
            Err(error) => {
                return FfiGlossaryImportResult {
                    imported: 0,
                    skipped: 0,
                    error,
                };
            }
        };
        let imported = terms.len() as u32;
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let error = mutate_glossary(&mut guard, |store| {
            store.replace_glossary_from_import(&terms, now_ms())
        })
        .err()
        .unwrap_or_default();
        FfiGlossaryImportResult {
            imported: if error.is_empty() { imported } else { 0 },
            skipped,
            error,
        }
    }

    /// Встречи, доступные в локальной истории.
    pub fn list_meetings(&self) -> Vec<FfiMeetingSummary> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, AudioManifestStore::list_meeting_summaries)
            .unwrap_or_default()
            .into_iter()
            .map(meeting_summary_to_ffi)
            .collect()
    }

    /// Сохранённые live captions выбранной встречи.
    pub fn list_captions(&self, meeting_id: String) -> Vec<FfiCaptionEvent> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, |store| store.list_captions(&meeting_id))
            .unwrap_or_default()
            .into_iter()
            .map(to_ffi)
            .collect()
    }

    /// Последний финальный транскрипт или пустой DTO.
    pub fn get_final_transcript(&self, meeting_id: String) -> FfiFinalTranscript {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, |store| store.get_final_transcript(&meeting_id))
            .ok()
            .flatten()
            .map(final_transcript_to_ffi)
            .unwrap_or_else(empty_final_transcript)
    }

    /// Сохранённые post-call артефакты выбранной встречи.
    pub fn list_artifacts(&self, meeting_id: String) -> Vec<FfiArtifact> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, |store| store.list_artifacts(&meeting_id))
            .unwrap_or_default()
            .into_iter()
            .map(artifact_to_ffi)
            .collect()
    }

    /// Пересобрать финальный транскрипт из сохранённых captions.
    pub fn assemble_final_now(&self, meeting_id: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        write_store(&mut guard, |store| {
            assemble_and_store_final(store, &meeting_id).map(|_| ())
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default()
    }

    /// Сгенерировать и сохранить локальный артефакт из final transcript.
    pub fn generate_artifact(
        &self,
        meeting_id: String,
        kind: FfiArtifactKind,
    ) -> FfiGenerateArtifactResult {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let final_transcript =
            match read_store(&guard, |store| store.get_final_transcript(&meeting_id)) {
                Ok(Some(transcript)) => transcript,
                Ok(None) => {
                    return FfiGenerateArtifactResult {
                        artifact: empty_artifact(),
                        error: "final transcript not found".to_string(),
                    };
                }
                Err(error) => {
                    return FfiGenerateArtifactResult {
                        artifact: empty_artifact(),
                        error: error.to_string(),
                    };
                }
            };
        let domain_kind = match kind {
            FfiArtifactKind::Brief => ArtifactKind::Brief,
            FfiArtifactKind::FollowUp => ArtifactKind::FollowUp,
        };
        let primary_language = guard.language_policy.primary;
        let generated_at_ms = now_ms();
        let body = match domain_kind {
            ArtifactKind::Brief => render_brief(&final_transcript.body_markdown, primary_language),
            ArtifactKind::FollowUp => {
                let started_at_ms = read_store(&guard, |store| store.list_meeting_summaries())
                    .ok()
                    .and_then(|meetings| {
                        meetings
                            .into_iter()
                            .find(|meeting| meeting.id == meeting_id)
                            .map(|meeting| meeting.started_at_ms)
                    })
                    .unwrap_or(generated_at_ms);
                render_follow_up(
                    &final_transcript.body_markdown,
                    primary_language,
                    &utc_date_label(started_at_ms),
                )
            }
        };
        let mut artifact = make_artifact(&meeting_id, domain_kind, &body, generated_at_ms);
        artifact.id = Uuid::new_v4().to_string();
        let insert_result = write_store(&mut guard, |store| store.insert_artifact(&artifact));
        match insert_result {
            Ok(()) => FfiGenerateArtifactResult {
                artifact: artifact_to_ffi(artifact),
                error: String::new(),
            },
            Err(error) => FfiGenerateArtifactResult {
                artifact: empty_artifact(),
                error: error.to_string(),
            },
        }
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
                let terms = match store.list_glossary_terms() {
                    Ok(terms) => active_terms(&terms, Some(&session_id)),
                    Err(error) => return error.to_string(),
                };
                let glossary = GlossaryEngine::from_terms(terms);
                let policy = guard.language_policy.clone();
                let mut pipeline = LiveCaptionPipeline::from_data_root(&root, policy);
                if pipeline.backend() == SttBackendKind::Whisper {
                    pipeline.set_initial_prompt(&glossary.build_whisper_prompt(800));
                }
                guard.stt_backend = match pipeline.backend() {
                    SttBackendKind::Mock => "mock".to_string(),
                    SttBackendKind::Whisper => "whisper".to_string(),
                };
                guard.store = Some(store);
                guard.recording_session_id = Some(session_id);
                guard.stt = Some(pipeline);
                guard.glossary = glossary;
                guard.pending_live_captions.clear();
                guard.pending_translations.clear();
                guard.host_translation_queue.clear();
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
                for mut event in events {
                    event.text = guard.glossary.normalize_caption(&event.text);
                    if let Some(store) = guard.store.as_mut() {
                        let _ = store.append_caption(&sid, &event, now_ms());
                    }
                    enqueue_caption(&mut guard, event);
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
            for mut event in flushed {
                event.text = guard.glossary.normalize_caption(&event.text);
                if let Some(store) = guard.store.as_mut() {
                    let _ = store.append_caption(&sid, &event, now_ms());
                }
                enqueue_caption(&mut guard, event);
            }
            if let Some(store) = guard.store.as_mut() {
                let _ = assemble_and_store_final(store, &sid);
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
    fn utc_date_label_formats_calendar_date() {
        assert_eq!(utc_date_label(0), "1970-01-01");
        assert_eq!(utc_date_label(1_785_628_800_000), "2026-08-02");
    }

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
    fn session_language_drives_english_demo() {
        let core = MeetingCore::new();
        assert!(core.set_session_language("en".into()).is_empty());
        assert_eq!(core.session_language(), "en");
        core.start_demo();
        let events = core.drain_events();
        assert_eq!(events[0].text, "Welcome");
        core.stop();
    }

    #[test]
    fn live_translation_is_separate_from_captions() {
        let core = MeetingCore::new();
        assert!(core.set_live_translation(true, "en".into()).is_empty());
        assert!(core.set_translation_backend("stub".into(), String::new()).is_empty());
        core.start_demo();
        let captions = core.drain_events();
        assert_eq!(captions[0].text, "Добро пожаловать");
        let translations = core.drain_live_translations();
        assert_eq!(translations[0].text, "Welcome");
        core.stop();
    }

    #[test]
    fn apple_backend_uses_host_bridge_queue() {
        let core = MeetingCore::new();
        core.set_host_translation_available(true);
        assert!(core.set_live_translation(true, "en".into()).is_empty());
        assert!(core.set_translation_backend("apple".into(), String::new()).is_empty());
        assert_eq!(core.effective_translation_backend(), "apple");
        core.start_demo();
        let _ = core.drain_events();
        let reqs = core.drain_host_translation_requests();
        assert!(!reqs.is_empty());
        assert_eq!(reqs[0].text, "Добро пожаловать");
        assert!(
            core.complete_host_translation(reqs[0].id.clone(), "Welcome".into())
                .is_empty()
        );
        let translations = core.drain_live_translations();
        assert_eq!(translations[0].text, "Welcome");
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
    fn upsert_rejects_empty_surface_and_canonical() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-glossary-validation-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let make_term = |surface: &str, canonical: &str| FfiGlossaryTerm {
            id: format!("{surface}-{canonical}"),
            surface: surface.into(),
            canonical: canonical.into(),
            language: "ru".into(),
            scope: FfiGlossaryScope::Global,
            meeting_id: String::new(),
        };

        assert!(
            !core
                .upsert_glossary_term(make_term(" ", "UniFFI"))
                .is_empty()
        );
        assert!(
            !core
                .upsert_glossary_term(make_term("униффи", "\n"))
                .is_empty()
        );
        assert!(core.list_glossary_terms().is_empty());
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

    #[test]
    fn glossary_normalizes_live_mock_captions() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-glossary-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.upsert_glossary_term(FfiGlossaryTerm {
                id: "term-uniffi".into(),
                surface: "униффи".into(),
                canonical: "UniFFI".into(),
                language: "ru".into(),
                scope: FfiGlossaryScope::Global,
                meeting_id: String::new(),
            })
            .is_empty()
        );
        assert!(core.start_recording("glossary-1".into()).is_empty());

        let mut loud = Vec::new();
        for _ in 0..4000 {
            loud.extend_from_slice(&3000_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, loud, 16_000, 0)
                .is_empty()
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
            finals.iter().any(|event| event.text.contains("UniFFI")),
            "expected normalized UniFFI token, got {finals:?}"
        );
        core.stop_recording();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn glossary_upsert_during_recording_updates_live_normalization() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-glossary-reload-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(core.start_recording("glossary-live".into()).is_empty());
        assert!(
            core.upsert_glossary_term(FfiGlossaryTerm {
                id: "term-live-uniffi".into(),
                surface: "униффи".into(),
                canonical: "UniFFI".into(),
                language: "ru".into(),
                scope: FfiGlossaryScope::Global,
                meeting_id: String::new(),
            })
            .is_empty()
        );

        let mut loud = Vec::new();
        for _ in 0..4000 {
            loud.extend_from_slice(&3000_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, loud, 16_000, 0)
                .is_empty()
        );
        let mut silence = Vec::new();
        for _ in 0..6000 {
            silence.extend_from_slice(&0_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, silence, 16_000, 500)
                .is_empty()
        );

        let captions = core.drain_live_captions();
        assert!(
            captions.iter().any(|event| event.text.contains("UniFFI")),
            "expected live glossary reload, got {captions:?}"
        );
        core.stop_recording();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stop_recording_persists_final_and_exposes_postcall_api() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-postcall-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let meeting_id = "postcall-1".to_string();
        assert!(core.start_recording(meeting_id.clone()).is_empty());

        let mut loud = Vec::new();
        for _ in 0..4000 {
            loud.extend_from_slice(&3000_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, loud, 16_000, 0)
                .is_empty()
        );
        let mut silence = Vec::new();
        for _ in 0..6000 {
            silence.extend_from_slice(&0_i16.to_le_bytes());
        }
        assert!(
            core.ingest_audio_chunk(FfiAudioChannel::Mic, silence, 16_000, 500)
                .is_empty()
        );

        core.stop_recording();

        let final_transcript = core.get_final_transcript(meeting_id.clone());
        assert_eq!(final_transcript.meeting_id, meeting_id);
        assert!(!final_transcript.body_markdown.is_empty());
        assert!(!core.list_captions(meeting_id.clone()).is_empty());
        let meetings = core.list_meetings();
        assert_eq!(meetings.len(), 1);
        assert!(meetings[0].has_final);

        let result = core.generate_artifact(meeting_id.clone(), FfiArtifactKind::Brief);
        assert!(result.error.is_empty(), "{}", result.error);
        assert!(result.artifact.body_markdown.contains("# Brief"));

        let follow_up = core.generate_artifact(meeting_id.clone(), FfiArtifactKind::FollowUp);
        assert!(follow_up.error.is_empty(), "{}", follow_up.error);
        let subject = follow_up
            .artifact
            .body_markdown
            .lines()
            .next()
            .expect("follow-up должен содержать subject");
        assert!(
            subject.chars().any(|character| character.is_ascii_digit()),
            "subject должен содержать дату: {subject}"
        );
        assert_eq!(core.list_artifacts(meeting_id.clone()).len(), 2);
        assert!(core.assemble_final_now(meeting_id).is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generate_artifact_reports_missing_final() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-postcall-error-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        let result = core.generate_artifact("missing".into(), FfiArtifactKind::FollowUp);

        assert!(!result.error.is_empty());
        assert!(result.artifact.id.is_empty());
        assert!(core.list_artifacts("missing".into()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
