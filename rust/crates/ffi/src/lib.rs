//! UniFFI facade MeetingRaft: Swift ↔ session + recording + live STT.

uniffi::setup_scaffolding!();

use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use domain::{
    Artifact, ArtifactKind, AudioChannel, CaptionPhase, FinalTranscript, GlossaryScope,
    GlossaryTerm, LanguagePolicy, MeetingSummary, SearchHit, SessionState, Speaker, SpeechLanguage,
};
use glossary::{GlossaryEngine, active_terms, parse_csv};
use postcall::{
    LlmClient, LlmError, OllamaNativeClient, OpenAiCompatLlmClient, assemble_final, brief_prompts,
    follow_up_prompts, make_artifact, render_brief, render_follow_up,
};
mod rebuild;

use postcall::{RebuildJobs, ThreadSpawner, diff_words};
use session::{ChannelMixer, MeetingSession};
use storage::{AudioManifestError, AudioManifestStore};
use stt::{
    LiveCaptionPipeline, SttBackendKind, models_dir, pcm_bytes_to_i16, resolve_whisper_model,
};
use sync::{CreateJobRequest, JobKind, SyncClient, wait_for_job_artifact};
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
    /// `mic` — говорит пользователь, `system` — собеседники (ADR-009).
    pub channel: String,
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

/// Ручная метка спикера встречи для Swift.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSpeaker {
    pub id: String,
    pub meeting_id: String,
    pub display_name: String,
    pub sort_index: i64,
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
    /// Пустая строка — названия нет; подстановку делает Swift.
    pub title: String,
    pub started_at_ms: u64,
    /// 0 — встреча ещё не завершена.
    pub ended_at_ms: u64,
    pub has_final: bool,
    pub artifact_count: u64,
}

/// Кусок дифференциации Live против Final для Swift.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDiffSpan {
    /// `equal` | `removed` | `added`.
    pub op: String,
    pub text: String,
}

/// Прогресс фонового пересбора Final для Swift.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRebuildProgress {
    pub job_id: String,
    pub meeting_id: String,
    /// `queued` | `running` | `succeeded` | `failed` | `cancelled`;
    /// пусто — задачи с таким id нет.
    pub state: String,
    pub done: u32,
    pub total: u32,
    pub error: String,
    /// Что фактически отработало: источник для provenance в UI.
    pub note: String,
}

/// Совпадение поиска по материалам встреч.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSearchHit {
    pub meeting_id: String,
    /// `caption` | `final` | `artifact` — куда вести из результата.
    pub kind: String,
    pub ref_id: String,
    pub snippet: String,
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

/// Статус backend job (ADR-007).
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBackendJob {
    pub id: String,
    pub meeting_id: String,
    pub kind: String,
    pub status: String,
    pub error: String,
    pub artifact_ids: Vec<String>,
}

/// Артефакт с backend (отдельно от локального FfiArtifact).
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBackendArtifact {
    pub id: String,
    pub kind: String,
    pub body_markdown: String,
    pub created_at: String,
    pub error: String,
}

/// Ссылка на LLM-модель из backend-каталога.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiLlmModelRef {
    pub provider_id: String,
    pub model: String,
    pub display_name: String,
}

struct MeetingCoreInner {
    session: MeetingSession,
    started_at: Option<Instant>,
    store: Option<AudioManifestStore>,
    recording_session_id: Option<String>,
    data_root: PathBuf,
    stt: Option<LiveCaptionPipeline>,
    stt_backend: String,
    /// Выравнивание mic и system перед подачей в STT (ADR-009).
    mixer: ChannelMixer,
    glossary: GlossaryEngine,
    pending_live_captions: VecDeque<FfiCaptionEvent>,
    /// Язык распознавания (captions); не путать с целевым языком перевода.
    language_policy: LanguagePolicy,
    translation_policy: TranslationPolicy,
    /// Swift зарегистрировал Apple / host bridge.
    host_translation_available: bool,
    host_translation_queue: HostPendingQueue,
    pending_translations: VecDeque<FfiCaptionEvent>,
    sync_client: SyncClient,
    llm_engine: String,
    llm_model_id: String,
    llm_base_url: String,
    llm_provider_id: String,
    preferred_whisper_model: String,
    /// Модель для post-call прохода; отдельная от live, скачивается по
    /// требованию при первом пересборе.
    post_call_whisper_model: String,
}

