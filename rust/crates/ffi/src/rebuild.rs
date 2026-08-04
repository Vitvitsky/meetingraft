//! Сам проход пересбора Final (Phase 10, T6).
//!
//! Оркестрация живёт здесь, потому что собирает три крейта: хранилище,
//! распознавание и post-call сборку. Работа выполняется в потоке и
//! **не держит мьютекс ядра**: проход идёт минутами, и блокировать на это
//! время весь UI недопустимо. Своё соединение с SQLite поток открывает
//! сам — WAL и busy_timeout это выдерживают.

use std::path::PathBuf;

use domain::{
    AudioChannel, FinalSegment, FinalTranscript, LanguagePolicy, Speaker, TranscriptSegment,
};
use glossary::{GlossaryEngine, active_terms};
use postcall::{
    DEFAULT_BATCH_SIZE, JobHandle, LlmClient, NullLlmClient, OllamaNativeClient,
    OpenAiCompatLlmClient, PolishReport, merge_channels, polish_segments, reattach_edits,
    render_segments,
};
use storage::AudioManifestStore;
#[cfg(not(feature = "whisper"))]
use stt::MockBatchTranscriber;
use stt::{BatchTranscribeError, BatchTranscriber};

/// Всё, что проходу нужно знать. Снимается с ядра **до** старта потока.
#[derive(Debug, Clone)]
pub struct RebuildParams {
    pub data_root: PathBuf,
    pub meeting_id: String,
    pub policy: LanguagePolicy,
    pub post_call_model: String,
    pub llm_engine: String,
    pub llm_base_url: String,
    pub llm_model_id: String,
    /// Имена спикеров по умолчанию; приходят из презентационного слоя,
    /// как и название встречи, — формулировка локале-зависима.
    pub mic_speaker_name: String,
    pub system_speaker_name: String,
}

/// Доли прогресса: распознавание — основная стоимость прохода.
const TRANSCRIBE_SHARE: f32 = 0.8;
const PROGRESS_STEPS: u32 = 100;

pub fn run_rebuild(params: RebuildParams, handle: &JobHandle) -> Result<(), String> {
    let mut store = AudioManifestStore::open(&params.data_root).map_err(|e| e.to_string())?;

    let mic = store
        .read_session_pcm(&params.meeting_id, AudioChannel::Mic)
        .map_err(|e| e.to_string())?;
    let system = store
        .read_session_pcm(&params.meeting_id, AudioChannel::System)
        .map_err(|e| e.to_string())?;
    if mic.is_empty() && system.is_empty() {
        return Err("нет сохранённого аудио встречи".to_string());
    }

    let terms = store.list_glossary_terms().map_err(|e| e.to_string())?;
    let glossary = GlossaryEngine::from_terms(active_terms(&terms, Some(&params.meeting_id)));
    let prompt = glossary.build_whisper_prompt(800);

    let (mut transcriber, engine_note) = open_transcriber(&params, &prompt)?;

    // Дорожки распознаются раздельно — только так канал сегмента известен
    // точно, без эвристики доминирования из live (ADR-009).
    let mic_segments = transcribe_track(&mut *transcriber, &mic, handle, 0.0)?;
    let system_segments =
        transcribe_track(&mut *transcriber, &system, handle, TRANSCRIBE_SHARE / 2.0)?;

    if handle.is_cancelled() {
        return Err("cancelled".to_string());
    }
    report(handle, TRANSCRIBE_SHARE);

    let merged = merge_channels(mic_segments, system_segments);
    if merged.is_empty() {
        return Err("распознавание не дало ни одного сегмента".to_string());
    }

    let (segments, polish) = polish_segments(
        merged,
        params.policy.primary,
        DEFAULT_BATCH_SIZE,
        &*llm_client(&params),
    );
    report(handle, 0.95);

    let version = store
        .next_final_version(&params.meeting_id)
        .map_err(|e| e.to_string())?;
    store
        .replace_final_segments(&params.meeting_id, version, &segments)
        .map_err(|e| e.to_string())?;

    // Журнал правок переезжает на новую нарезку здесь и только здесь:
    // сегменты пересбор создаёт заново, а ручная работа живёт отдельно и
    // сама на них не переберётся. Без этого шага правки остались бы с
    // `applied_version` старой версии — в новой их не видно, и в списке
    // неприменившихся тоже, потому что `NULL` никто не выставляет.
    // «Ничего не пропадает молча» — главное требование механики.
    reattach_meeting_edits(&mut store, &params.meeting_id, version, &segments)?;

    // Спикеры назначаются до рендера: markdown производен от сегментов,
    // и собранный раньше он остался бы без имён (Phase 11, T6).
    let speakers = assign_default_speakers(&mut store, &params, version, &segments)?;
    let attributed = store
        .list_final_segments(&params.meeting_id, version)
        .map_err(|e| e.to_string())?;
    store
        .upsert_final_transcript(&FinalTranscript {
            meeting_id: params.meeting_id.clone(),
            version,
            body_markdown: render_segments(&attributed, &speakers),
            created_at_ms: crate::now_ms(),
        })
        .map_err(|e| e.to_string())?;

    handle.set_note(provenance(&engine_note, &polish));
    report(handle, 1.0);
    Ok(())
}

