//! UniFFI facade MeetingRaft: Swift ↔ session + recording + live STT.

uniffi::setup_scaffolding!();

use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use domain::{
    Artifact, ArtifactKind, AudioChannel, CaptionPhase, FinalTranscript, GlossaryKind,
    GlossaryScope, GlossaryTerm, LanguagePolicy, MeetingSummary, SearchHit, SessionState, Speaker,
    SpeechLanguage, body_fingerprint, edits_by_position,
};
use glossary::{GlossaryEngine, active_terms, parse_csv};
use postcall::{
    LlmClient, LlmError, OllamaNativeClient, OpenAiCompatLlmClient, assemble_final, brief_prompts,
    follow_up_prompts, make_artifact, occurrences_to_edit, plan_edit, promotable_term,
    render_brief, render_follow_up,
};
mod rebuild;

use postcall::{RebuildJobs, ThreadSpawner, diff_words, render_segments, speaker_stats};
use session::{ChannelMixer, MeetingSession};
use storage::{AudioManifestError, AudioManifestStore, DiagnosticsLog};
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

/// Что термин делает с текстом (Epic 19).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiGlossaryKind {
    /// Только подсказка распознавателю; готовый текст не трогает.
    Hint,
    /// Замена surface → canonical в готовом тексте.
    Replacement,
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
    /// Вид записи. Обязателен на границе: без него экран словаря
    /// возвращал бы подсказку как замену и первым же сохранением
    /// превращал автоматически рождённую подсказку в глобальную замену,
    /// переписывающую все будущие тексты.
    pub kind: FfiGlossaryKind,
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

/// Сегмент финального транскрипта для Swift.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFinalSegment {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    /// `mic` — владелец машины, `system` — остальные.
    pub channel: String,
    pub speaker_id: String,
    /// Разрешённое имя; пусто, если спикер не назначен или удалён.
    pub speaker_name: String,
    /// Спикера поставил человек: массовое назначение по каналу такую
    /// реплику не тронет.
    pub speaker_pinned: bool,
    pub text: String,
    /// Текст заменён ручной правкой из журнала (Epic 19).
    pub text_edited: bool,
    /// Что распознала модель; пусто, когда правки нет (Epic 19).
    pub original_text: String,
    /// id подсказки, родившейся из этой правки: кнопка «заменять
    /// всюду» показывается ровно когда поле непустое.
    ///
    /// Пусто, если термина нет, он уже замена или принадлежит чужой
    /// встрече. Решение принимает Rust: в Swift не должно уезжать ни
    /// знание про виды записи глоссария, ни разбор диффа.
    pub promotable_term_id: String,
}

/// Правка, не легшая ни на одну версию после пересбора.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSegmentEdit {
    pub id: String,
    pub channel: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub original_text: String,
    pub edited_text: String,
}

/// Кусок записи для прослушивания реплики.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAudioFragment {
    /// PCM i16 little-endian, моно.
    pub pcm: Vec<u8>,
    /// 0 — в запрошенном диапазоне записи нет.
    pub sample_rate: u32,
    pub duration_ms: u64,
}

/// Сводка по участнику встречи.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSpeakerStat {
    pub speaker_id: String,
    pub display_name: String,
    pub channel: String,
    pub segment_count: u32,
    pub speaking_ms: u64,
    /// Доля от общего времени речи, 0…1.
    pub share: f64,
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
    /// Текст Final изменился после сборки артефакта (Epic 8).
    ///
    /// Считается в ядре: разбор в Swift не уезжает.
    pub is_stale: bool,
    /// Версия Final, из которой собран; 0 — неизвестно.
    ///
    /// Ноль означает артефакт из базы, заведённой до отслеживания
    /// источника, и `is_stale` у такого всегда `false`: неизвестное не то
    /// же самое, что устаревшее.
    pub source_version: u32,
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
    /// Журнал решений распознавания; по умолчанию включён и локален.
    diagnostics: DiagnosticsLog,
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

/// Записать в журнал то, что движок выбросил или придержал.
///
/// Без этого отсев галлюцинаций (Epic 16) остаётся непроверяемым: он
/// молча удаляет текст, и попавшую под нож речь никто не увидит.
fn drain_stt_diagnostics(inner: &mut MeetingCoreInner) {
    let Some(pipeline) = inner.stt.as_mut() else {
        return;
    };
    let records = pipeline.take_diagnostics();
    if records.is_empty() {
        return;
    }
    inner.diagnostics.append(&records, now_ms());
}