/// Фасад сессии для macOS shell.
#[derive(uniffi::Object)]
pub struct MeetingCore {
    inner: Mutex<MeetingCoreInner>,
    /// Реестр фоновых пересборов. Вне `inner` намеренно: у него своя
    /// синхронизация, и проход не должен держать мьютекс ядра минутами.
    jobs: RebuildJobs,
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

/// Нормализует id модели Whisper; неизвестные → `"auto"`.
fn normalize_whisper_model_id(model_id: &str) -> String {
    match model_id {
        "auto" | "base" | "small" | "large-v3-turbo" => model_id.to_owned(),
        _ => "auto".to_owned(),
    }
}

fn to_ffi(event: domain::CaptionEvent) -> FfiCaptionEvent {
    FfiCaptionEvent {
        id: event.id,
        text: event.text,
        phase: match event.phase {
            CaptionPhase::Partial => FfiCaptionPhase::Partial,
            CaptionPhase::Final => FfiCaptionPhase::Final,
        },
        channel: event.channel.code().to_string(),
    }
}

/// Частота дискретизации живого пути (ADR-005 / AudioChunkPipeline).
const SAMPLE_RATE_HZ: u32 = 16_000;

/// Прогнать выровненные кадры через STT.
fn transcribe_frames(
    inner: &mut MeetingCoreInner,
    frames: &[session::MixedFrame],
    sample_rate: u32,
) -> Vec<domain::CaptionEvent> {
    let Some(pipeline) = inner.stt.as_mut() else {
        return Vec::new();
    };
    frames
        .iter()
        .flat_map(|frame| pipeline.push_frame(&frame.pcm, sample_rate, frame.dominant))
        .collect()
}

/// Нормализовать глоссарием, сохранить и отдать в очередь UI.
fn store_and_enqueue(inner: &mut MeetingCoreInner, events: Vec<domain::CaptionEvent>) {
    let Some(session_id) = inner.recording_session_id.clone() else {
        return;
    };
    for mut event in events {
        event.text = inner.glossary.normalize_caption(&event.text);
        if let Some(store) = inner.store.as_mut() {
            let _ = store.append_caption(&session_id, &event, now_ms());
        }
        enqueue_caption(inner, event);
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
    let effective = resolve_effective(&inner.translation_policy, inner.host_translation_available);
    match effective {
        EffectiveBackend::Off => {}
        EffectiveBackend::AppleHost => {
            inner.host_translation_queue.enqueue(
                &event.text,
                source,
                target,
                event.phase,
                event.channel,
            );
        }
        other => match translate_now(
            other,
            &inner.translation_policy,
            &event.text,
            source,
            target,
        ) {
            Ok(text) => {
                let mut translated =
                    domain::CaptionEvent::new(Uuid::new_v4().to_string(), text, event.phase);
                translated.channel = event.channel;
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
        title: summary.title,
        started_at_ms: summary.started_at_ms,
        ended_at_ms: summary.ended_at_ms.unwrap_or(0),
        has_final: summary.has_final,
        artifact_count: summary.artifact_count,
    }
}

fn search_hit_to_ffi(hit: SearchHit) -> FfiSearchHit {
    FfiSearchHit {
        meeting_id: hit.meeting_id,
        kind: hit.kind.code().to_string(),
        ref_id: hit.ref_id,
        snippet: hit.snippet,
    }
}

fn speaker_to_ffi(speaker: Speaker) -> FfiSpeaker {
    FfiSpeaker {
        id: speaker.id,
        meeting_id: speaker.meeting_id,
        display_name: speaker.display_name,
        sort_index: speaker.sort_index,
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

fn normalize_llm_engine(code: &str) -> &str {
    match code {
        "backend" => "backend",
        "ollama" => "ollama",
        "openai_compat" => "openai_compat",
        _ => "builtin_templates",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CoreError {
    Http { status: u16, body: String },
    Empty,
    Transport(String),
    NotConfigured,
}

impl From<LlmError> for CoreError {
    fn from(error: LlmError) -> Self {
        match error {
            LlmError::Http { status, body } => Self::Http { status, body },
            LlmError::EmptyResponse => Self::Empty,
            LlmError::Transport(message) => Self::Transport(message),
            LlmError::NotConfigured => Self::NotConfigured,
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { status, body } => {
                write!(formatter, "LLM-провайдер вернул HTTP {status}: {body}")
            }
            Self::Empty => formatter.write_str("LLM-провайдер вернул пустой ответ"),
            Self::Transport(message) => write!(formatter, "Ошибка транспорта LLM: {message}"),
            Self::NotConfigured => formatter.write_str("LLM-клиент не настроен"),
        }
    }
}

fn store_generated_artifact(
    inner: &mut MeetingCoreInner,
    meeting_id: &str,
    kind: ArtifactKind,
    body: &str,
    generated_at_ms: u64,
    template_id: Option<&str>,
) -> FfiGenerateArtifactResult {
    let mut artifact = make_artifact(meeting_id, kind, body, generated_at_ms);
    artifact.id = Uuid::new_v4().to_string();
    if let Some(template_id) = template_id {
        artifact.template_id = template_id.to_owned();
    }
    match write_store(inner, |store| store.insert_artifact(&artifact)) {
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

fn empty_backend_job(error: String) -> FfiBackendJob {
    FfiBackendJob {
        id: String::new(),
        meeting_id: String::new(),
        kind: String::new(),
        status: String::new(),
        error,
        artifact_ids: Vec::new(),
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
    let version = store.next_final_version(meeting_id)?;
    let transcript = assemble_final(
        meeting_id,
        &captions,
        |text| glossary.normalize_caption(text),
        now_ms(),
        version,
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
                mixer: ChannelMixer::new(),
                glossary: GlossaryEngine::from_terms(Vec::new()),
                pending_live_captions: VecDeque::new(),
                language_policy: LanguagePolicy::default_v1(),
                translation_policy: TranslationPolicy::disabled(),
                host_translation_available: false,
                host_translation_queue: HostPendingQueue::default(),
                pending_translations: VecDeque::new(),
                sync_client: SyncClient::new("", ""),
                llm_engine: "builtin_templates".to_string(),
                llm_model_id: String::new(),
                llm_base_url: String::new(),
                llm_provider_id: String::new(),
                preferred_whisper_model: "auto".to_string(),
                post_call_whisper_model: "large-v3-turbo".to_string(),
            }),
            jobs: RebuildJobs::new(Box::new(ThreadSpawner)),
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
        resolve_effective(&guard.translation_policy, guard.host_translation_available)
            .code()
            .to_owned()
    }

    /// ADR-007: base URL + bearer token (в памяти процесса).
    pub fn set_api_config(&self, base_url: String, token: String) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.sync_client = SyncClient::new(base_url, token);
    }

    /// Выбрать генератор post-call артефактов; неизвестные значения используют builtin.
    /// `provider_id` — id backend-провайдера; для локальных движков передавать пустую строку.
    pub fn set_llm_config(
        &self,
        engine_code: String,
        model_id: String,
        base_url: String,
        provider_id: String,
    ) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.llm_engine = normalize_llm_engine(&engine_code).to_owned();
        guard.llm_model_id = model_id;
        guard.llm_base_url = base_url.trim().trim_end_matches('/').to_owned();
        guard.llm_provider_id = provider_id;
    }

    /// Каталог LLM с backend; при ошибке sync / не настроенном API — пустой список.
    pub fn list_backend_llm_models(&self) -> Vec<FfiLlmModelRef> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        match guard.sync_client.list_models() {
            Ok(models) => models
                .into_iter()
                .map(|model| FfiLlmModelRef {
                    provider_id: model.provider_id,
                    model: model.model,
                    display_name: model.display_name,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn api_base_url(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        // SyncClient doesn't expose base_url — store separately or add getter.
        // Use health probe path via clone fields: add accessors on SyncClient.
        guard.sync_client.base_url().to_owned()
    }

    /// Пустая строка = OK.
    pub fn test_api_connection(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        match guard.sync_client.health() {
            Ok(()) => String::new(),
            Err(error) => error.to_string(),
        }
    }

    pub fn submit_backend_job(&self, meeting_id: String, kind_code: String) -> FfiBackendJob {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let Some(kind) = JobKind::from_code(&kind_code) else {
            return empty_backend_job(format!("unsupported job kind: {kind_code}"));
        };
        let request = CreateJobRequest {
            meeting_id,
            kind,
            primary_language: guard.language_policy.primary.code().to_owned(),
            allowed_languages: guard
                .language_policy
                .allowed
                .iter()
                .map(|l| l.code().to_owned())
                .collect(),
            payload: None,
        };
        match guard.sync_client.create_job(&request) {
            Ok(job) => FfiBackendJob {
                id: job.id,
                meeting_id: job.meeting_id,
                kind: job.kind.as_str().to_owned(),
                status: job.status.as_str().to_owned(),
                error: job.error.unwrap_or_default(),
                artifact_ids: job.artifact_ids,
            },
            Err(error) => empty_backend_job(error.to_string()),
        }
    }

    pub fn get_backend_job(&self, job_id: String) -> FfiBackendJob {
        let guard = self.inner.lock().expect("meeting core poisoned");
        match guard.sync_client.get_job(&job_id) {
            Ok(job) => FfiBackendJob {
                id: job.id,
                meeting_id: job.meeting_id,
                kind: job.kind.as_str().to_owned(),
                status: job.status.as_str().to_owned(),
                error: job.error.unwrap_or_default(),
                artifact_ids: job.artifact_ids,
            },
            Err(error) => empty_backend_job(error.to_string()),
        }
    }

    pub fn get_backend_artifact(&self, artifact_id: String) -> FfiBackendArtifact {
        let guard = self.inner.lock().expect("meeting core poisoned");
        match guard.sync_client.get_artifact(&artifact_id) {
            Ok(artifact) => FfiBackendArtifact {
                id: artifact.id,
                kind: artifact.kind.as_str().to_owned(),
                body_markdown: artifact.body_markdown,
                created_at: artifact.created_at,
                error: String::new(),
            },
            Err(error) => FfiBackendArtifact {
                id: String::new(),
                kind: String::new(),
                body_markdown: String::new(),
                created_at: String::new(),
                error: error.to_string(),
            },
        }
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
        let Some((phase_final, channel)) = guard.host_translation_queue.take_awaiting(&id) else {
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
            channel: channel.code().to_string(),
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

    /// Спикеры встречи в пользовательском порядке.
    pub fn list_speakers(&self, meeting_id: String) -> Vec<FfiSpeaker> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, |store| store.list_speakers(&meeting_id))
            .unwrap_or_default()
            .into_iter()
            .map(speaker_to_ffi)
            .collect()
    }

    /// Создать или обновить ручную метку спикера; пустой id генерируется здесь.
    pub fn upsert_speaker(
        &self,
        meeting_id: String,
        id: String,
        display_name: String,
        sort_index: i64,
    ) -> String {
        let speaker = Speaker {
            id: if id.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                id
            },
            meeting_id,
            display_name,
            sort_index,
        };
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        write_store(&mut guard, |store| store.upsert_speaker(&speaker))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    /// Удалить ручную метку спикера.
    pub fn delete_speaker(&self, id: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        write_store(&mut guard, |store| store.delete_speaker(&id))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
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
    /// Полнотекстовый поиск по названиям, транскриптам и артефактам.
    /// Пустой запрос возвращает пустой список, а не всю базу.
    pub fn search_meetings(&self, query: String, limit: u32) -> Vec<FfiSearchHit> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let root = guard.data_root.clone();
        drop(guard);
        let Ok(store) = AudioManifestStore::open(&root) else {
            return Vec::new();
        };
        store
            .search(&query, limit)
            .map(|hits| hits.into_iter().map(search_hit_to_ffi).collect())
            .unwrap_or_default()
    }

    /// Удалить встречу целиком; пустая строка ошибки означает успех.
    pub fn delete_meeting(&self, meeting_id: String) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        if guard.recording_session_id.as_deref() == Some(meeting_id.as_str()) {
            return "meeting is being recorded".to_string();
        }
        let root = guard.data_root.clone();
        drop(guard);
        match AudioManifestStore::open(&root) {
            Ok(mut store) => match store.delete_meeting(&meeting_id) {
                Ok(()) => String::new(),
                Err(err) => err.to_string(),
            },
            Err(err) => err.to_string(),
        }
    }

    /// Сравнить live-финалы с версией Final по словам.
    ///
    /// Считается в ядре: сравнение — доменная логика, вью только
    /// раскрашивает (`AGENTS.md`).
    pub fn diff_live_vs_final(&self, meeting_id: String, version: u32) -> Vec<FfiDiffSpan> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let root = guard.data_root.clone();
        drop(guard);
        let Ok(store) = AudioManifestStore::open(&root) else {
            return Vec::new();
        };
        let live = store
            .list_captions(&meeting_id)
            .map(|captions| {
                captions
                    .iter()
                    .filter(|caption| caption.phase == CaptionPhase::Final)
                    .map(|caption| caption.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let final_text = store
            .get_final_transcript_version(&meeting_id, version)
            .ok()
            .flatten()
            .map(|transcript| transcript.body_markdown)
            .unwrap_or_default();

        diff_words(&live, &final_text)
            .into_iter()
            .map(|span| FfiDiffSpan {
                op: span.op.code().to_string(),
                text: span.text,
            })
            .collect()
    }

    /// Модель post-call прохода (`large-v3-turbo` по умолчанию).
    pub fn set_post_call_whisper_model(&self, model_id: String) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.post_call_whisper_model = model_id;
    }

    /// Запустить пересбор Final в фоне; возвращает id задачи.
    ///
    /// Повторный вызов для встречи с идущим проходом отдаёт тот же id:
    /// два прохода, пишущие сегменты одной версии, — это гонка.
    pub fn start_final_rebuild(&self, meeting_id: String) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let params = rebuild::RebuildParams {
            data_root: guard.data_root.clone(),
            meeting_id: meeting_id.clone(),
            policy: guard.language_policy.clone(),
            post_call_model: guard.post_call_whisper_model.clone(),
            llm_engine: normalize_llm_engine(&guard.llm_engine).to_owned(),
            llm_base_url: guard.llm_base_url.clone(),
            llm_model_id: guard.llm_model_id.clone(),
        };
        drop(guard);

        let job_id = Uuid::new_v4().to_string();
        self.jobs.start(job_id, meeting_id, move |handle| {
            rebuild::run_rebuild(params, handle)
        })
    }

    /// Состояние прохода; пустой `state` — задачи с таким id нет.
    pub fn final_rebuild_progress(&self, job_id: String) -> FfiRebuildProgress {
        match self.jobs.progress(&job_id) {
            Some(progress) => FfiRebuildProgress {
                job_id: progress.job_id,
                meeting_id: progress.meeting_id,
                state: progress.state.code().to_string(),
                done: progress.done,
                total: progress.total,
                error: progress.error,
                note: progress.note,
            },
            None => FfiRebuildProgress {
                job_id,
                meeting_id: String::new(),
                state: String::new(),
                done: 0,
                total: 0,
                error: String::new(),
                note: String::new(),
            },
        }
    }

    /// Попросить проход остановиться; он увидит это между единицами работы.
    pub fn cancel_final_rebuild(&self, job_id: String) {
        self.jobs.cancel(&job_id);
    }

    /// Идущий пересбор этой встречи, если он есть.
    pub fn active_final_rebuild(&self, meeting_id: String) -> String {
        self.jobs.active_job_for(&meeting_id).unwrap_or_default()
    }

    /// Переименовать встречу; пустая строка ошибки означает успех.
    pub fn rename_meeting(&self, meeting_id: String, title: String) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let root = guard.data_root.clone();
        drop(guard);
        match AudioManifestStore::open(&root) {
            Ok(mut store) => match store.set_meeting_title(&meeting_id, &title) {
                Ok(()) => String::new(),
                Err(err) => err.to_string(),
            },
            Err(err) => err.to_string(),
        }
    }

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

    /// Все версии финального транскрипта (новые первыми).
    pub fn list_final_transcripts(&self, meeting_id: String) -> Vec<FfiFinalTranscript> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, |store| store.list_final_transcripts(&meeting_id))
            .unwrap_or_default()
            .into_iter()
            .map(final_transcript_to_ffi)
            .collect()
    }

    /// Финальный транскрипт конкретной версии или пустой DTO.
    pub fn get_final_transcript_version(
        &self,
        meeting_id: String,
        version: u32,
    ) -> FfiFinalTranscript {
        let guard = self.inner.lock().expect("meeting core poisoned");
        read_store(&guard, |store| {
            store.get_final_transcript_version(&meeting_id, version)
        })
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
        let engine = normalize_llm_engine(&guard.llm_engine).to_owned();
        let generated_at_ms = now_ms();
        if engine == "backend" {
            let job_kind = match domain_kind {
                ArtifactKind::Brief => JobKind::Brief,
                ArtifactKind::FollowUp => JobKind::FollowUp,
            };
            let (system, user) = match domain_kind {
                ArtifactKind::Brief => brief_prompts(
                    &final_transcript.body_markdown,
                    guard.language_policy.primary,
                ),
                ArtifactKind::FollowUp => follow_up_prompts(
                    &final_transcript.body_markdown,
                    guard.language_policy.primary,
                ),
            };
            let request = CreateJobRequest {
                meeting_id: meeting_id.clone(),
                kind: job_kind,
                primary_language: guard.language_policy.primary.code().to_owned(),
                allowed_languages: guard
                    .language_policy
                    .allowed
                    .iter()
                    .map(|language| language.code().to_owned())
                    .collect(),
                payload: Some(serde_json::json!({
                    "provider_id": guard.llm_provider_id,
                    "model": guard.llm_model_id,
                    "system": system,
                    "user": user,
                })),
            };
            let client = guard.sync_client.clone();
            drop(guard);
            let backend_artifact =
                match wait_for_job_artifact(&client, &request, 20, Duration::from_millis(250)) {
                    Ok(artifact) => artifact,
                    Err(error) => {
                        return FfiGenerateArtifactResult {
                            artifact: empty_artifact(),
                            error: error.to_string(),
                        };
                    }
                };
            let template_id = match domain_kind {
                ArtifactKind::Brief => "backend.brief",
                ArtifactKind::FollowUp => "backend.follow_up",
            };
            let mut guard = self.inner.lock().expect("meeting core poisoned");
            return store_generated_artifact(
                &mut guard,
                &meeting_id,
                domain_kind,
                &backend_artifact.body_markdown,
                generated_at_ms,
                Some(template_id),
            );
        }
        if matches!(engine.as_str(), "ollama" | "openai_compat") {
            let base_url = guard.llm_base_url.clone();
            let model_id = guard.llm_model_id.clone();
            let primary_language = guard.language_policy.primary;
            let final_body = final_transcript.body_markdown.clone();
            drop(guard);

            let (system, user) = match domain_kind {
                ArtifactKind::Brief => brief_prompts(&final_body, primary_language),
                ArtifactKind::FollowUp => follow_up_prompts(&final_body, primary_language),
            };
            let completion = match engine.as_str() {
                "ollama" => OllamaNativeClient::new(base_url, model_id).complete(&system, &user),
                "openai_compat" => {
                    OpenAiCompatLlmClient::new(base_url, model_id).complete(&system, &user)
                }
                _ => unreachable!("локальная LLM-ветка проверена выше"),
            };
            let body = match completion.map_err(CoreError::from) {
                Ok(body) => body,
                Err(error) => {
                    return FfiGenerateArtifactResult {
                        artifact: empty_artifact(),
                        error: error.to_string(),
                    };
                }
            };
            let template_id = match (engine.as_str(), domain_kind) {
                ("ollama", ArtifactKind::Brief) => "ollama.brief",
                ("ollama", ArtifactKind::FollowUp) => "ollama.follow_up",
                ("openai_compat", ArtifactKind::Brief) => "openai.brief",
                ("openai_compat", ArtifactKind::FollowUp) => "openai.follow_up",
                _ => unreachable!("локальная LLM-ветка проверена выше"),
            };
            let mut guard = self.inner.lock().expect("meeting core poisoned");
            return store_generated_artifact(
                &mut guard,
                &meeting_id,
                domain_kind,
                &body,
                generated_at_ms,
                Some(template_id),
            );
        }

        let primary_language = guard.language_policy.primary;
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
        store_generated_artifact(
            &mut guard,
            &meeting_id,
            domain_kind,
            &body,
            generated_at_ms,
            None,
        )
    }

    /// Recording + live STT (Whisper если модель есть и feature включён, иначе Mock).
    ///
    /// `title` формирует Swift: формат даты локале-зависим и не должен
    /// уезжать в ядро. Пустая строка допустима.
    pub fn start_recording(&self, session_id: String, title: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let root = guard.data_root.clone();
        match AudioManifestStore::open(&root) {
            Ok(mut store) => {
                if let Err(err) = store.begin_session(&session_id, now_ms(), &title) {
                    return err.to_string();
                }
                let terms = match store.list_glossary_terms() {
                    Ok(terms) => active_terms(&terms, Some(&session_id)),
                    Err(error) => return error.to_string(),
                };
                let glossary = GlossaryEngine::from_terms(terms);
                let policy = guard.language_policy.clone();
                let preferred = guard.preferred_whisper_model.clone();
                let mut pipeline =
                    LiveCaptionPipeline::from_data_root(&root, policy, Some(&preferred));
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
                guard.mixer.reset();
                self.jobs.set_recording(true);
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
        resolve_whisper_model(&guard.data_root, Some(&guard.preferred_whisper_model))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Предпочитаемая модель Whisper: `auto` | `base` | `small` | `large-v3-turbo`.
    pub fn set_preferred_whisper_model(&self, model_id: String) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.preferred_whisper_model = normalize_whisper_model_id(&model_id);
    }

    pub fn preferred_whisper_model(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.preferred_whisper_model.clone()
    }

    /// Имена `ggml-*.bin` в `{data_root}/models`, отсортированные.
    pub fn list_local_whisper_models(&self) -> Vec<String> {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let dir = models_dir(&guard.data_root);
        if !dir.is_dir() {
            return Vec::new();
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let name = path.file_name()?.to_str()?;
                if path.extension().and_then(|ext| ext.to_str()) == Some("bin")
                    && name.starts_with("ggml-")
                {
                    Some(name.to_owned())
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        names
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

        // Оба канала идут в микшер; STT получает выровненный микс, а канал
        // говорящего приезжает отдельным полем события (ADR-009).
        let samples = pcm_bytes_to_i16(&pcm);
        guard.mixer.push(domain_channel, &samples, timestamp_ms);
        let frames = guard.mixer.drain();
        let events = transcribe_frames(&mut guard, &frames, sample_rate);
        store_and_enqueue(&mut guard, events);
        String::new()
    }

    /// Ждать ли системный канал: Swift сообщает это после старта tap.
    /// Пока tap не запущен, микшер не должен простаивать допуск впустую.
    pub fn set_system_audio_expected(&self, expected: bool) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.mixer.set_system_expected(expected);
    }

    pub fn stop_recording(&self) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        let sid = guard.recording_session_id.clone();
        // Хвост микшера не должен ждать допуска — иначе теряются последние
        // слоты речи; после него добираем хвост самого движка.
        let pending = guard.mixer.flush();
        let mut flushed = transcribe_frames(&mut guard, &pending, SAMPLE_RATE_HZ);
        flushed.extend(guard.stt.as_mut().map(|p| p.flush()).unwrap_or_default());
        guard.mixer.reset();
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
            let _ = store.end_session(now_ms());
        }
        guard.store = None;
        guard.recording_session_id = None;
        guard.stt = None;
        guard.stt_backend = "idle".to_string();
        self.jobs.set_recording(false);
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
    use mockito::{Matcher, Server};
    use std::thread;
    use std::time::Duration;

    fn seed_final_transcript(root: &std::path::Path, meeting_id: &str) {
        let mut store = AudioManifestStore::open(root).expect("test store должен открыться");
        store
            .upsert_final_transcript(&FinalTranscript {
                meeting_id: meeting_id.to_owned(),
                version: 1,
                body_markdown: "Обсудили backend-генерацию.".into(),
                created_at_ms: 1_785_628_800_000,
            })
            .expect("final transcript должен сохраниться");
    }

    fn seed_final_captions(root: &std::path::Path, meeting_id: &str) {
        let mut store = AudioManifestStore::open(root).expect("test store должен открыться");
        store
            .append_caption(
                meeting_id,
                &domain::CaptionEvent::new(
                    "c1".into(),
                    "Первая финальная фраза".into(),
                    CaptionPhase::Final,
                ),
                10,
            )
            .expect("caption должен сохраниться");
        store
            .append_caption(
                meeting_id,
                &domain::CaptionEvent::new(
                    "c2".into(),
                    "Вторая финальная фраза".into(),
                    CaptionPhase::Final,
                ),
                20,
            )
            .expect("caption должен сохраниться");
    }

    /// Сквозной проход: аудио → распознавание → слияние → сегменты.
    ///
    /// Идёт на MockBatchTranscriber, поэтому проверяется весь путь
    /// post-call, а не только его края.
    #[test]
    fn final_rebuild_writes_segments_and_reports_provenance() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-rebuild-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-rebuild".to_string();
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store
                .begin_session(&meeting_id, 1, "Тест")
                .expect("session");
            let loud: Vec<u8> = (0..16_000)
                .flat_map(|i| if i % 2 == 0 { 3000_i16 } else { -3000 }.to_le_bytes())
                .collect();
            store
                .append_chunk(AudioChannel::Mic, &loud, 16_000, 0)
                .expect("chunk");
            store.end_session(2_000).expect("end");
        }

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let job_id = core.start_final_rebuild(meeting_id.clone());
        assert!(!job_id.is_empty());

        let mut progress = core.final_rebuild_progress(job_id.clone());
        for _ in 0..300 {
            if matches!(
                progress.state.as_str(),
                "succeeded" | "failed" | "cancelled"
            ) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            progress = core.final_rebuild_progress(job_id.clone());
        }

        assert_eq!(progress.state, "succeeded", "{}", progress.error);
        assert_eq!((progress.done, progress.total), (100, 100));
        assert!(progress.note.contains("re-ASR"), "note: {}", progress.note);
        // Полировки не было — NullLlmClient; provenance обязан это сказать.
        assert!(
            !progress.note.contains("+ LLM polish"),
            "note не должен обещать полировку: {}",
            progress.note
        );

        let store = AudioManifestStore::open(&root).expect("store");
        let version = store.next_final_version(&meeting_id).expect("version") - 1;
        let segments = store
            .list_final_segments(&meeting_id, version)
            .expect("segments");
        assert!(!segments.is_empty(), "сегменты должны быть записаны");
        assert_eq!(segments[0].channel, AudioChannel::Mic);
        assert!(segments[0].speaker_id.is_empty());

        let transcript = store.get_final_transcript(&meeting_id).expect("final");
        assert!(transcript.is_some_and(|t| !t.body_markdown.is_empty()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Второй запуск по той же встрече не плодит параллельный проход.
    #[test]
    fn starting_rebuild_twice_returns_the_same_job() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-rebuild-twice-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        let first = core.start_final_rebuild("m1".into());
        let second = core.start_final_rebuild("m1".into());

        // Проход без аудио падает сразу, поэтому первый может успеть
        // завершиться; тогда второй id законно новый.
        let first_state = core.final_rebuild_progress(first.clone()).state;
        if first_state == "running" || first_state == "queued" {
            assert_eq!(first, second);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn diff_live_vs_final_marks_the_corrected_word() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-diff-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-diff".to_string();
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store
                .append_caption(
                    &meeting_id,
                    &domain::CaptionEvent::new(
                        "c1".into(),
                        "обсудили билинг".into(),
                        CaptionPhase::Final,
                    ),
                    10,
                )
                .expect("caption");
            store
                .upsert_final_transcript(&FinalTranscript {
                    meeting_id: meeting_id.clone(),
                    version: 1,
                    body_markdown: "обсудили биллинг".into(),
                    created_at_ms: 20,
                })
                .expect("final");
        }
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        let spans = core.diff_live_vs_final(meeting_id, 1);

        let ops: Vec<&str> = spans.iter().map(|span| span.op.as_str()).collect();
        assert_eq!(ops, vec!["equal", "removed", "added"], "{spans:?}");
        assert_eq!(spans[1].text, "билинг");
        assert_eq!(spans[2].text, "биллинг");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn diff_of_unknown_meeting_is_empty() {
        let root = std::env::temp_dir().join(format!("mr-ffi-diff-none-{}", now_ms()));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        assert!(core.diff_live_vs_final("nope".into(), 1).is_empty());
    }

    #[test]
    fn progress_of_unknown_job_is_empty() {
        let root = std::env::temp_dir().join(format!("mr-ffi-nojob-{}", now_ms()));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        let progress = core.final_rebuild_progress("nope".into());

        assert!(progress.state.is_empty());
        assert!(progress.meeting_id.is_empty());
    }

    #[test]
    fn rename_search_and_delete_meeting_round_trip() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-library-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let meeting_id = "m-library".to_string();
        // Строку в sessions создаёт только start_recording — без неё
        // встречи нет ни в списке, ни для переименования.
        assert!(
            core.start_recording(meeting_id.clone(), "Черновик".into())
                .is_empty()
        );
        core.stop_recording();
        seed_final_captions(&root, &meeting_id);
        assert!(core.assemble_final_now(meeting_id.clone()).is_empty());

        assert!(
            core.rename_meeting(meeting_id.clone(), "Ретро спринта".into())
                .is_empty()
        );
        let summary = core
            .list_meetings()
            .into_iter()
            .find(|item| item.id == meeting_id)
            .expect("встреча должна быть в списке");
        assert_eq!(summary.title, "Ретро спринта");

        let hits = core.search_meetings("финальная".into(), 10);
        assert!(!hits.is_empty(), "поиск должен найти финальный транскрипт");
        assert_eq!(hits[0].meeting_id, meeting_id);

        assert!(core.delete_meeting(meeting_id.clone()).is_empty());
        assert!(core.list_meetings().is_empty());
        assert!(core.search_meetings("финальная".into(), 10).is_empty());
    }

    #[test]
    fn rename_unknown_meeting_reports_error() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-rename-missing-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        assert!(!core.rename_meeting("missing".into(), "x".into()).is_empty());
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-empty-query-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.start_recording("m-empty".into(), String::new())
                .is_empty()
        );
        core.stop_recording();
        seed_final_captions(&root, "m-empty");
        assert!(core.assemble_final_now("m-empty".into()).is_empty());

        assert!(core.search_meetings("   ".into(), 10).is_empty());
    }

    #[test]
    fn assemble_final_now_increments_version() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-final-versions-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_captions(&root, "m-versions");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let meeting_id = "m-versions".to_string();

        assert!(core.assemble_final_now(meeting_id.clone()).is_empty());
        let v1 = core.get_final_transcript(meeting_id.clone());
        assert_eq!(v1.version, 1);
        assert!(core.assemble_final_now(meeting_id.clone()).is_empty());
        let latest = core.get_final_transcript(meeting_id.clone());
        assert_eq!(latest.version, 2);
        let list = core.list_final_transcripts(meeting_id.clone());
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].version, 2);
        let old = core.get_final_transcript_version(meeting_id.clone(), 1);
        assert_eq!(old.version, 1);
        assert_eq!(old.body_markdown, v1.body_markdown);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn utc_date_label_formats_calendar_date() {
        assert_eq!(utc_date_label(0), "1970-01-01");
        assert_eq!(utc_date_label(1_785_628_800_000), "2026-08-02");
    }

    fn seed_whisper_models(root: &std::path::Path, filenames: &[&str]) {
        let models = models_dir(root);
        std::fs::create_dir_all(&models).unwrap();
        for name in filenames {
            std::fs::write(models.join(name), b"x").unwrap();
        }
    }

    #[test]
    fn set_preferred_whisper_model_affects_whisper_model_path() {
        let root = std::env::temp_dir().join(format!("mr-ffi-whisper-pref-{}", now_ms()));
        seed_whisper_models(&root, &["ggml-base.bin", "ggml-large-v3-turbo.bin"]);
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert_eq!(core.preferred_whisper_model(), "auto");
        core.set_preferred_whisper_model("base".into());
        assert_eq!(core.preferred_whisper_model(), "base");
        assert!(core.whisper_model_path().ends_with("ggml-base.bin"));
        core.set_preferred_whisper_model("auto".into());
        assert!(
            core.whisper_model_path()
                .ends_with("ggml-large-v3-turbo.bin")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn set_preferred_whisper_model_normalizes_unknown_to_auto() {
        let core = MeetingCore::new();
        core.set_preferred_whisper_model("unknown-model".into());
        assert_eq!(core.preferred_whisper_model(), "auto");
    }

    #[test]
    fn list_local_whisper_models_lists_ggml_bins() {
        let root = std::env::temp_dir().join(format!("mr-ffi-whisper-list-{}", now_ms()));
        seed_whisper_models(
            &root,
            &[
                "ggml-small.bin",
                "ggml-base.bin",
                "readme.txt",
                "ggml-large-v3-turbo.bin",
            ],
        );
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert_eq!(
            core.list_local_whisper_models(),
            vec![
                "ggml-base.bin".to_owned(),
                "ggml-large-v3-turbo.bin".to_owned(),
                "ggml-small.bin".to_owned(),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_local_whisper_models_empty_when_dir_missing() {
        let root = std::env::temp_dir().join(format!("mr-ffi-whisper-missing-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&root);
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(core.list_local_whisper_models().is_empty());
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
        assert!(
            core.set_translation_backend("stub".into(), String::new())
                .is_empty()
        );
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
        assert!(
            core.set_translation_backend("apple".into(), String::new())
                .is_empty()
        );
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
        assert!(
            core.start_recording("rec-1".into(), String::new())
                .is_empty()
        );
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
    fn speakers_round_trip_via_core() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-speakers-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.upsert_speaker("m1".into(), "".into(), "Спикер 1".into(), 0)
                .is_empty()
        );
        let list = core.list_speakers("m1".into());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].display_name, "Спикер 1");
        assert!(!list[0].id.is_empty());
        assert!(
            core.upsert_speaker(
                "m1".into(),
                list[0].id.clone(),
                "Алиса".into(),
                list[0].sort_index
            )
            .is_empty()
        );
        assert_eq!(core.list_speakers("m1".into())[0].display_name, "Алиса");
        assert!(core.delete_speaker(list[0].id.clone()).is_empty());
        assert!(core.list_speakers("m1".into()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn loud_mic_produces_live_captions() {
        let root = std::env::temp_dir().join(format!("mr-ffi-stt-{}", now_ms()));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.start_recording("live-1".into(), String::new())
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
        assert!(
            core.start_recording("glossary-1".into(), String::new())
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
        assert!(
            core.start_recording("glossary-live".into(), String::new())
                .is_empty()
        );
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
        assert!(
            core.start_recording(meeting_id.clone(), String::new())
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

    #[test]
    fn list_backend_llm_models_maps_sync() {
        let mut server = Server::new();
        let _m = server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"models":[{"provider_id":"p","model":"m","display_name":"D"}]}"#)
            .create();
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-list-models-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_api_config(server.url(), "dev-token".into());

        let models = core.list_backend_llm_models();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].provider_id, "p");
        assert_eq!(models[0].model, "m");
        assert_eq!(models[0].display_name, "D");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generate_artifact_backend_uses_job_artifact() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .match_body(Matcher::Exact(
                r#"{"meeting_id":"m-backend","kind":"brief","primary_language":"ru","allowed_languages":["ru","en","es"],"payload":{"model":"Google/gemma-4-12b-it","provider_id":"default","system":"Create a concise meeting brief in language `ru`. Return Markdown with a summary, decisions, and key discussion points. Do not invent facts absent from the transcript.","user":"Create the meeting brief from this final transcript:\n\n<transcript>\nОбсудили backend-генерацию.\n</transcript>"}}"#
                    .into(),
            ))
            .with_status(201)
            .with_body(
                r#"{"id":"j1","meeting_id":"m-backend","kind":"brief","status":"succeeded","error":null,"artifact_ids":["a1"]}"#,
            )
            .create();
        let _artifact = server
            .mock("GET", "/v1/artifacts/a1")
            .with_status(200)
            .with_body(
                r##"{"id":"a1","kind":"brief","body_markdown":"# Stub brief","created_at":"2026-08-02T00:00:00Z"}"##,
            )
            .create();
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-backend-success-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_transcript(&root, "m-backend");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_api_config(server.url(), "dev-token".into());
        core.set_llm_config(
            "backend".into(),
            "Google/gemma-4-12b-it".into(),
            String::new(),
            "default".into(),
        );

        let result = core.generate_artifact("m-backend".into(), FfiArtifactKind::Brief);

        assert!(result.error.is_empty(), "{}", result.error);
        assert_eq!(result.artifact.body_markdown, "# Stub brief");
        assert_eq!(result.artifact.template_id, "backend.brief");
        assert_eq!(core.list_artifacts("m-backend".into()).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generate_artifact_backend_surfaces_job_error() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .match_body(Matcher::Exact(
                r#"{"meeting_id":"m-backend-error","kind":"follow_up","primary_language":"ru","allowed_languages":["ru","en","es"],"payload":{"model":"Google/gemma-4-12b-it","provider_id":"default","system":"You are a meeting assistant. Draft a follow-up email in language `ru` as Markdown. Start with the subject line in an HTML comment, then include a greeting, a concise meeting summary, explicitly stated next steps, and a closing. Do not invent facts, assignments, or deadlines absent from the transcript.","user":"Draft a follow-up email from this final transcript:\n\n<transcript>\nОбсудили backend-генерацию.\n</transcript>"}}"#
                    .into(),
            ))
            .with_status(201)
            .with_body(
                r#"{"id":"j2","meeting_id":"m-backend-error","kind":"follow_up","status":"failed","error":"model unavailable","artifact_ids":[]}"#,
            )
            .create();
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-backend-error-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_transcript(&root, "m-backend-error");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_api_config(server.url(), "dev-token".into());
        core.set_llm_config(
            "backend".into(),
            "Google/gemma-4-12b-it".into(),
            String::new(),
            "default".into(),
        );

        let result = core.generate_artifact("m-backend-error".into(), FfiArtifactKind::FollowUp);

        assert!(
            result.error.contains("model unavailable"),
            "{}",
            result.error
        );
        assert!(result.artifact.id.is_empty());
        assert!(core.list_artifacts("m-backend-error".into()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_llm_engine_keeps_builtin_generation() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-builtin-normalization-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_transcript(&root, "m-builtin");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_llm_config(
            "unknown".into(),
            "ignored".into(),
            String::new(),
            String::new(),
        );

        let result = core.generate_artifact("m-builtin".into(), FfiArtifactKind::Brief);

        assert!(result.error.is_empty(), "{}", result.error);
        assert!(result.artifact.body_markdown.contains("# Brief"));
        assert_eq!(result.artifact.template_id, "builtin.brief");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generate_artifact_ollama_uses_http_body() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/api/chat")
            .match_body(Matcher::Regex(r#""model":"gemma2","stream":false"#.into()))
            .with_status(200)
            .with_body(
                r##"{"message":{"role":"assistant","content":"# Ollama brief"},"done":true}"##,
            )
            .expect(2)
            .create();
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-ollama-success-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_transcript(&root, "m-ollama");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_llm_config(
            "ollama".into(),
            "gemma2".into(),
            server.url(),
            String::new(),
        );

        let result = core.generate_artifact("m-ollama".into(), FfiArtifactKind::Brief);
        let follow_up = core.generate_artifact("m-ollama".into(), FfiArtifactKind::FollowUp);

        assert!(result.error.is_empty(), "{}", result.error);
        assert_eq!(result.artifact.body_markdown, "# Ollama brief");
        assert_eq!(result.artifact.template_id, "ollama.brief");
        assert!(follow_up.error.is_empty(), "{}", follow_up.error);
        assert_eq!(follow_up.artifact.template_id, "ollama.follow_up");
        assert_eq!(core.list_artifacts("m-ollama".into()).len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generate_artifact_ollama_error_does_not_insert() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/api/chat")
            .with_status(500)
            .with_body("model unavailable")
            .create();
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-ollama-error-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_transcript(&root, "m-ollama-error");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_llm_config(
            "ollama".into(),
            "gemma2".into(),
            server.url(),
            String::new(),
        );

        let result = core.generate_artifact("m-ollama-error".into(), FfiArtifactKind::FollowUp);

        assert!(result.error.contains("HTTP 500"), "{}", result.error);
        assert!(
            result.error.contains("model unavailable"),
            "{}",
            result.error
        );
        assert!(result.artifact.id.is_empty());
        assert!(
            core.list_artifacts("m-ollama-error".into()).is_empty(),
            "ошибка LLM не должна сохранять fallback-артефакт"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generate_artifact_openai_compat_sets_follow_up_template_id() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .match_body(Matcher::Regex(r#""model":"qwen2\.5""#.into()))
            .with_status(200)
            .with_body(
                r##"{"choices":[{"message":{"role":"assistant","content":"# Follow-up from compat"}}]}"##,
            )
            .expect(2)
            .create();
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-openai-success-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        seed_final_transcript(&root, "m-openai");
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_llm_config(
            "openai_compat".into(),
            "qwen2.5".into(),
            format!("{}/", server.url()),
            String::new(),
        );

        let brief = core.generate_artifact("m-openai".into(), FfiArtifactKind::Brief);
        let follow_up = core.generate_artifact("m-openai".into(), FfiArtifactKind::FollowUp);

        assert!(brief.error.is_empty(), "{}", brief.error);
        assert_eq!(brief.artifact.template_id, "openai.brief");
        assert!(follow_up.error.is_empty(), "{}", follow_up.error);
        assert_eq!(follow_up.artifact.body_markdown, "# Follow-up from compat");
        assert_eq!(follow_up.artifact.template_id, "openai.follow_up");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn llm_errors_map_to_core_error_variants() {
        let cases = [
            (
                postcall::LlmError::Http {
                    status: 429,
                    body: "busy".into(),
                },
                "LLM-провайдер вернул HTTP 429: busy",
            ),
            (
                postcall::LlmError::EmptyResponse,
                "LLM-провайдер вернул пустой ответ",
            ),
            (
                postcall::LlmError::Transport("connection refused".into()),
                "Ошибка транспорта LLM: connection refused",
            ),
            (postcall::LlmError::NotConfigured, "LLM-клиент не настроен"),
        ];

        for (source, expected) in cases {
            let error = CoreError::from(source);
            assert_eq!(error.to_string(), expected);
        }
    }
}