/// Пересадить ручные правки встречи на сегменты новой версии.
///
/// Берётся весь журнал, а не только правки предыдущей версии: правка,
/// не нашедшая места в прошлый раз, получает новый шанс — модель могла
/// распознать это место так же, как в тот раз, когда её вводили.
fn reattach_meeting_edits(
    store: &mut AudioManifestStore,
    meeting_id: &str,
    version: u32,
    segments: &[FinalSegment],
) -> Result<(), String> {
    let edits = store
        .list_segment_edits(meeting_id)
        .map_err(|e| e.to_string())?;
    for edit in reattach_edits(&edits, segments, version) {
        store
            .upsert_segment_edit(&edit)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Завести спикеров по умолчанию и раздать их по каналам.
///
/// Для звонка один на один это и есть вся атрибуция: канал `mic` —
/// владелец машины, `system` — собеседник. Дальше человеку остаётся
/// только заменить имя.
/// Возвращает участников встречи после назначения — их имена нужны
/// рендеру, а `ensure_speaker` мог оставить прежнее имя, данное человеком.
fn assign_default_speakers(
    store: &mut AudioManifestStore,
    params: &RebuildParams,
    version: u32,
    segments: &[FinalSegment],
) -> Result<Vec<Speaker>, String> {
    for (channel, name) in [
        (AudioChannel::Mic, &params.mic_speaker_name),
        (AudioChannel::System, &params.system_speaker_name),
    ] {
        // Канала не было — спикера не заводим: пустой «Собеседник» в
        // списке участников это мусор, а не информация.
        if !segments.iter().any(|segment| segment.channel == channel) {
            continue;
        }
        let speaker = Speaker::default_for(&params.meeting_id, channel, name);
        store.ensure_speaker(&speaker).map_err(|e| e.to_string())?;
        store
            .set_channel_speaker(&params.meeting_id, version, channel, &speaker.id)
            .map_err(|e| e.to_string())?;
    }
    store
        .list_speakers(&params.meeting_id)
        .map_err(|e| e.to_string())
}

/// Что фактически отработало. Полировка попадает сюда только если прошла
/// целиком: наполовину причёсанный текст не «отполирован».
fn provenance(engine_note: &str, polish: &PolishReport) -> String {
    if polish.is_complete() {
        format!("{engine_note} + LLM polish")
    } else if let Some(reason) = polish.error.as_deref() {
        format!("{engine_note} (без полировки: {reason})")
    } else {
        engine_note.to_string()
    }
}

/// Модель качается по требованию, поэтому её отсутствие — не сбой
/// прохода, а внятное сообщение пользователю.
#[cfg(feature = "whisper")]
fn open_transcriber(
    params: &RebuildParams,
    prompt: &str,
) -> Result<(Box<dyn BatchTranscriber>, String), String> {
    use stt::WhisperBatchTranscriber;

    let mut transcriber = WhisperBatchTranscriber::open(
        &params.data_root,
        &params.post_call_model,
        params.policy.clone(),
    )
    .map_err(|error| match error {
        BatchTranscribeError::ModelMissing { model_id } => format!("модель {model_id} не скачана"),
        other => other.to_string(),
    })?;
    transcriber.set_initial_prompt(prompt);
    Ok((
        Box::new(transcriber),
        format!("re-ASR {}", params.post_call_model),
    ))
}

/// Без Whisper проход всё равно проходим целиком — иначе весь путь
/// post-call был бы непроверяемым вне Mac. В note честно сказано, что
/// распознавания не было.
#[cfg(not(feature = "whisper"))]
fn open_transcriber(
    params: &RebuildParams,
    _prompt: &str,
) -> Result<(Box<dyn BatchTranscriber>, String), String> {
    Ok((
        Box::new(MockBatchTranscriber::new()),
        format!("mock re-ASR (вместо {})", params.post_call_model),
    ))
}

fn transcribe_track(
    transcriber: &mut dyn BatchTranscriber,
    pcm: &[i16],
    handle: &JobHandle,
    progress_from: f32,
) -> Result<Vec<TranscriptSegment>, String> {
    if pcm.is_empty() {
        return Ok(Vec::new());
    }
    let span = TRANSCRIBE_SHARE / 2.0;
    let mut on_progress = |value: f32| {
        if handle.is_cancelled() {
            return false;
        }
        // Пока идёт запись, живое распознавание важнее фонового прохода.
        while handle.should_yield() && !handle.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        report(handle, progress_from + value * span);
        true
    };
    match transcriber.transcribe_all(pcm, 16_000, &mut on_progress) {
        Ok(segments) => Ok(segments),
        Err(BatchTranscribeError::Cancelled) => Err("cancelled".to_string()),
        Err(error) => Err(error.to_string()),
    }
}

fn report(handle: &JobHandle, fraction: f32) {
    let done = (fraction.clamp(0.0, 1.0) * PROGRESS_STEPS as f32) as u32;
    handle.report(done, PROGRESS_STEPS);
}

fn llm_client(params: &RebuildParams) -> Box<dyn LlmClient> {
    match params.llm_engine.as_str() {
        "ollama" => Box::new(OllamaNativeClient::new(
            params.llm_base_url.clone(),
            params.llm_model_id.clone(),
        )),
        "openai_compat" => Box::new(OpenAiCompatLlmClient::new(
            params.llm_base_url.clone(),
            params.llm_model_id.clone(),
        )),
        // Шаблоны и backend-джобы полировать сегменты не умеют; молча
        // подменять их чем-то другим нельзя, поэтому просто не полируем.
        _ => Box::new(NullLlmClient),
    }
}