/// Нормализовать глоссарием, сохранить и отдать в очередь UI.
fn store_and_enqueue(inner: &mut MeetingCoreInner, events: Vec<domain::CaptionEvent>) {
    drain_stt_diagnostics(inner);
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

/// Открыть хранилище по корню данных, не удерживая мьютекс ядра.
///
/// Свободная функция, а не метод: приватные методы внутри
/// `#[uniffi::export]`-блока всё равно пытаются пройти через границу, а
/// хранилище через неё не проходит.
fn open_store(core: &MeetingCore) -> Option<AudioManifestStore> {
    let guard = core.inner.lock().expect("meeting core poisoned");
    let root = guard.data_root.clone();
    drop(guard);
    AudioManifestStore::open(&root).ok()
}

/// Пересобрать markdown всех версий Final после правки атрибуции.
///
/// `body_markdown` производен от сегментов (ADR-011). Без пересборки
/// экспорт и Brief показывали бы имя, отменённое минуту назад, — молча и
/// без единого признака расхождения с экраном.
///
/// Возвращает описание ошибки или пустую строку.
fn rerender_final_bodies(store: &mut AudioManifestStore, meeting_id: &str) -> String {
    let speakers = match store.list_speakers(meeting_id) {
        Ok(speakers) => speakers,
        Err(error) => return error.to_string(),
    };
    let transcripts = match store.list_final_transcripts(meeting_id) {
        Ok(transcripts) => transcripts,
        Err(error) => return error.to_string(),
    };
    for transcript in transcripts {
        let segments = match store.list_final_segments(meeting_id, transcript.version) {
            Ok(segments) => segments,
            Err(error) => return error.to_string(),
        };
        // Версии, собранные до re-ASR, сегментов не имеют: их markdown —
        // единственный носитель текста, и перезаписать его нечем.
        if segments.is_empty() {
            continue;
        }
        let body_markdown = render_segments(&segments, &speakers);
        if body_markdown == transcript.body_markdown {
            continue;
        }
        if let Err(error) = store.upsert_final_transcript(&FinalTranscript {
            meeting_id: meeting_id.to_string(),
            version: transcript.version,
            body_markdown,
            // Время создания версии не трогаем: правка имени новой версии
            // не создаёт.
            created_at_ms: transcript.created_at_ms,
        }) {
            return error.to_string();
        }
    }
    String::new()
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

/// Действует ли термин в этой встрече.
fn term_applies_to_meeting(term: &GlossaryTerm, meeting_id: &str) -> bool {
    match &term.scope {
        GlossaryScope::Global => true,
        GlossaryScope::Meeting { meeting_id: id } => id == meeting_id,
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
        kind: match term.kind {
            GlossaryKind::Hint => FfiGlossaryKind::Hint,
            GlossaryKind::Replacement => FfiGlossaryKind::Replacement,
        },
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
        kind: match term.kind {
            FfiGlossaryKind::Hint => GlossaryKind::Hint,
            FfiGlossaryKind::Replacement => GlossaryKind::Replacement,
        },
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

/// Разошёлся ли артефакт с текущим состоянием транскрипта.
///
/// Версии мало: правка сегмента и назначение спикеров переписывают тело
/// Final на месте, номера версии не меняя. Отпечаток ловит и это, и
/// пересбор в новую версию.
///
/// Артефакт без записанного источника отставшим не считается: про него
/// ничего не известно, и выдать это за «устарел» — соврать в другую
/// сторону.
fn artifact_is_stale(artifact: &Artifact, latest: Option<&FinalTranscript>) -> bool {
    let (Some(version), Some(fingerprint)) = (
        artifact.source_version,
        artifact.source_fingerprint.as_deref(),
    ) else {
        return false;
    };
    let Some(latest) = latest else {
        return false;
    };
    version != latest.version || fingerprint != body_fingerprint(&latest.body_markdown)
}

fn artifact_to_ffi(artifact: Artifact, latest: Option<&FinalTranscript>) -> FfiArtifact {
    let is_stale = artifact_is_stale(&artifact, latest);
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
        is_stale,
        source_version: artifact.source_version.unwrap_or(0),
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
        is_stale: false,
        source_version: 0,
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
    source: &FinalTranscript,
) -> FfiGenerateArtifactResult {
    let mut artifact = make_artifact(meeting_id, kind, body, generated_at_ms);
    artifact.id = Uuid::new_v4().to_string();
    if let Some(template_id) = template_id {
        artifact.template_id = template_id.to_owned();
    }
    // Из чего собрано — чтобы позднейшая правка транскрипта не разошлась
    // с артефактом молча (Epic 8).
    artifact.source_version = Some(source.version);
    artifact.source_fingerprint = Some(body_fingerprint(&source.body_markdown));
    match write_store(inner, |store| store.insert_artifact(&artifact)) {
        Ok(()) => FfiGenerateArtifactResult {
            artifact: artifact_to_ffi(artifact, Some(source)),
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

/// Копия HTTP-клиента, чтобы запрос шёл вне мьютекса ядра (Epic 21).
///
/// Мьютекс один на всё ядро, и через него же каждые 50 мс проходит
/// `drain_events` живых субтитров. Запрос под гвардом останавливает их на
/// весь свой таймаут, а это молчаливый отказ посреди записи.
fn sync_client_snapshot(core: &MeetingCore) -> SyncClient {
    let guard = core.inner.lock().expect("meeting core poisoned");
    guard.sync_client.clone()
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
        let root = PathBuf::from(data_root);
        std::sync::Arc::new(Self {
            inner: Mutex::new(MeetingCoreInner {
                session: MeetingSession::new(),
                started_at: None,
                store: None,
                recording_session_id: None,
                diagnostics: DiagnosticsLog::new(&root, true),
                data_root: root,
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
        match sync_client_snapshot(self).list_models() {
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
        match sync_client_snapshot(self).health() {
            Ok(()) => String::new(),
            Err(error) => error.to_string(),
        }
    }

    pub fn submit_backend_job(&self, meeting_id: String, kind_code: String) -> FfiBackendJob {
        let Some(kind) = JobKind::from_code(&kind_code) else {
            return empty_backend_job(format!("unsupported job kind: {kind_code}"));
        };
        // Запрос собирается под мьютексом, отправляется — уже без него.
        let (client, request) = {
            let guard = self.inner.lock().expect("meeting core poisoned");
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
            (guard.sync_client.clone(), request)
        };
        match client.create_job(&request) {
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
        match sync_client_snapshot(self).get_job(&job_id) {
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
        match sync_client_snapshot(self).get_artifact(&artifact_id) {
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

    /// Путь к локальному журналу диагностики.
    ///
    /// Журнал никуда не отправляется: он лежит рядом с записями встреч, и
    /// уходит куда-либо, только если человек сам его отдаст.
    pub fn diagnostics_log_path(&self) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.diagnostics.path().to_string_lossy().into_owned()
    }

    pub fn diagnostics_log_size_bytes(&self) -> u64 {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.diagnostics.size_bytes()
    }

    pub fn is_diagnostics_log_enabled(&self) -> bool {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.diagnostics.is_enabled()
    }

    /// Выключение обязано и перестать писать, и не оставлять прошлое:
    /// журнал содержит текст встреч, и «выключено» должно значить пусто.
    pub fn set_diagnostics_log_enabled(&self, enabled: bool) {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        guard.diagnostics.set_enabled(enabled);
        if !enabled {
            guard.diagnostics.clear();
        }
    }

    pub fn clear_diagnostics_log(&self) {
        let guard = self.inner.lock().expect("meeting core poisoned");
        guard.diagnostics.clear();
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
            meeting_id: meeting_id.clone(),
            display_name,
            sort_index,
        };
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        match write_store(&mut guard, |store| {
            store.upsert_speaker(&speaker)?;
            Ok(rerender_final_bodies(store, &meeting_id))
        }) {
            Ok(error) => error,
            Err(error) => error.to_string(),
        }
    }

    /// Удалить участника; его реплики остаются, но без имени.
    ///
    /// Встреча в параметрах не для поиска записи, а ради пересборки
    /// markdown: без неё экспорт сохранит имя удалённого участника.
    pub fn delete_speaker(&self, meeting_id: String, id: String) -> String {
        let mut guard = self.inner.lock().expect("meeting core poisoned");
        match write_store(&mut guard, |store| {
            store.delete_speaker(&id)?;
            Ok(rerender_final_bodies(store, &meeting_id))
        }) {
            Ok(error) => error,
            Err(error) => error.to_string(),
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

    /// Сегменты версии Final с именами говорящих.
    pub fn list_final_segments(&self, meeting_id: String, version: u32) -> Vec<FfiFinalSegment> {
        let Some(store) = open_store(self) else {
            return Vec::new();
        };
        let segments = store
            .list_final_segments(&meeting_id, version)
            .unwrap_or_default();
        let speakers = store.list_speakers(&meeting_id).unwrap_or_default();
        // Словарь читается один раз на весь список: подсказка ищется для
        // каждого правленого сегмента, а чтение на строку превратило бы
        // открытие транскрипта в сотни запросов.
        let terms = store.list_glossary_terms().unwrap_or_default();
        let language = {
            let guard = self.inner.lock().expect("meeting core poisoned");
            guard.language_policy.primary
        };
        segments
            .into_iter()
            .map(|segment| {
                let speaker_name = speakers
                    .iter()
                    .find(|speaker| speaker.id == segment.speaker_id)
                    .map(|speaker| speaker.display_name.clone())
                    .unwrap_or_default();
                let promotable_term_id = if segment.text_edited {
                    promotable_term(
                        &segment.original_text,
                        &segment.text,
                        &terms,
                        &meeting_id,
                        language,
                    )
                    .map(|term| term.id.clone())
                    .unwrap_or_default()
                } else {
                    String::new()
                };
                FfiFinalSegment {
                    index: segment.index,
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    channel: segment.channel.code().to_string(),
                    speaker_id: segment.speaker_id,
                    speaker_name,
                    speaker_pinned: segment.speaker_pinned,
                    text: segment.text,
                    text_edited: segment.text_edited,
                    original_text: segment.original_text,
                    promotable_term_id,
                }
            })
            .collect()
    }

    /// Звук реплики: диапазон времени на её же дорожке.
    ///
    /// Нужен, чтобы услышать спорное слово перед правкой (Epic 19):
    /// исправлять распознанное на слух, не имея слуха, нельзя.
    ///
    /// Канал берётся из сегмента, а не угадывается: после ADR-011 он
    /// известен точно, и играть надо именно того, кто это сказал.
    pub fn segment_audio(
        &self,
        meeting_id: String,
        channel_code: String,
        start_ms: u64,
        end_ms: u64,
    ) -> FfiAudioFragment {
        let empty = FfiAudioFragment {
            pcm: Vec::new(),
            sample_rate: 0,
            duration_ms: 0,
        };
        let Some(store) = open_store(self) else {
            return empty;
        };
        let Ok(fragment) = store.read_pcm_range(
            &meeting_id,
            AudioChannel::from_code(&channel_code),
            start_ms,
            end_ms,
        ) else {
            return empty;
        };
        FfiAudioFragment {
            duration_ms: fragment.duration_ms(),
            sample_rate: fragment.sample_rate,
            pcm: fragment
                .pcm
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect(),
        }
    }

    /// Сводка по участникам версии Final.
    pub fn list_speaker_stats(&self, meeting_id: String, version: u32) -> Vec<FfiSpeakerStat> {
        let Some(store) = open_store(self) else {
            return Vec::new();
        };
        let segments = store
            .list_final_segments(&meeting_id, version)
            .unwrap_or_default();
        let speakers = store.list_speakers(&meeting_id).unwrap_or_default();
        speaker_stats(&segments, &speakers)
            .into_iter()
            .map(|stat| FfiSpeakerStat {
                speaker_id: stat.speaker_id,
                display_name: stat.display_name,
                channel: stat.channel.code().to_string(),
                segment_count: stat.segment_count,
                speaking_ms: stat.speaking_ms,
                share: stat.share,
            })
            .collect()
    }

    /// Назначить спикера всем непоправленным репликам канала.
    pub fn assign_channel_speaker(
        &self,
        meeting_id: String,
        version: u32,
        channel_code: String,
        speaker_id: String,
    ) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        match store.set_channel_speaker(
            &meeting_id,
            version,
            AudioChannel::from_code(&channel_code),
            &speaker_id,
        ) {
            Ok(_) => rerender_final_bodies(&mut store, &meeting_id),
            Err(error) => error.to_string(),
        }
    }

    /// Назначить спикера одной реплике; она перестаёт подчиняться каналу.
    pub fn assign_segment_speaker(
        &self,
        meeting_id: String,
        version: u32,
        index: u32,
        speaker_id: String,
    ) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        match store.set_segment_speaker(&meeting_id, version, index, &speaker_id) {
            Ok(()) => rerender_final_bodies(&mut store, &meeting_id),
            Err(error) => error.to_string(),
        }
    }

    /// Правка текста сегмента. Пустая строка в ответе — успех.
    ///
    /// Текст, совпавший с распознанным, удаляет правку: возврат к
    /// исходному — это отмена, а не ещё одна правка (Epic 19).
    pub fn edit_segment_text(
        &self,
        meeting_id: String,
        version: u32,
        index: u32,
        text: String,
    ) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };

        // Сбой чтения — не «сегмента нет»: проглотить его значит сказать
        // человеку неправду о его данных.
        let segments = match store.list_final_segments(&meeting_id, version) {
            Ok(segments) => segments,
            Err(error) => return error.to_string(),
        };
        let Some(segment) = segments.into_iter().find(|s| s.index == index) else {
            return format!("сегмент {index} не найден");
        };

        // Предыдущая правка ищется до разбора: list_final_segments уже
        // отдал правленый текст, а сравнивать введённое надо с
        // распознанным. Иначе повторный ввод того же текста читался бы
        // как возврат к исходному и правка бы удалилась.
        //
        // Правка ищется тем же правилом, что и при чтении сегментов, — и
        // только среди правок этой версии. Без фильтра по версии правка
        // первой версии перехватывалась бы правкой того же места во
        // второй: версия переезжала, в первой правка молча исчезала, а
        // исходный текст оставался от первой — «вернуть исходное» во
        // второй требовало бы ввести текст, которого человеку никто не
        // показывал.
        let existing = match store.list_segment_edits(&meeting_id) {
            Ok(edits) => edits,
            Err(error) => return error.to_string(),
        };
        let previous = edits_by_position(&existing, version)
            .get(&segment.position())
            .map(|edit| (*edit).clone());

        let mut recognized = segment.clone();
        if let Some(previous) = &previous {
            recognized.text = previous.original_text.clone();
            // На разбор в plan_edit это поле не влияет — выставляется для
            // смысловой целостности копии: `recognized` должен отражать
            // распознанное состояние сегмента, а не текущее правленое.
            recognized.text_edited = false;
        }

        // Пустой список вместо ошибки здесь недопустим: по нему plan_edit
        // не найдёт действующий термин, выдаст новый с видом «подсказка», и
        // подтверждённая человеком замена будет молча понижена.
        let terms = match store.list_glossary_terms() {
            Ok(terms) => terms,
            Err(error) => return error.to_string(),
        };
        let language = {
            let guard = self.inner.lock().expect("meeting core poisoned");
            guard.language_policy.primary
        };

        let outcome = plan_edit(
            &meeting_id,
            version,
            &recognized,
            &text,
            language,
            &terms,
            &Uuid::new_v4().to_string(),
            &Uuid::new_v4().to_string(),
            now_ms(),
        );

        match (outcome.edit, previous) {
            (Some(mut edit), previous) => {
                // Правка того же места перезаписывается, а не копится.
                if let Some(previous) = previous {
                    edit.id = previous.id;
                }
                if let Err(error) = store.upsert_segment_edit(&edit) {
                    return error.to_string();
                }
            }
            (None, Some(previous)) => {
                if let Err(error) = store.delete_segment_edit(&previous.id) {
                    return error.to_string();
                }
            }
            (None, None) => {}
        }

        if let Some(term) = outcome.term
            && let Err(error) = store.upsert_glossary_term(&term, now_ms())
        {
            return error.to_string();
        }
        // Тело markdown производно от сегментов — после правки его надо
        // пересобрать, как это делает назначение спикера.
        rerender_final_bodies(&mut store, &meeting_id)
    }

    /// Правки, не легшие на текущую версию после пересбора.
    pub fn list_unapplied_edits(&self, meeting_id: String) -> Vec<FfiSegmentEdit> {
        let Some(store) = open_store(self) else {
            return Vec::new();
        };
        store
            .list_unapplied_segment_edits(&meeting_id)
            .unwrap_or_default()
            .into_iter()
            .map(|edit| FfiSegmentEdit {
                id: edit.id,
                channel: edit.channel.code().to_string(),
                start_ms: edit.start_ms,
                end_ms: edit.end_ms,
                original_text: edit.original_text,
                edited_text: edit.edited_text,
            })
            .collect()
    }

    /// Снять правку из журнала. Пустая строка — успех.
    ///
    /// Нужен неприменившимся правкам: показать их и не дать убрать —
    /// значит оставить человеку раздел, который никогда не опустеет.
    pub fn delete_segment_edit(&self, edit_id: String) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        match store.delete_segment_edit(&edit_id) {
            Ok(()) => String::new(),
            Err(error) => error.to_string(),
        }
    }

    /// Превратить подсказку в замену: применять всюду.
    ///
    /// Единственный способ получить замену из правки — явный жест
    /// человека. Автоматически рождаются только подсказки.
    ///
    /// Термин должен действовать в этой встрече: глобальный или её
    /// собственный. Термином чужой встречи замена наплодила бы правки
    /// там, где он не применяется, — а значит, следующий пересбор их не
    /// подтвердит и человек получит расхождение без объяснения.
    pub fn promote_term_to_replacement(
        &self,
        term_id: String,
        meeting_id: String,
        version: u32,
    ) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        let terms = match store.list_glossary_terms() {
            Ok(terms) => terms,
            Err(error) => return error.to_string(),
        };
        let Some(mut term) = terms.into_iter().find(|term| term.id == term_id) else {
            return format!("термин {term_id} не найден");
        };
        if !term_applies_to_meeting(&term, &meeting_id) {
            return format!("термин {term_id} не действует во встрече {meeting_id}");
        }
        // Массовая замена идёт через `normalize_caption`, а тот берёт
        // только замены: вид выставляется здесь и до вызова.
        term.kind = GlossaryKind::Replacement;
        if let Err(error) = store.upsert_glossary_term(&term, now_ms()) {
            return error.to_string();
        }

        // Сбой чтения не должен выглядеть успешной заменой, которая
        // ничего не заменила.
        let segments = match store.list_final_segments(&meeting_id, version) {
            Ok(segments) => segments,
            Err(error) => return error.to_string(),
        };
        let existing = match store.list_segment_edits(&meeting_id) {
            Ok(edits) => edits,
            Err(error) => return error.to_string(),
        };
        let mut ids = std::iter::repeat_with(|| Uuid::new_v4().to_string());
        let created = occurrences_to_edit(
            &term,
            &meeting_id,
            version,
            &segments,
            &existing,
            now_ms(),
            &mut ids,
        );
        for edit in &created {
            if let Err(error) = store.upsert_segment_edit(edit) {
                return error.to_string();
            }
        }
        rerender_final_bodies(&mut store, &meeting_id)
    }

    /// Вернуть реплику под назначение по каналу.
    pub fn unpin_segment_speaker(&self, meeting_id: String, version: u32, index: u32) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        match store.unpin_segment_speaker(&meeting_id, version, index) {
            Ok(()) => rerender_final_bodies(&mut store, &meeting_id),
            Err(error) => error.to_string(),
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
    /// Запустить пересбор Final в фоне; возвращает id задачи.
    ///
    /// Имена спикеров по умолчанию приходят из Swift: формулировка
    /// локале-зависима и не должна жить в ядре.
    pub fn start_final_rebuild_named(
        &self,
        meeting_id: String,
        mic_speaker_name: String,
        system_speaker_name: String,
    ) -> String {
        self.start_rebuild(meeting_id, mic_speaker_name, system_speaker_name)
    }

    pub fn start_final_rebuild(&self, meeting_id: String) -> String {
        self.start_rebuild(meeting_id, "You".to_string(), "Others".to_string())
    }

    fn start_rebuild(
        &self,
        meeting_id: String,
        mic_speaker_name: String,
        system_speaker_name: String,
    ) -> String {
        let guard = self.inner.lock().expect("meeting core poisoned");
        let params = rebuild::RebuildParams {
            data_root: guard.data_root.clone(),
            meeting_id: meeting_id.clone(),
            policy: guard.language_policy.clone(),
            post_call_model: guard.post_call_whisper_model.clone(),
            llm_engine: normalize_llm_engine(&guard.llm_engine).to_owned(),
            llm_base_url: guard.llm_base_url.clone(),
            llm_model_id: guard.llm_model_id.clone(),
            mic_speaker_name,
            system_speaker_name,
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
        // Последний Final — то, с чем артефакт обязан совпадать: Brief,
        // экспорт и пересборка опираются именно на него.
        let latest = read_store(&guard, |store| store.get_final_transcript(&meeting_id))
            .ok()
            .flatten();
        read_store(&guard, |store| store.list_artifacts(&meeting_id))
            .unwrap_or_default()
            .into_iter()
            .map(|artifact| artifact_to_ffi(artifact, latest.as_ref()))
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
                &final_transcript,
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
                &final_transcript,
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
            &final_transcript,
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
        // Спикер по умолчанию назначается самим проходом (Phase 11, T2).
        assert!(!segments[0].speaker_id.is_empty());

        let transcript = store.get_final_transcript(&meeting_id).expect("final");
        assert!(transcript.is_some_and(|t| !t.body_markdown.is_empty()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Пересбор подписывает реплики без единой ручной операции: канал
    /// микрофона — владелец машины, системный — собеседник.
    #[test]
    fn rebuild_assigns_default_speakers_and_keeps_renames() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-speakers-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-speakers".to_string();
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

        let first = run_rebuild_to_completion(&core, &meeting_id);
        assert_eq!(first, "succeeded");

        let store = AudioManifestStore::open(&root).expect("store");
        let speakers = store.list_speakers(&meeting_id).expect("speakers");
        assert_eq!(speakers.len(), 1, "системного канала не было: {speakers:?}");
        assert_eq!(speakers[0].display_name, "Вы");
        let version = store.next_final_version(&meeting_id).expect("version") - 1;
        let segments = store
            .list_final_segments(&meeting_id, version)
            .expect("segments");
        assert!(segments.iter().all(|s| s.speaker_id == speakers[0].id));
        drop(store);

        // Человек дал спикеру настоящее имя.
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            let mut renamed = speakers[0].clone();
            renamed.display_name = "Сергей".into();
            store.upsert_speaker(&renamed).expect("rename");
        }

        let second = run_rebuild_to_completion(&core, &meeting_id);
        assert_eq!(second, "succeeded");

        let store = AudioManifestStore::open(&root).expect("store");
        let after = store.list_speakers(&meeting_id).expect("speakers");
        assert_eq!(after.len(), 1, "пересбор не должен плодить дубликаты");
        assert_eq!(after[0].display_name, "Сергей", "имя затёрто пересбором");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn run_rebuild_to_completion(core: &MeetingCore, meeting_id: &str) -> String {
        let job_id = core.start_final_rebuild_named(
            meeting_id.to_string(),
            "Вы".to_string(),
            "Собеседник".to_string(),
        );
        for _ in 0..300 {
            let progress = core.final_rebuild_progress(job_id.clone());
            if matches!(
                progress.state.as_str(),
                "succeeded" | "failed" | "cancelled"
            ) {
                return progress.state;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        "timeout".to_string()
    }

    fn edits_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mr-ffi-{name}-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ))
    }

    /// Артефакт, только что собранный из версии 1 засеянной встречи.
    fn seed_and_generate(name: &str) -> (std::path::PathBuf, String, std::sync::Arc<MeetingCore>) {
        let root = edits_root(name);
        let meeting_id = format!("m-{name}");
        seed_segment_version(
            &root,
            &meeting_id,
            1,
            &[(0, 0, 1_000, "упирается в юни-эф-эф-ай")],
        );
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        (root, meeting_id, core)
    }

    /// Правка текста после сборки расходится с артефактом.
    #[test]
    fn editing_a_segment_marks_the_artifact_stale() {
        let (root, meeting_id, core) = seed_and_generate("artifact-stale-edit");

        let generated = core.generate_artifact(meeting_id.clone(), FfiArtifactKind::Brief);
        assert!(generated.error.is_empty(), "{}", generated.error);
        assert!(!generated.artifact.is_stale, "только что собран");
        assert_eq!(generated.artifact.source_version, 1);

        let error = core.edit_segment_text(meeting_id.clone(), 1, 0, "упирается в UniFFI".into());
        assert!(error.is_empty(), "правка: {error}");

        let artifacts = core.list_artifacts(meeting_id);
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].is_stale, "текст правился после сборки");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Переименование спикера тоже переписывает тело Final — случай,
    /// который время последней правки не ловит вовсе: журнал правок про
    /// спикеров ничего не знает.
    #[test]
    fn renaming_a_speaker_marks_the_artifact_stale() {
        let (root, meeting_id, core) = seed_and_generate("artifact-stale-speaker");

        let error = core.upsert_speaker(meeting_id.clone(), "s1".into(), "Пётр".into(), 0);
        assert!(error.is_empty(), "спикер: {error}");
        let error = core.assign_channel_speaker(meeting_id.clone(), 1, "mic".into(), "s1".into());
        assert!(error.is_empty(), "назначение: {error}");

        let generated = core.generate_artifact(meeting_id.clone(), FfiArtifactKind::Brief);
        assert!(generated.error.is_empty(), "{}", generated.error);
        assert!(!generated.artifact.is_stale);

        let error = core.upsert_speaker(meeting_id.clone(), "s1".into(), "Пётр Иванов".into(), 0);
        assert!(error.is_empty(), "переименование: {error}");

        let artifacts = core.list_artifacts(meeting_id);
        assert!(artifacts[0].is_stale, "подпись реплики изменилась");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Пересбор в новую версию: текст версии 1 не менялся, но Brief
    /// собран не из того Final, который теперь считается последним.
    #[test]
    fn a_newer_final_version_marks_the_artifact_stale() {
        let (root, meeting_id, core) = seed_and_generate("artifact-stale-version");

        let generated = core.generate_artifact(meeting_id.clone(), FfiArtifactKind::Brief);
        assert!(generated.error.is_empty(), "{}", generated.error);

        seed_segment_version(
            &root,
            &meeting_id,
            2,
            &[(0, 0, 1_000, "упирается в UniFFI")],
        );

        let artifacts = core.list_artifacts(meeting_id);
        assert!(artifacts[0].is_stale, "собран по версии 1, сейчас 2");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ничего не трогали — плашки быть не должно, иначе она обесценится.
    #[test]
    fn an_untouched_artifact_is_not_stale() {
        let (root, meeting_id, core) = seed_and_generate("artifact-fresh");

        let generated = core.generate_artifact(meeting_id.clone(), FfiArtifactKind::Brief);
        assert!(generated.error.is_empty(), "{}", generated.error);

        let artifacts = core.list_artifacts(meeting_id);
        assert!(!artifacts[0].is_stale);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Артефакт из базы, заведённой до отслеживания источника: полей нет,
    /// и «неизвестно» не выдаётся за «устарел» даже при разошедшемся
    /// тексте.
    #[test]
    fn an_artifact_without_recorded_source_is_never_stale() {
        let (root, meeting_id, core) = seed_and_generate("artifact-legacy");
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store
                .insert_artifact(&Artifact {
                    id: "a-legacy".into(),
                    meeting_id: meeting_id.clone(),
                    kind: ArtifactKind::Brief,
                    template_id: "builtin.brief".into(),
                    body_markdown: "# Старый бриф".into(),
                    created_at_ms: 10,
                    source_version: None,
                    source_fingerprint: None,
                })
                .expect("артефакт");
        }

        let error = core.edit_segment_text(meeting_id.clone(), 1, 0, "упирается в UniFFI".into());
        assert!(error.is_empty(), "правка: {error}");

        let artifacts = core.list_artifacts(meeting_id);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].source_version, 0, "источник неизвестен");
        assert!(!artifacts[0].is_stale);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Сервер, который принимает соединение и молчит: так «медленный
    /// backend» воспроизводится точно, без надежд на задержки mockito.
    /// Возвращает адрес и приёмник, сигналящий о принятом соединении.
    fn silent_server() -> (String, std::sync::mpsc::Receiver<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("слушатель должен подняться");
        let url = format!("http://{}", listener.local_addr().expect("адрес"));
        let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            // Соединение держим до конца теста: закрытый сокет вернул бы
            // клиенту ошибку сразу, и ждать стало бы нечего.
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept() {
                held.push(stream);
                if accepted_tx.send(()).is_err() {
                    return;
                }
            }
        });
        (url, accepted_rx)
    }

    /// Замер `drain_events` в отдельном потоке: главный поток не имеет
    /// права уснуть на мьютексе, иначе тест не отличит блокировку от неё.
    fn drain_events_completes_within(
        core: &std::sync::Arc<MeetingCore>,
        timeout: Duration,
    ) -> bool {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let probe = std::sync::Arc::clone(core);
        thread::spawn(move || {
            probe.drain_events();
            let _ = done_tx.send(());
        });
        done_rx.recv_timeout(timeout).is_ok()
    }

    /// Живые субтитры опрашивают `drain_events` каждые 50 мс через тот же
    /// мьютекс, что и сетевые методы. Пока идёт запрос к недоступному
    /// backend, опрос обязан проходить — иначе субтитры встают на весь
    /// таймаут запроса (Epic 21).
    #[test]
    fn network_call_does_not_hold_the_core_mutex() {
        let root = edits_root("net-mutex");
        let (url, accepted) = silent_server();

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_api_config(url, "t".into());

        let calling = std::sync::Arc::clone(&core);
        thread::spawn(move || {
            let _ = calling.test_api_connection();
        });

        // Принятое соединение доказывает, что запрос ушёл и клиент сидит
        // в ожидании ответа; до этого момента мерить нечего.
        accepted
            .recv_timeout(Duration::from_secs(5))
            .expect("клиент должен дойти до отправки запроса");

        assert!(
            drain_events_completes_within(&core, Duration::from_secs(2)),
            "drain_events ждёт сетевой вызов — живые субтитры встанут"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Тот же инвариант для генерации артефакта: таймаут LLM больше
    /// минуты, и сегодня он держится только на `drop(guard)` в коде.
    #[test]
    fn generate_artifact_does_not_hold_the_core_mutex() {
        let root = edits_root("net-mutex-artifact");
        let meeting_id = "m-net-mutex".to_string();
        seed_final_transcript(&root, &meeting_id);
        let (url, accepted) = silent_server();

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        core.set_api_config(url, "t".into());
        core.set_llm_config("backend".into(), "m1".into(), String::new(), "p1".into());

        let calling = std::sync::Arc::clone(&core);
        let meeting = meeting_id.clone();
        thread::spawn(move || {
            let _ = calling.generate_artifact(meeting, FfiArtifactKind::Brief);
        });

        accepted
            .recv_timeout(Duration::from_secs(5))
            .expect("клиент должен дойти до отправки запроса");

        assert!(
            drain_events_completes_within(&core, Duration::from_secs(2)),
            "generate_artifact ждёт LLM под мьютексом"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn seed_segment_version(
        root: &std::path::Path,
        meeting_id: &str,
        version: u32,
        segments: &[(u32, u64, u64, &str)],
    ) {
        let mut store = AudioManifestStore::open(root).expect("store");
        store
            .upsert_final_transcript(&FinalTranscript {
                meeting_id: meeting_id.to_owned(),
                version,
                body_markdown: String::new(),
                created_at_ms: 1,
            })
            .expect("transcript");
        let rows: Vec<domain::FinalSegment> = segments
            .iter()
            .map(|(index, start_ms, end_ms, text)| domain::FinalSegment {
                index: *index,
                start_ms: *start_ms,
                end_ms: *end_ms,
                channel: AudioChannel::Mic,
                speaker_id: String::new(),
                speaker_pinned: false,
                text: (*text).to_owned(),
                text_edited: false,
                original_text: String::new(),
            })
            .collect();
        store
            .replace_final_segments(meeting_id, version, &rows)
            .expect("segments");
    }

    /// Две правки, не легшие ни на одну версию.
    fn seed_two_unapplied_edits(root: &std::path::Path, meeting_id: &str) {
        let mut store = AudioManifestStore::open(root).expect("store");
        for (id, start_ms, end_ms) in [("e1", 0_u64, 1_000_u64), ("e2", 2_000, 3_000)] {
            store
                .upsert_segment_edit(&domain::SegmentEdit {
                    id: id.to_owned(),
                    meeting_id: meeting_id.to_owned(),
                    channel: AudioChannel::Mic,
                    start_ms,
                    end_ms,
                    original_text: format!("распознано {id}"),
                    edited_text: format!("поправлено {id}"),
                    created_at_ms: 10,
                    applied_version: None,
                })
                .expect("правка");
        }
    }

    /// Удаление снимает только названную правку.
    ///
    /// Вторая правка в данных обязательна: без неё тест не отличит
    /// «удалил нужную» от «вычистил журнал».
    #[test]
    fn delete_segment_edit_removes_only_named_one() {
        let root = edits_root("edit-delete");
        let meeting_id = "m-edit-delete".to_string();
        seed_two_unapplied_edits(&root, &meeting_id);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let before = core.list_unapplied_edits(meeting_id.clone());
        assert_eq!(before.len(), 2, "подготовка: две неприменившиеся правки");

        let error = core.delete_segment_edit(before[0].id.clone());
        assert!(error.is_empty(), "удаление: {error}");

        let after = core.list_unapplied_edits(meeting_id.clone());
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, before[1].id);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Правленый сегмент отдаёт распознанное и id подсказки для повышения.
    #[test]
    fn edited_segment_exposes_original_text_and_promotable_term() {
        let root = edits_root("edit-fields");
        let meeting_id = "m-edit-fields".to_string();
        seed_segment_version(
            &root,
            &meeting_id,
            1,
            &[(0, 0, 1_000, "упирается в юни-эф-эф-ай")],
        );

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let error = core.edit_segment_text(meeting_id.clone(), 1, 0, "упирается в UniFFI".into());
        assert!(error.is_empty(), "правка: {error}");

        let segments = core.list_final_segments(meeting_id.clone(), 1);
        assert_eq!(segments[0].text, "упирается в UniFFI");
        assert_eq!(segments[0].original_text, "упирается в юни-эф-эф-ай");
        assert!(
            !segments[0].promotable_term_id.is_empty(),
            "из короткой правки родилась подсказка — её и повышаем"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Неправленый сегмент не предлагает ни исходного текста, ни повышения.
    #[test]
    fn untouched_segment_exposes_neither_field() {
        let root = edits_root("edit-fields-untouched");
        let meeting_id = "m-edit-fields-untouched".to_string();
        seed_segment_version(&root, &meeting_id, 1, &[(0, 0, 1_000, "обычная реплика")]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let segments = core.list_final_segments(meeting_id.clone(), 1);
        assert!(segments[0].original_text.is_empty());
        assert!(segments[0].promotable_term_id.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Сквозной сценарий переноса: правка на версии 1 переезжает на
    /// сегмент версии 2 вместе с его координатами.
    ///
    /// Пересбор создаёт новую нарезку, а журнал живёт отдельно и сам на
    /// неё не переберётся. Без вызова переноса правка осталась бы с
    /// версией 1: в новой версии её не видно, и в списке неприменившихся
    /// тоже — `applied_version IS NULL` никто не выставляет. Без записи
    /// новых координат перенос был бы тихой пустышкой: версия новая,
    /// координаты старые, наложение при чтении ищет по координатам.
    #[test]
    fn rebuild_moves_manual_edits_onto_the_new_version() {
        let root = edits_root("edit-rebuild");
        let meeting_id = "m-edit-rebuild".to_string();
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
        // Версия 1 нарезана иначе, чем нарежет пересбор: так видно, что
        // правка переезжает на новые границы, а не совпадает с ними
        // случайно. Текст тот же, что выдаст mock-распознавание, — по нему
        // правка и опознаёт своё место.
        seed_segment_version(&root, &meeting_id, 1, &[(0, 200, 800, "[mock 0-1000]")]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let saved = core.edit_segment_text(
            meeting_id.clone(),
            1,
            0,
            "человек поправил эту реплику руками".into(),
        );
        assert!(saved.is_empty(), "{saved}");

        assert_eq!(run_rebuild_to_completion(&core, &meeting_id), "succeeded");

        let store = AudioManifestStore::open(&root).expect("store");
        let version = store.next_final_version(&meeting_id).expect("version") - 1;
        assert_eq!(version, 2, "пересбор создаёт новую версию");

        let edits = store.list_segment_edits(&meeting_id).expect("журнал");
        assert_eq!(edits.len(), 1, "перенос не плодит строк журнала");
        assert_eq!(edits[0].applied_version, Some(2), "правка на новой версии");
        assert_eq!(
            (edits[0].start_ms, edits[0].end_ms),
            (0, 1_000),
            "координаты переехали на сегмент новой нарезки"
        );
        assert_eq!(
            edits[0].original_text, "[mock 0-1000]",
            "распознанное первой версии сохраняется: на нём стоит отмена"
        );

        let segments = core.list_final_segments(meeting_id.clone(), version);
        assert_eq!(segments[0].text, "человек поправил эту реплику руками");
        assert!(segments[0].text_edited);
        assert!(
            core.list_unapplied_edits(meeting_id.clone()).is_empty(),
            "правка применилась — в разделе неприменившихся ей не место"
        );

        let body = store
            .get_final_transcript_version(&meeting_id, version)
            .expect("транскрипт")
            .expect("версия есть")
            .body_markdown;
        assert!(
            body.contains("человек поправил эту реплику руками"),
            "тело собирается после переноса: {body}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Правка не переезжает на другую версию сама.
    ///
    /// Пересбор при неизменной модели даёт ту же нарезку, поэтому одно и
    /// то же место в двух версиях — норма. Раньше правка искалась по
    /// координатам без учёта версии: правка второй версии забирала себе
    /// строку журнала первой, и в первой правка молча исчезала — а
    /// исходный текст оставался от первой, так что «вернуть исходное» во
    /// второй требовало ввести текст, которого человеку не показывали.
    #[test]
    fn edit_of_one_version_does_not_steal_the_edit_of_another() {
        let root = edits_root("edit-versions");
        let meeting_id = "m-edit-versions".to_string();
        let recognized = "зашли на интра ру";
        seed_segment_version(&root, &meeting_id, 1, &[(0, 1_000, 2_000, recognized)]);
        seed_segment_version(&root, &meeting_id, 2, &[(0, 1_000, 2_000, recognized)]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.edit_segment_text(meeting_id.clone(), 1, 0, "зашли на intra.ru".into())
                .is_empty()
        );
        assert!(
            core.edit_segment_text(meeting_id.clone(), 2, 0, "зашли на портал".into())
                .is_empty()
        );

        assert_eq!(
            core.list_final_segments(meeting_id.clone(), 1)[0].text,
            "зашли на intra.ru",
            "правка первой версии осталась на месте"
        );
        assert_eq!(
            core.list_final_segments(meeting_id.clone(), 2)[0].text,
            "зашли на портал"
        );

        let store = AudioManifestStore::open(&root).expect("store");
        assert_eq!(
            store.list_segment_edits(&meeting_id).expect("журнал").len(),
            2,
            "у каждой версии своя строка журнала"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Правка → чтение → повторная правка того же места → возврат к
    /// распознанному, и всё это без потери замены, подтверждённой
    /// человеком.
    ///
    /// Запись термина — это «удалить по (форма, язык, область) и
    /// вставить». Пока `plan_edit` выдавал термин с новым id и видом
    /// «подсказка», следующая правка той же фразы стирала строку
    /// «замена», созданную явным жестом: замена переставала работать без
    /// единого сигнала, а сохранённая ссылка на термин протухала.
    #[test]
    fn repeated_edit_keeps_the_confirmed_replacement() {
        let root = edits_root("edit-term");
        let meeting_id = "m-edit-term".to_string();
        let recognized = "зашли на интра ру";
        seed_segment_version(&root, &meeting_id, 1, &[(0, 1_000, 2_000, recognized)]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.edit_segment_text(meeting_id.clone(), 1, 0, "зашли на intra.ru".into())
                .is_empty()
        );

        let terms = core.list_glossary_terms();
        assert_eq!(terms.len(), 1, "правка рождает ровно один термин");
        assert!(
            matches!(terms[0].kind, FfiGlossaryKind::Hint),
            "автоматически рождается только подсказка"
        );
        let term_id = terms[0].id.clone();

        // Явный жест человека: «заменять всюду».
        let promoted = core.promote_term_to_replacement(term_id.clone(), meeting_id.clone(), 1);
        assert!(promoted.is_empty(), "{promoted}");

        // Человек правит то же место ещё раз — подтверждает ту же замену.
        assert!(
            core.edit_segment_text(meeting_id.clone(), 1, 0, "зашли на intra.ru".into())
                .is_empty()
        );

        let terms = core.list_glossary_terms();
        assert_eq!(terms.len(), 1, "повторная правка не плодит терминов");
        assert_eq!(terms[0].id, term_id, "идентификатор термина не протухает");
        assert!(
            matches!(terms[0].kind, FfiGlossaryKind::Replacement),
            "замену понижает только человек"
        );

        // Возврат к распознанному — это отмена, а не ещё одна правка.
        assert!(
            core.edit_segment_text(meeting_id.clone(), 1, 0, recognized.into())
                .is_empty()
        );
        let segments = core.list_final_segments(meeting_id.clone(), 1);
        assert_eq!(segments[0].text, recognized);
        assert!(!segments[0].text_edited);
        let store = AudioManifestStore::open(&root).expect("store");
        assert!(
            store
                .list_segment_edits(&meeting_id)
                .expect("журнал")
                .is_empty(),
            "отменённая правка уходит из журнала"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Вид записи ходит через границу в обе стороны.
    ///
    /// Экран словаря отправляет обратно то, что получил из списка. Пока
    /// вида в DTO не было, первое же сохранение автоматически рождённой
    /// подсказки делало её глобальной заменой, переписывающей все будущие
    /// тексты, — ровно тот сценарий, ради которого виды и разделили.
    #[test]
    fn glossary_kind_survives_the_ffi_round_trip() {
        let root = edits_root("term-kind");
        let meeting_id = "m-term-kind".to_string();
        seed_segment_version(&root, &meeting_id, 1, &[(0, 0, 1_000, "зашли на интра ру")]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.edit_segment_text(meeting_id.clone(), 1, 0, "зашли на intra.ru".into())
                .is_empty()
        );

        let from_list = core.list_glossary_terms();
        assert_eq!(from_list.len(), 1);
        assert!(matches!(from_list[0].kind, FfiGlossaryKind::Hint));

        // Экран словаря сохраняет то же, что показал.
        let error = core.upsert_glossary_term(from_list[0].clone());
        assert!(error.is_empty(), "{error}");

        let after = core.list_glossary_terms();
        assert!(
            matches!(after[0].kind, FfiGlossaryKind::Hint),
            "подсказка не должна становиться заменой сама по себе"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Термин чужой встречи в этой не действует: массовая замена по нему
    /// наплодила бы правки там, где он не применяется.
    #[test]
    fn promote_rejects_a_term_of_another_meeting() {
        let root = edits_root("promote-scope");
        let meeting_id = "m-promote-here".to_string();
        seed_segment_version(&root, &meeting_id, 1, &[(0, 0, 1_000, "открой интра ру")]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let error = core.upsert_glossary_term(FfiGlossaryTerm {
            id: "foreign".into(),
            surface: "интра ру".into(),
            canonical: "intra.ru".into(),
            language: "ru".into(),
            scope: FfiGlossaryScope::Meeting,
            meeting_id: "m-promote-there".into(),
            kind: FfiGlossaryKind::Hint,
        });
        assert!(error.is_empty(), "{error}");

        let refused = core.promote_term_to_replacement("foreign".into(), meeting_id.clone(), 1);
        assert!(
            refused.contains("не действует"),
            "чужой термин должен быть отвергнут: {refused}"
        );

        let store = AudioManifestStore::open(&root).expect("store");
        assert!(
            store
                .list_segment_edits(&meeting_id)
                .expect("журнал")
                .is_empty(),
            "отказ не должен оставлять правок"
        );
        let terms = core.list_glossary_terms();
        assert!(
            matches!(terms[0].kind, FfiGlossaryKind::Hint),
            "отказ не должен менять вид термина"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Сбой чтения сегментов — это ошибка, а не «сегмента нет».
    #[test]
    fn storage_failure_is_not_reported_as_a_missing_segment() {
        let root = edits_root("storage-error-read");
        let meeting_id = "m-storage-error-read".to_string();
        seed_segment_version(&root, &meeting_id, 1, &[(0, 0, 1_000, "открой интра ру")]);

        // Портим колонку так, что разбор строки обязан упасть.
        {
            let connection = rusqlite::Connection::open(root.join("meetingraft.sqlite3"))
                .expect("прямое соединение");
            connection
                .execute("UPDATE final_segments SET start_ms = 'сломано'", [])
                .expect("порча");
        }

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        let error = core.edit_segment_text(meeting_id.clone(), 1, 0, "что-то".into());

        assert!(
            !error.is_empty() && !error.contains("не найден"),
            "сбой базы выдан за отсутствующий сегмент: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Сбой чтения журнала не может выглядеть успешной заменой.
    ///
    /// Пустой журнал вместо ошибки — это не только молчание: массовая
    /// замена решает по журналу, какие места человек уже поправил руками,
    /// и, не увидев их, переписала бы ручную работу поверх.
    #[test]
    fn storage_failure_does_not_look_like_a_successful_replacement() {
        let root = edits_root("storage-error-journal");
        let meeting_id = "m-storage-error-journal".to_string();
        seed_segment_version(&root, &meeting_id, 1, &[(0, 0, 1_000, "открой интра ру")]);

        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
        assert!(
            core.edit_segment_text(meeting_id.clone(), 1, 0, "открой intra.ru".into())
                .is_empty()
        );
        let term_id = core.list_glossary_terms()[0].id.clone();

        // Битая строка чужой версии: чтение сегментов версии 1 её не
        // касается, а чтение всего журнала обязано на ней упасть.
        {
            let connection = rusqlite::Connection::open(root.join("meetingraft.sqlite3"))
                .expect("прямое соединение");
            connection
                .execute(
                    "INSERT INTO segment_edits
                     (id, meeting_id, channel, start_ms, end_ms, original_text,
                      edited_text, created_at_ms, applied_version)
                     VALUES ('broken', ?1, 'mic', 'сломано', 1, '', '', 0, 2)",
                    rusqlite::params![meeting_id],
                )
                .expect("порча");
        }

        let error = core.promote_term_to_replacement(term_id, meeting_id.clone(), 1);

        assert!(
            !error.is_empty(),
            "замена не может завершиться успехом, не прочитав журнал"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Назначение через фасад видно в списке сегментов и в сводке.
    #[test]
    fn speaker_assignment_is_visible_through_the_facade() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-assign-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-assign".to_string();
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            let segments = vec![
                domain::FinalSegment {
                    index: 0,
                    start_ms: 0,
                    end_ms: 3_000,
                    channel: AudioChannel::Mic,
                    speaker_id: String::new(),
                    speaker_pinned: false,
                    text: "я говорю".into(),
                    text_edited: false,
                    original_text: String::new(),
                },
                domain::FinalSegment {
                    index: 1,
                    start_ms: 3_000,
                    end_ms: 4_000,
                    channel: AudioChannel::System,
                    speaker_id: String::new(),
                    speaker_pinned: false,
                    text: "они отвечают".into(),
                    text_edited: false,
                    original_text: String::new(),
                },
            ];
            store
                .replace_final_segments(&meeting_id, 1, &segments)
                .expect("segments");
            store
                .upsert_speaker(&domain::Speaker {
                    id: "sp-peter".into(),
                    meeting_id: meeting_id.clone(),
                    display_name: "Пётр".into(),
                    sort_index: 1,
                })
                .expect("speaker");
        }
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        assert!(
            core.assign_channel_speaker(meeting_id.clone(), 1, "system".into(), "sp-peter".into())
                .is_empty()
        );

        let segments = core.list_final_segments(meeting_id.clone(), 1);
        assert_eq!(segments.len(), 2);
        assert!(segments[0].speaker_name.is_empty(), "микрофон не тронут");
        assert_eq!(segments[1].speaker_name, "Пётр");
        assert!(!segments[1].speaker_pinned);

        // Точечная правка перекрывает канал и переживает его повторное
        // назначение.
        assert!(
            core.assign_segment_speaker(meeting_id.clone(), 1, 1, String::new())
                .is_empty()
        );
        assert!(
            core.assign_channel_speaker(meeting_id.clone(), 1, "system".into(), "sp-peter".into())
                .is_empty()
        );
        let after = core.list_final_segments(meeting_id.clone(), 1);
        assert!(after[1].speaker_id.is_empty(), "правка затёрта");
        assert!(after[1].speaker_pinned);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Правка атрибуции обязана дойти до markdown: он и уходит в экспорт
    /// и в Brief, а расходится с экраном молча.
    #[test]
    fn attribution_edits_rewrite_final_markdown() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-render-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-render".to_string();
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store
                .replace_final_segments(
                    &meeting_id,
                    1,
                    &[domain::FinalSegment {
                        index: 0,
                        start_ms: 0,
                        end_ms: 2_000,
                        channel: AudioChannel::System,
                        speaker_id: String::new(),
                        speaker_pinned: false,
                        text: "нужно решить к пятнице".into(),
                        text_edited: false,
                        original_text: String::new(),
                    }],
                )
                .expect("segments");
            store
                .upsert_final_transcript(&FinalTranscript {
                    meeting_id: meeting_id.clone(),
                    version: 1,
                    body_markdown: "нужно решить к пятнице".into(),
                    created_at_ms: 111,
                })
                .expect("transcript");
            store
                .upsert_speaker(&domain::Speaker {
                    id: "sp-peter".into(),
                    meeting_id: meeting_id.clone(),
                    display_name: "Пётр".into(),
                    sort_index: 0,
                })
                .expect("speaker");
        }
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        assert!(
            core.assign_channel_speaker(meeting_id.clone(), 1, "system".into(), "sp-peter".into())
                .is_empty()
        );
        assert_eq!(
            core.get_final_transcript_version(meeting_id.clone(), 1)
                .body_markdown,
            "**Пётр:** нужно решить к пятнице"
        );

        // Переименование участника — тоже правка атрибуции.
        assert!(
            core.upsert_speaker(meeting_id.clone(), "sp-peter".into(), "Пётр И.".into(), 0)
                .is_empty()
        );
        let renamed = core.get_final_transcript_version(meeting_id.clone(), 1);
        assert_eq!(renamed.body_markdown, "**Пётр И.:** нужно решить к пятнице");
        assert_eq!(renamed.created_at_ms, 111, "правка имени не создаёт версию");

        // Удаление снимает подпись, а не текст.
        assert!(
            core.delete_speaker(meeting_id.clone(), "sp-peter".into())
                .is_empty()
        );
        assert_eq!(
            core.get_final_transcript_version(meeting_id.clone(), 1)
                .body_markdown,
            "нужно решить к пятнице"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Журнал должен быть управляем целиком: путь виден, выключение
    /// стирает прошлое. Иначе «выключено» означало бы «не пишем новое, но
    /// старое лежит».
    #[test]
    fn diagnostics_log_is_local_and_erasable() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-diag-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        assert!(core.is_diagnostics_log_enabled());
        let path = core.diagnostics_log_path();
        assert!(path.starts_with(root.to_string_lossy().as_ref()));

        std::fs::create_dir_all(&root).expect("root");
        std::fs::write(&path, "{}\n").expect("write");
        assert!(core.diagnostics_log_size_bytes() > 0);

        core.set_diagnostics_log_enabled(false);
        assert!(!core.is_diagnostics_log_enabled());
        assert_eq!(core.diagnostics_log_size_bytes(), 0, "прошлое тоже стёрто");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Звук реплики должен приходить с её собственной дорожки и в
    /// границах её времени: слушают конкретное слово конкретного
    /// человека, а не отрезок встречи.
    #[test]
    fn segment_audio_returns_the_range_of_its_own_channel() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-audio-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-audio".to_string();
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store.begin_session(&meeting_id, 1, "").expect("session");
            for index in 0..10u64 {
                let sample = (index as i16) + 1;
                let bytes: Vec<u8> = (0..1_600).flat_map(|_| sample.to_le_bytes()).collect();
                store
                    .append_chunk(AudioChannel::Mic, &bytes, 16_000, index * 100)
                    .expect("chunk");
            }
        }
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        let fragment = core.segment_audio(meeting_id.clone(), "mic".into(), 100, 300);
        assert_eq!(fragment.sample_rate, 16_000);
        assert_eq!(fragment.duration_ms, 200);
        assert_eq!(fragment.pcm.len(), 200 * 16 * 2, "i16 little-endian");

        // На системной дорожке этой встречи не записано ничего.
        let other = core.segment_audio(meeting_id, "system".into(), 100, 300);
        assert_eq!(other.duration_ms, 0);
        assert!(other.pcm.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn speaker_stats_report_share_of_speech() {
        let root = std::env::temp_dir().join(format!(
            "mr-ffi-stats-{}-{:?}",
            now_ms(),
            std::thread::current().id()
        ));
        let meeting_id = "m-stats".to_string();
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store
                .replace_final_segments(
                    &meeting_id,
                    1,
                    &[domain::FinalSegment {
                        index: 0,
                        start_ms: 0,
                        end_ms: 4_000,
                        channel: AudioChannel::Mic,
                        speaker_id: "sp".into(),
                        speaker_pinned: false,
                        text: "текст".into(),
                        text_edited: false,
                        original_text: String::new(),
                    }],
                )
                .expect("segments");
            store
                .upsert_speaker(&domain::Speaker {
                    id: "sp".into(),
                    meeting_id: meeting_id.clone(),
                    display_name: "Вы".into(),
                    sort_index: 0,
                })
                .expect("speaker");
        }
        let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());

        let stats = core.list_speaker_stats(meeting_id, 1);

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].display_name, "Вы");
        assert_eq!(stats[0].speaking_ms, 4_000);
        assert!((stats[0].share - 1.0).abs() < 0.001);
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
            kind: FfiGlossaryKind::Replacement,
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
        assert!(
            core.delete_speaker("m1".into(), list[0].id.clone())
                .is_empty()
        );
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
                kind: FfiGlossaryKind::Replacement,
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
                kind: FfiGlossaryKind::Replacement,
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
