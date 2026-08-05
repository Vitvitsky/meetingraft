//! SQLite audio_manifest + файлы чанков на диске.

use std::fs;
use std::path::{Path, PathBuf};

use domain::FinalSegment;
use domain::{
    Artifact, ArtifactKind, AudioChannel, CaptionEvent, CaptionPhase, FinalTranscript,
    GlossaryKind, GlossaryScope, GlossaryTerm, MeetingSummary, SearchHit, SearchHitKind, Speaker,
    SpeechLanguage, edits_by_position,
};
use rusqlite::{Connection, params};
use thiserror::Error;

/// Ошибки store.
#[derive(Debug, Error)]
pub enum AudioManifestError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("session not open")]
    SessionNotOpen,
    #[error("meeting not found: {0}")]
    MeetingNotFound(String),
    #[error("meeting is being recorded: {0}")]
    SessionActive(String),
    #[error("segment not found: {meeting_id} v{version} #{index}")]
    SegmentNotFound {
        meeting_id: String,
        version: u32,
        index: u32,
    },
    #[error("chunk {path} truncated: expected {expected_frames} frames, got {actual_frames}")]
    ChunkTruncated {
        path: String,
        expected_frames: usize,
        actual_frames: usize,
    },
}

/// Максимальная длительность выдаваемого фрагмента.
///
/// Фрагмент едет через границу UniFFI целиком; час записи это больше
/// сотни мегабайт, а слушают всегда одну реплику.
const MAX_FRAGMENT_MS: u64 = 30_000;

/// Кусок записи с частотой дискретизации, на которой он записан.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PcmFragment {
    pub pcm: Vec<i16>,
    /// 0 — в диапазоне ничего не нашлось.
    pub sample_rate: u32,
}

impl PcmFragment {
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        self.pcm.len() as u64 * 1000 / u64::from(self.sample_rate)
    }
}

/// Номер отсчёта по смещению внутри чанка.
fn frame_index(offset_ms: u64, sample_rate: u32) -> usize {
    (offset_ms * u64::from(sample_rate) / 1000) as usize
}

/// Строка manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestChunk {
    pub session_id: String,
    pub channel: AudioChannel,
    pub seq: u32,
    pub path: PathBuf,
    pub sample_rate: u32,
    pub frame_count: u32,
    pub timestamp_ms: u64,
}

/// Store: SQLite + PCM files под `root/sessions/{id}/{channel}/`.
pub struct AudioManifestStore {
    root: PathBuf,
    conn: Connection,
    active_session: Option<String>,
    next_seq: [u32; 2],
}

impl AudioManifestStore {
    /// Соединение для модулей крейта, живущих в соседних файлах.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Открыть/создать БД в `root/meetingraft.sqlite3`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, AudioManifestError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let db_path = root.join("meetingraft.sqlite3");
        let conn = Connection::open(&db_path)?;
        // Снижает flaky SQLITE_BUSY при параллельных тестах / WAL checkpoint.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Вне migrate: journal_mode не выполняется внутри транзакции.
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        crate::migrations::migrate(&conn)?;
        Ok(Self {
            root,
            conn,
            active_session: None,
            next_seq: [0, 0],
        })
    }

    /// Начать recording session. Пустое название допустимо.
    pub fn begin_session(
        &mut self,
        session_id: &str,
        started_at_ms: u64,
        title: &str,
    ) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (id, started_at_ms, title, ended_at_ms)
             VALUES (?1, ?2, ?3, NULL)",
            params![session_id, started_at_ms as i64, title],
        )?;
        for channel in [AudioChannel::Mic, AudioChannel::System] {
            fs::create_dir_all(self.chunk_dir(session_id, channel))?;
        }
        self.active_session = Some(session_id.to_string());
        self.next_seq = [0, 0];
        Ok(())
    }

    /// Закончить session: записать время окончания и сбросить active.
    pub fn end_session(&mut self, ended_at_ms: u64) -> Result<(), AudioManifestError> {
        if let Some(session_id) = self.active_session.take() {
            self.conn.execute(
                "UPDATE sessions SET ended_at_ms = ?2 WHERE id = ?1",
                params![session_id, ended_at_ms as i64],
            )?;
        }
        Ok(())
    }

    /// Переименовать встречу. Неизвестный id — ошибка, а не тихий no-op.
    pub fn set_meeting_title(
        &mut self,
        meeting_id: &str,
        title: &str,
    ) -> Result<(), AudioManifestError> {
        let updated = self.conn.execute(
            "UPDATE sessions SET title = ?2 WHERE id = ?1",
            params![meeting_id, title],
        )?;
        if updated == 0 {
            return Err(AudioManifestError::MeetingNotFound(meeting_id.to_owned()));
        }
        Ok(())
    }

    /// Записать PCM chunk на диск и в manifest.
    pub fn append_chunk(
        &mut self,
        channel: AudioChannel,
        pcm: &[u8],
        sample_rate: u32,
        timestamp_ms: u64,
    ) -> Result<ManifestChunk, AudioManifestError> {
        let session_id = self
            .active_session
            .clone()
            .ok_or(AudioManifestError::SessionNotOpen)?;
        let idx = match channel {
            AudioChannel::Mic => 0,
            AudioChannel::System => 1,
        };
        let seq = self.next_seq[idx];
        self.next_seq[idx] = seq + 1;

        let rel = format!(
            "sessions/{}/{}/{:06}.pcm",
            session_id,
            channel.dir_name(),
            seq
        );
        let abs = self.root.join(&rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs, pcm)?;

        let frame_count = (pcm.len() / 2) as u32;
        self.conn.execute(
            "INSERT INTO audio_manifest
             (session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session_id,
                channel.dir_name(),
                seq,
                rel,
                sample_rate,
                frame_count,
                timestamp_ms as i64
            ],
        )?;

        Ok(ManifestChunk {
            session_id,
            channel,
            seq,
            path: abs,
            sample_rate,
            frame_count,
            timestamp_ms,
        })
    }

    /// Число чанков в session (оба канала).
    pub fn chunk_count(&self, session_id: &str) -> Result<u64, AudioManifestError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM audio_manifest WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Список чанков session.
    pub fn list_chunks(&self, session_id: &str) -> Result<Vec<ManifestChunk>, AudioManifestError> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms
             FROM audio_manifest WHERE session_id = ?1 ORDER BY channel, seq",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let channel_str: String = row.get(1)?;
            let channel = match channel_str.as_str() {
                "system" => AudioChannel::System,
                _ => AudioChannel::Mic,
            };
            let rel: String = row.get(3)?;
            Ok(ManifestChunk {
                session_id: row.get(0)?,
                channel,
                seq: row.get::<_, i64>(2)? as u32,
                path: self.root.join(rel),
                sample_rate: row.get::<_, i64>(4)? as u32,
                frame_count: row.get::<_, i64>(5)? as u32,
                timestamp_ms: row.get::<_, i64>(6)? as u64,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Сохранить live caption event.
    pub fn append_caption(
        &mut self,
        session_id: &str,
        event: &domain::CaptionEvent,
        created_at_ms: u64,
    ) -> Result<(), AudioManifestError> {
        let phase = match event.phase {
            domain::CaptionPhase::Partial => "partial",
            domain::CaptionPhase::Final => "final",
        };
        self.conn.execute(
            "INSERT INTO caption_events (id, session_id, text, phase, created_at_ms, channel)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id,
                session_id,
                event.text,
                phase,
                created_at_ms as i64,
                event.channel.code()
            ],
        )?;
        if event.phase == domain::CaptionPhase::Final {
            Self::index_text(&self.conn, session_id, "caption", &event.id, &event.text)?;
        }
        Ok(())
    }

    /// Записать текст в поисковый индекс, заменив прежнюю версию.
    fn index_text(
        conn: &Connection,
        meeting_id: &str,
        kind: &str,
        ref_id: &str,
        body: &str,
    ) -> Result<(), AudioManifestError> {
        conn.execute(
            "DELETE FROM meeting_fts WHERE meeting_id = ?1 AND kind = ?2 AND ref_id = ?3",
            params![meeting_id, kind, ref_id],
        )?;
        conn.execute(
            "INSERT INTO meeting_fts (meeting_id, kind, ref_id, body)
             VALUES (?1, ?2, ?3, ?4)",
            params![meeting_id, kind, ref_id, body],
        )?;
        Ok(())
    }

    /// Число caption events session.
    pub fn caption_count(&self, session_id: &str) -> Result<u64, AudioManifestError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM caption_events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Вернуть caption events сессии в хронологическом порядке.
    pub fn list_captions(&self, session_id: &str) -> Result<Vec<CaptionEvent>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT id, text, phase, channel
             FROM caption_events
             WHERE session_id = ?1
             ORDER BY created_at_ms, id",
        )?;
        let rows = statement.query_map(params![session_id], |row| {
            let phase: String = row.get(2)?;
            let channel: String = row.get(3)?;
            Ok(CaptionEvent {
                id: row.get(0)?,
                text: row.get(1)?,
                phase: Self::parse_caption_phase(&phase)?,
                channel: AudioChannel::from_code(&channel),
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Сохранить или заменить версию финального транскрипта.
    pub fn upsert_final_transcript(
        &mut self,
        transcript: &FinalTranscript,
    ) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "INSERT INTO final_transcripts
             (meeting_id, version, body_markdown, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(meeting_id, version) DO UPDATE SET
                 body_markdown = excluded.body_markdown,
                 created_at_ms = excluded.created_at_ms",
            params![
                transcript.meeting_id,
                transcript.version,
                transcript.body_markdown,
                transcript.created_at_ms as i64
            ],
        )?;
        Self::index_text(
            &self.conn,
            &transcript.meeting_id,
            "final",
            &transcript.version.to_string(),
            &transcript.body_markdown,
        )?;
        Ok(())
    }

    /// Следующий номер версии финального транскрипта (MAX+1 или 1).
    pub fn next_final_version(&self, meeting_id: &str) -> Result<u32, AudioManifestError> {
        let next: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1
             FROM final_transcripts
             WHERE meeting_id = ?1",
            params![meeting_id],
            |row| row.get(0),
        )?;
        Ok(next as u32)
    }

    /// Все версии финального транскрипта встречи (новые первыми).
    pub fn list_final_transcripts(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<FinalTranscript>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT meeting_id, version, body_markdown, created_at_ms
             FROM final_transcripts
             WHERE meeting_id = ?1
             ORDER BY version DESC",
        )?;
        let rows = statement.query_map(params![meeting_id], Self::map_final_transcript_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Конкретная версия финального транскрипта или None.
    pub fn get_final_transcript_version(
        &self,
        meeting_id: &str,
        version: u32,
    ) -> Result<Option<FinalTranscript>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT meeting_id, version, body_markdown, created_at_ms
             FROM final_transcripts
             WHERE meeting_id = ?1 AND version = ?2",
        )?;
        let mut rows = statement.query(params![meeting_id, version])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(Self::map_final_transcript_row(row)?))
    }

    /// Вернуть последнюю версию финального транскрипта встречи.
    pub fn get_final_transcript(
        &self,
        meeting_id: &str,
    ) -> Result<Option<FinalTranscript>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT meeting_id, version, body_markdown, created_at_ms
             FROM final_transcripts
             WHERE meeting_id = ?1
             ORDER BY version DESC
             LIMIT 1",
        )?;
        let mut rows = statement.query(params![meeting_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(Self::map_final_transcript_row(row)?))
    }

    /// Сохранить post-call артефакт.
    pub fn insert_artifact(&mut self, artifact: &Artifact) -> Result<(), AudioManifestError> {
        let kind = match artifact.kind {
            ArtifactKind::Brief => "brief",
            ArtifactKind::FollowUp => "follow_up",
        };
        self.conn.execute(
            "INSERT INTO artifacts
             (id, meeting_id, kind, template_id, body_markdown, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                artifact.id,
                artifact.meeting_id,
                kind,
                artifact.template_id,
                artifact.body_markdown,
                artifact.created_at_ms as i64
            ],
        )?;
        Self::index_text(
            &self.conn,
            &artifact.meeting_id,
            "artifact",
            &artifact.id,
            &artifact.body_markdown,
        )?;
        Ok(())
    }

    /// Вернуть артефакты встречи в хронологическом порядке.
    pub fn list_artifacts(&self, meeting_id: &str) -> Result<Vec<Artifact>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT id, meeting_id, kind, template_id, body_markdown, created_at_ms
             FROM artifacts
             WHERE meeting_id = ?1
             ORDER BY created_at_ms, id",
        )?;
        let rows = statement.query_map(params![meeting_id], |row| {
            let kind: String = row.get(2)?;
            Ok(Artifact {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                kind: Self::parse_artifact_kind(&kind)?,
                template_id: row.get(3)?,
                body_markdown: row.get(4)?,
                created_at_ms: row.get::<_, i64>(5)? as u64,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Вернуть встречи с признаками готового финала и числом артефактов.
    pub fn list_meeting_summaries(&self) -> Result<Vec<MeetingSummary>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT
                 sessions.id,
                 sessions.title,
                 sessions.started_at_ms,
                 sessions.ended_at_ms,
                 EXISTS(
                     SELECT 1
                     FROM final_transcripts
                     WHERE final_transcripts.meeting_id = sessions.id
                 ),
                 (
                     SELECT COUNT(*)
                     FROM artifacts
                     WHERE artifacts.meeting_id = sessions.id
                 )
             FROM sessions
             ORDER BY sessions.started_at_ms DESC, sessions.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(MeetingSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                started_at_ms: row.get::<_, i64>(2)? as u64,
                ended_at_ms: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
                has_final: row.get(4)?,
                artifact_count: row.get::<_, i64>(5)? as u64,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Заменить сегменты версии Final целиком.
    ///
    /// Именно замена, а не добавление: пересбор той же версии должен
    /// давать тот же результат, а не удваивать сегменты.
    pub fn replace_final_segments(
        &mut self,
        meeting_id: &str,
        version: u32,
        segments: &[FinalSegment],
    ) -> Result<(), AudioManifestError> {
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "DELETE FROM final_segments WHERE meeting_id = ?1 AND version = ?2",
            params![meeting_id, version],
        )?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO final_segments
                 (meeting_id, version, idx, start_ms, end_ms, channel, speaker_id,
                  speaker_pinned, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for segment in segments {
                statement.execute(params![
                    meeting_id,
                    version,
                    segment.index,
                    segment.start_ms as i64,
                    segment.end_ms as i64,
                    segment.channel.code(),
                    segment.speaker_id,
                    segment.speaker_pinned,
                    segment.text
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Сегменты версии Final в порядке следования, с наложенными поверх
    /// ручными правками из журнала `segment_edits` (Epic 19): текст берётся
    /// из таблицы `final_segments`, но там, где на эту версию легла правка,
    /// подменяется на правленый, а сегмент помечается `text_edited`.
    pub fn list_final_segments(
        &self,
        meeting_id: &str,
        version: u32,
    ) -> Result<Vec<FinalSegment>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT idx, start_ms, end_ms, channel, speaker_id, speaker_pinned, text
             FROM final_segments
             WHERE meeting_id = ?1 AND version = ?2
             ORDER BY idx",
        )?;
        let rows = statement.query_map(params![meeting_id, version], |row| {
            let channel: String = row.get(3)?;
            Ok(FinalSegment {
                index: row.get::<_, i64>(0)? as u32,
                start_ms: row.get::<_, i64>(1)? as u64,
                end_ms: row.get::<_, i64>(2)? as u64,
                channel: AudioChannel::from_code(&channel),
                speaker_id: row.get(4)?,
                speaker_pinned: row.get(5)?,
                text: row.get(6)?,
                text_edited: false,
                original_text: String::new(),
            })
        })?;
        let mut segments = rows.collect::<Result<Vec<_>, _>>()?;

        // Правка перекрывает распознанное: журнал — источник истины для
        // текста, таблица сегментов хранит то, что выдала модель.
        //
        // Из базы берём только правки этой версии (журнал не чистится при
        // пересборе и растёт без границы), а сверяем с сегментами по
        // хэш-карте, а не перебором: иначе на каждое открытие экрана
        // транскрипта уходило бы O(N · M_всего) вместо O(N + M_версии).
        let edits = self.list_segment_edits_for_version(meeting_id, version)?;
        // Правило «какая правка отвечает за это место» одно на весь проект
        // и живёт в домене: тремя разными правилами в трёх слоях правка
        // одной версии перехватывала правку другой.
        let by_position = edits_by_position(&edits, version);
        for segment in &mut segments {
            if let Some(edit) = by_position.get(&segment.position()) {
                segment.text = edit.edited_text.clone();
                segment.text_edited = true;
                segment.original_text = edit.original_text.clone();
            }
        }
        Ok(segments)
    }

    /// Назначить спикера всем сегментам канала.
    ///
    /// Основная операция фазы: после Phase 8 канал `mic` — это владелец
    /// машины, `system` — остальные, поэтому для звонка один на один
    /// одного вызова достаточно, чтобы весь транскрипт стал подписанным.
    ///
    /// Сегменты с ручной правкой не трогаются: иначе человек терял бы
    /// свои исправления при каждом переназначении, ничего об этом не
    /// узнав.
    pub fn set_channel_speaker(
        &mut self,
        meeting_id: &str,
        version: u32,
        channel: AudioChannel,
        speaker_id: &str,
    ) -> Result<u64, AudioManifestError> {
        let updated = self.conn.execute(
            "UPDATE final_segments
             SET speaker_id = ?4
             WHERE meeting_id = ?1 AND version = ?2 AND channel = ?3
               AND speaker_pinned = 0",
            params![meeting_id, version, channel.code(), speaker_id],
        )?;
        Ok(updated as u64)
    }

    /// Назначить спикера одной реплике и пометить её как правленую.
    pub fn set_segment_speaker(
        &mut self,
        meeting_id: &str,
        version: u32,
        index: u32,
        speaker_id: &str,
    ) -> Result<(), AudioManifestError> {
        let updated = self.conn.execute(
            "UPDATE final_segments
             SET speaker_id = ?4, speaker_pinned = 1
             WHERE meeting_id = ?1 AND version = ?2 AND idx = ?3",
            params![meeting_id, version, index, speaker_id],
        )?;
        if updated == 0 {
            return Err(AudioManifestError::SegmentNotFound {
                meeting_id: meeting_id.to_owned(),
                version,
                index,
            });
        }
        Ok(())
    }

    /// Снять ручную правку, вернув реплику под назначение по каналу.
    pub fn unpin_segment_speaker(
        &mut self,
        meeting_id: &str,
        version: u32,
        index: u32,
    ) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "UPDATE final_segments
             SET speaker_pinned = 0
             WHERE meeting_id = ?1 AND version = ?2 AND idx = ?3",
            params![meeting_id, version, index],
        )?;
        Ok(())
    }

    /// Собрать PCM канала за всю сессию в один поток.
    ///
    /// Каналы читаются раздельно: post-call проход (Phase 10) распознаёт
    /// их по отдельности, и тогда канал сегмента известен точно, без
    /// эвристики доминирования, которой пользуется live (ADR-009).
    ///
    /// Отсутствующий файл — ошибка, а не молчаливая тишина в середине
    /// записи: тишина исказила бы транскрипт незаметно.
    pub fn read_session_pcm(
        &self,
        session_id: &str,
        channel: AudioChannel,
    ) -> Result<Vec<i16>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT path, frame_count
             FROM audio_manifest
             WHERE session_id = ?1 AND channel = ?2
             ORDER BY seq",
        )?;
        let rows = statement.query_map(params![session_id, channel.dir_name()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;

        let mut pcm = Vec::new();
        for row in rows {
            let (relative, frame_count) = row?;
            let path = self.root.join(&relative);
            let bytes = fs::read(&path)?;
            if bytes.len() / 2 != frame_count {
                return Err(AudioManifestError::ChunkTruncated {
                    path: relative,
                    expected_frames: frame_count,
                    actual_frames: bytes.len() / 2,
                });
            }
            pcm.reserve(frame_count);
            pcm.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
            );
        }
        Ok(pcm)
    }

    /// Кусок записи по диапазону времени сегмента.
    ///
    /// Нужен, чтобы услышать спорную реплику перед правкой (Epic 19):
    /// исправлять распознанное на слух, не имея слуха, нельзя.
    ///
    /// Читаются только перекрывающие диапазон чанки. Читать всю дорожку
    /// ради трёх секунд означало бы поднимать с диска сотню мегабайт на
    /// каждое нажатие.
    pub fn read_pcm_range(
        &self,
        session_id: &str,
        channel: AudioChannel,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<PcmFragment, AudioManifestError> {
        if end_ms <= start_ms {
            return Ok(PcmFragment::default());
        }
        // Потолок против запроса «дай всю встречу»: фрагмент едет через
        // границу UniFFI целиком, и час записи это сотня мегабайт.
        let end_ms = end_ms.min(start_ms + MAX_FRAGMENT_MS);

        let mut statement = self.conn.prepare(
            "SELECT path, sample_rate, frame_count, timestamp_ms
             FROM audio_manifest
             WHERE session_id = ?1 AND channel = ?2
               AND timestamp_ms < ?3
               AND timestamp_ms + (frame_count * 1000 / sample_rate) > ?4
             ORDER BY seq",
        )?;
        let rows = statement.query_map(
            params![
                session_id,
                channel.dir_name(),
                end_ms as i64,
                start_ms as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u32,
                    row.get::<_, i64>(2)? as usize,
                    row.get::<_, i64>(3)? as u64,
                ))
            },
        )?;

        let mut fragment = PcmFragment::default();
        for row in rows {
            let (relative, sample_rate, frame_count, chunk_start_ms) = row?;
            if sample_rate == 0 {
                continue;
            }
            if fragment.sample_rate == 0 {
                fragment.sample_rate = sample_rate;
            }
            // Чанки с другой частотой пропускаем: пересэмплировать здесь
            // значило бы тихо менять звук, который человек пришёл сверить
            // с текстом.
            if sample_rate != fragment.sample_rate {
                continue;
            }

            let bytes = fs::read(self.root.join(&relative))?;
            let samples: Vec<i16> = bytes
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let available = samples.len().min(frame_count);

            let from =
                frame_index(start_ms.saturating_sub(chunk_start_ms), sample_rate).min(available);
            let to = frame_index(end_ms.saturating_sub(chunk_start_ms), sample_rate).min(available);
            if from < to {
                fragment.pcm.extend_from_slice(&samples[from..to]);
            }
        }
        Ok(fragment)
    }

    /// Удалить встречу целиком: строки БД, поисковый индекс и PCM-чанки
    /// на диске (`architecture.md`: удаление удаляет всё).
    ///
    /// Активную сессию удалить нельзя — сначала остановите запись.
    pub fn delete_meeting(&mut self, meeting_id: &str) -> Result<(), AudioManifestError> {
        if self.active_session.as_deref() == Some(meeting_id) {
            return Err(AudioManifestError::SessionActive(meeting_id.to_owned()));
        }

        let transaction = self.conn.transaction()?;
        let removed =
            transaction.execute("DELETE FROM sessions WHERE id = ?1", params![meeting_id])?;
        for statement in [
            "DELETE FROM audio_manifest WHERE session_id = ?1",
            "DELETE FROM caption_events WHERE session_id = ?1",
            "DELETE FROM final_transcripts WHERE meeting_id = ?1",
            "DELETE FROM final_segments WHERE meeting_id = ?1",
            // Журнал правок хранит дословные реплики в `original_text` и
            // `edited_text`: оставить его после удаления встречи — значит
            // оставить в базе сам разговор, который человек удалил.
            "DELETE FROM segment_edits WHERE meeting_id = ?1",
            "DELETE FROM artifacts WHERE meeting_id = ?1",
            "DELETE FROM speakers WHERE meeting_id = ?1",
            "DELETE FROM meeting_fts WHERE meeting_id = ?1",
        ] {
            transaction.execute(statement, params![meeting_id])?;
        }
        transaction.commit()?;

        if removed == 0 {
            return Err(AudioManifestError::MeetingNotFound(meeting_id.to_owned()));
        }

        // Файлы — после успешного коммита: иначе откат оставил бы строки
        // без чанков.
        let dir = self.root.join("sessions").join(meeting_id);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// Полнотекстовый поиск по материалам встреч.
    ///
    /// Запрос пользователя не является выражением FTS: кавычки, звёздочки
    /// и скобки в нём — обычные символы, поэтому каждое слово
    /// экранируется и превращается в префиксное.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>, AudioManifestError> {
        let Some(expression) = Self::to_fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut statement = self.conn.prepare(
            "SELECT meeting_id, kind, ref_id,
                    snippet(meeting_fts, 3, '[', ']', '…', 12)
             FROM meeting_fts
             WHERE meeting_fts MATCH ?1
             ORDER BY bm25(meeting_fts)
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![expression, limit], |row| {
            let kind: String = row.get(1)?;
            Ok(SearchHit {
                meeting_id: row.get(0)?,
                kind: SearchHitKind::from_code(&kind),
                ref_id: row.get(2)?,
                snippet: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// `биллинг счёт` → `"биллинг"* "счёт"*`; пустой запрос → `None`.
    fn to_fts_query(query: &str) -> Option<String> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|word| word.replace('"', "\"\""))
            .filter(|word| !word.is_empty())
            .map(|word| format!("\"{word}\"*"))
            .collect();
        if terms.is_empty() {
            return None;
        }
        Some(terms.join(" "))
    }

    /// Добавить или обновить термин по id либо уникальному ключу.
    pub fn upsert_glossary_term(
        &mut self,
        term: &GlossaryTerm,
        updated_at_ms: u64,
    ) -> Result<(), AudioManifestError> {
        let transaction = self.conn.transaction()?;
        Self::write_glossary_term(&transaction, term, updated_at_ms)?;
        transaction.commit()?;
        Ok(())
    }

    /// Удалить термин по id.
    pub fn delete_glossary_term(&mut self, id: &str) -> Result<(), AudioManifestError> {
        self.conn
            .execute("DELETE FROM glossary_terms WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Вернуть сохранённые термины в стабильном порядке.
    pub fn list_glossary_terms(&self) -> Result<Vec<GlossaryTerm>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT id, surface, canonical, language, scope, meeting_id, kind
             FROM glossary_terms
             ORDER BY surface, language, scope, ifnull(meeting_id, ''), id",
        )?;
        let rows = statement.query_map([], |row| {
            let language: String = row.get(3)?;
            let scope: String = row.get(4)?;
            let meeting_id: Option<String> = row.get(5)?;
            Ok(GlossaryTerm {
                id: row.get(0)?,
                surface: row.get(1)?,
                canonical: row.get(2)?,
                language: Self::parse_speech_language(&language)?,
                scope: Self::parse_glossary_scope(&scope, meeting_id)?,
                kind: GlossaryKind::from_code(row.get::<_, i64>(6)?),
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Вернуть спикеров встречи в порядке sort_index.
    pub fn list_speakers(&self, meeting_id: &str) -> Result<Vec<Speaker>, AudioManifestError> {
        let mut statement = self.conn.prepare(
            "SELECT id, meeting_id, display_name, sort_index
             FROM speakers
             WHERE meeting_id = ?1
             ORDER BY sort_index, id",
        )?;
        let rows = statement.query_map(params![meeting_id], |row| {
            Ok(Speaker {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                display_name: row.get(2)?,
                sort_index: row.get(3)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Добавить или обновить спикера по id.
    /// Создать спикера, если его ещё нет.
    ///
    /// В отличие от `upsert_speaker`, имя существующего не трогается:
    /// пересбор не имеет права переименовывать спикера обратно после
    /// того, как человек дал ему настоящее имя.
    pub fn ensure_speaker(&mut self, speaker: &Speaker) -> Result<bool, AudioManifestError> {
        let inserted = self.conn.execute(
            "INSERT INTO speakers (id, meeting_id, display_name, sort_index)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO NOTHING",
            params![
                speaker.id,
                speaker.meeting_id,
                speaker.display_name,
                speaker.sort_index
            ],
        )?;
        Ok(inserted > 0)
    }

    pub fn upsert_speaker(&mut self, speaker: &Speaker) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "INSERT INTO speakers (id, meeting_id, display_name, sort_index)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 meeting_id = excluded.meeting_id,
                 display_name = excluded.display_name,
                 sort_index = excluded.sort_index",
            params![
                speaker.id,
                speaker.meeting_id,
                speaker.display_name,
                speaker.sort_index
            ],
        )?;
        Ok(())
    }

    /// Удалить спикера по id.
    /// Удалить спикера, сняв его со всех реплик.
    ///
    /// Без очистки ссылок сегменты остались бы с идентификатором
    /// несуществующего спикера и молча показывались бы без имени.
    pub fn delete_speaker(&mut self, id: &str) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "UPDATE final_segments SET speaker_id = '' WHERE speaker_id = ?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM speakers WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Слить импортированные термины одной транзакцией.
    pub fn replace_glossary_from_import(
        &mut self,
        terms: &[GlossaryTerm],
        updated_at_ms: u64,
    ) -> Result<(), AudioManifestError> {
        let transaction = self.conn.transaction()?;
        for term in terms {
            Self::write_glossary_term(&transaction, term, updated_at_ms)?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn write_glossary_term(
        connection: &Connection,
        term: &GlossaryTerm,
        updated_at_ms: u64,
    ) -> Result<(), rusqlite::Error> {
        let (scope, meeting_id) = match &term.scope {
            GlossaryScope::Global => ("global", None),
            GlossaryScope::Meeting { meeting_id } => ("meeting", Some(meeting_id.as_str())),
        };
        connection.execute(
            "DELETE FROM glossary_terms
             WHERE id = ?1
                OR (
                    surface = ?2
                    AND language = ?3
                    AND scope = ?4
                    AND ifnull(meeting_id, '') = ifnull(?5, '')
                )",
            params![
                term.id,
                term.surface,
                term.language.code(),
                scope,
                meeting_id
            ],
        )?;
        connection.execute(
            "INSERT INTO glossary_terms
             (id, surface, canonical, language, scope, meeting_id, updated_at_ms, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                term.id,
                term.surface,
                term.canonical,
                term.language.code(),
                scope,
                meeting_id,
                updated_at_ms as i64,
                term.kind.code()
            ],
        )?;
        Ok(())
    }

    fn parse_speech_language(value: &str) -> Result<SpeechLanguage, rusqlite::Error> {
        match value {
            "ru" => Ok(SpeechLanguage::Ru),
            "en" => Ok(SpeechLanguage::En),
            "es" => Ok(SpeechLanguage::Es),
            _ => Err(Self::invalid_storage_value("glossary language", value)),
        }
    }

    fn parse_glossary_scope(
        value: &str,
        meeting_id: Option<String>,
    ) -> Result<GlossaryScope, rusqlite::Error> {
        match (value, meeting_id) {
            ("global", _) => Ok(GlossaryScope::Global),
            ("meeting", Some(meeting_id)) => Ok(GlossaryScope::Meeting { meeting_id }),
            ("meeting", None) => Err(Self::invalid_storage_value(
                "glossary scope",
                "meeting without id",
            )),
            _ => Err(Self::invalid_storage_value("glossary scope", value)),
        }
    }

    fn parse_caption_phase(value: &str) -> Result<CaptionPhase, rusqlite::Error> {
        match value {
            "partial" => Ok(CaptionPhase::Partial),
            "final" => Ok(CaptionPhase::Final),
            _ => Err(Self::invalid_storage_value("caption phase", value)),
        }
    }

    fn parse_artifact_kind(value: &str) -> Result<ArtifactKind, rusqlite::Error> {
        match value {
            "brief" => Ok(ArtifactKind::Brief),
            "follow_up" => Ok(ArtifactKind::FollowUp),
            _ => Err(Self::invalid_storage_value("artifact kind", value)),
        }
    }

    fn map_final_transcript_row(
        row: &rusqlite::Row<'_>,
    ) -> Result<FinalTranscript, rusqlite::Error> {
        Ok(FinalTranscript {
            meeting_id: row.get(0)?,
            version: row.get::<_, i64>(1)? as u32,
            body_markdown: row.get(2)?,
            created_at_ms: row.get::<_, i64>(3)? as u64,
        })
    }

    fn invalid_storage_value(field: &str, value: &str) -> rusqlite::Error {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid {field}: {value}"),
            )
            .into(),
        )
    }

    fn chunk_dir(&self, session_id: &str, channel: AudioChannel) -> PathBuf {
        self.root
            .join("sessions")
            .join(session_id)
            .join(channel.dir_name())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use domain::{
        Artifact, ArtifactKind, CaptionEvent, CaptionPhase, FinalTranscript, GlossaryKind,
        GlossaryScope, GlossaryTerm, SpeechLanguage,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn tmp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "meetingraft-storage-test-{nanos}-{seq}-{:?}",
            std::thread::current().id()
        ))
    }

    #[test]
    fn append_two_channels_lists_and_counts() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("s1", 1, "").unwrap();
            store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
            store
                .append_chunk(AudioChannel::System, &[3, 0, 4, 0], 16_000, 10)
                .unwrap();
            assert_eq!(store.chunk_count("s1").unwrap(), 2);
            let list = store.list_chunks("s1").unwrap();
            assert_eq!(list.len(), 2);
            assert!(list[0].path.exists());
            assert_eq!(list[0].channel, AudioChannel::Mic);
            assert_eq!(list[1].channel, AudioChannel::System);
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Файл, созданный до версионирования: схема шага 1, `user_version = 0`.
    /// `open` обязан догнать схему и сохранить строки.
    #[test]
    fn open_upgrades_pre_versioning_database() {
        let root = tmp_root();
        fs::create_dir_all(&root).unwrap();
        {
            let conn = Connection::open(root.join("meetingraft.sqlite3")).unwrap();
            conn.execute_batch(crate::migrations::baseline_schema())
                .unwrap();
            conn.execute(
                "INSERT INTO sessions (id, started_at_ms) VALUES (?1, ?2)",
                params!["legacy", 7_i64],
            )
            .unwrap();
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 0, "база до версионирования");
        }

        {
            let store = AudioManifestStore::open(&root).unwrap();
            let summaries = store.list_meeting_summaries().unwrap();
            assert_eq!(summaries.len(), 1);
            assert_eq!(summaries[0].id, "legacy");
        }
        {
            let conn = Connection::open(root.join("meetingraft.sqlite3")).unwrap();
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version as u32, crate::migrations::schema_version());
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn append_without_session_fails() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let err = store
                .append_chunk(AudioChannel::Mic, &[0, 0], 16_000, 0)
                .unwrap_err();
            assert!(matches!(err, AudioManifestError::SessionNotOpen));
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn append_caption_persists() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("s1", 1, "").unwrap();
            let event = domain::CaptionEvent::new(
                "c1".into(),
                "привет".into(),
                domain::CaptionPhase::Final,
            );
            store.append_caption("s1", &event, 42).unwrap();
            assert_eq!(store.caption_count("s1").unwrap(), 1);
        }
        let _ = fs::remove_dir_all(&root);
    }

    fn segment(index: u32, start_ms: u64, channel: AudioChannel, text: &str) -> FinalSegment {
        FinalSegment {
            index,
            start_ms,
            end_ms: start_ms + 500,
            channel,
            speaker_id: String::new(),
            speaker_pinned: false,
            text: text.to_string(),
            text_edited: false,
            original_text: String::new(),
        }
    }

    fn seeded_segments(store: &mut AudioManifestStore) {
        let segments = vec![
            segment(0, 0, AudioChannel::Mic, "я"),
            segment(1, 600, AudioChannel::System, "они"),
            segment(2, 1200, AudioChannel::System, "они снова"),
        ];
        store.replace_final_segments("m1", 1, &segments).unwrap();
    }

    /// Основная операция фазы: одно назначение подписывает весь канал.
    #[test]
    fn channel_assignment_touches_only_its_channel() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            seeded_segments(&mut store);

            let updated = store
                .set_channel_speaker("m1", 1, AudioChannel::System, "sp-peter")
                .unwrap();

            assert_eq!(updated, 2);
            let read = store.list_final_segments("m1", 1).unwrap();
            assert_eq!(read[0].speaker_id, "", "микрофон не тронут");
            assert_eq!(read[1].speaker_id, "sp-peter");
            assert_eq!(read[2].speaker_id, "sp-peter");
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Ручная правка обязана пережить повторное назначение канала: иначе
    /// человек терял бы свою работу молча.
    #[test]
    fn manual_assignment_survives_channel_reassignment() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            seeded_segments(&mut store);
            store
                .set_channel_speaker("m1", 1, AudioChannel::System, "sp-peter")
                .unwrap();

            store.set_segment_speaker("m1", 1, 2, "sp-anna").unwrap();
            store
                .set_channel_speaker("m1", 1, AudioChannel::System, "sp-peter")
                .unwrap();

            let read = store.list_final_segments("m1", 1).unwrap();
            assert_eq!(read[1].speaker_id, "sp-peter");
            assert_eq!(read[2].speaker_id, "sp-anna", "правка затёрта");
            assert!(read[2].speaker_pinned);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unpinning_returns_segment_under_channel_rule() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            seeded_segments(&mut store);
            store.set_segment_speaker("m1", 1, 2, "sp-anna").unwrap();

            store.unpin_segment_speaker("m1", 1, 2).unwrap();
            store
                .set_channel_speaker("m1", 1, AudioChannel::System, "sp-peter")
                .unwrap();

            let read = store.list_final_segments("m1", 1).unwrap();
            assert_eq!(read[2].speaker_id, "sp-peter");
            assert!(!read[2].speaker_pinned);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn assigning_unknown_segment_is_an_error() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            seeded_segments(&mut store);

            let err = store.set_segment_speaker("m1", 1, 99, "sp").unwrap_err();

            assert!(
                matches!(err, AudioManifestError::SegmentNotFound { .. }),
                "{err:?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Удалённый спикер не должен оставлять висячие ссылки: сегмент с
    /// несуществующим id молча показывался бы без имени.
    #[test]
    fn deleting_speaker_clears_segment_references() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            seeded_segments(&mut store);
            store
                .upsert_speaker(&Speaker {
                    id: "sp-peter".into(),
                    meeting_id: "m1".into(),
                    display_name: "Пётр".into(),
                    sort_index: 0,
                })
                .unwrap();
            store
                .set_channel_speaker("m1", 1, AudioChannel::System, "sp-peter")
                .unwrap();

            store.delete_speaker("sp-peter").unwrap();

            let read = store.list_final_segments("m1", 1).unwrap();
            assert!(read.iter().all(|s| s.speaker_id.is_empty()));
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn final_segments_round_trip_in_order() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let segments = vec![
                segment(0, 0, AudioChannel::Mic, "я говорю"),
                segment(1, 600, AudioChannel::System, "они отвечают"),
            ];

            store.replace_final_segments("m1", 1, &segments).unwrap();
        }

        {
            let store = AudioManifestStore::open(&root).unwrap();
            let read = store.list_final_segments("m1", 1).unwrap();

            assert_eq!(read.len(), 2);
            assert_eq!(read[0].text, "я говорю");
            assert_eq!(read[0].channel, AudioChannel::Mic);
            assert_eq!(read[1].channel, AudioChannel::System);
            assert_eq!(read[1].index, 1);
            assert_eq!(read[1].duration_ms(), 500);
            assert!(read[0].speaker_id.is_empty(), "спикеров ещё нет");
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Пересбор той же версии заменяет сегменты, а не удваивает их.
    #[test]
    fn replacing_version_does_not_duplicate_segments() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store
                .replace_final_segments("m1", 1, &[segment(0, 0, AudioChannel::Mic, "черновик")])
                .unwrap();

            store
                .replace_final_segments("m1", 1, &[segment(0, 0, AudioChannel::Mic, "начисто")])
                .unwrap();

            let read = store.list_final_segments("m1", 1).unwrap();
            assert_eq!(read.len(), 1);
            assert_eq!(read[0].text, "начисто");
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Версии независимы: пересбор одной не трогает другую.
    #[test]
    fn versions_of_final_segments_are_independent() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store
                .replace_final_segments("m1", 1, &[segment(0, 0, AudioChannel::Mic, "версия 1")])
                .unwrap();
            store
                .replace_final_segments("m1", 2, &[segment(0, 0, AudioChannel::Mic, "версия 2")])
                .unwrap();

            store
                .replace_final_segments(
                    "m1",
                    2,
                    &[segment(0, 0, AudioChannel::Mic, "версия 2 bis")],
                )
                .unwrap();

            assert_eq!(
                store.list_final_segments("m1", 1).unwrap()[0].text,
                "версия 1"
            );
            assert_eq!(
                store.list_final_segments("m1", 2).unwrap()[0].text,
                "версия 2 bis"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_final_segments_of_unknown_version_is_empty() {
        let root = tmp_root();
        {
            let store = AudioManifestStore::open(&root).unwrap();
            assert!(store.list_final_segments("m1", 7).unwrap().is_empty());
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Правленый сегмент отдаёт и правку, и то, что распознала модель.
    ///
    /// Тексты заведомо разные: совпади они, тест прошёл бы, ничего не
    /// проверив.
    #[test]
    fn edited_segment_carries_recognized_text() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store
                .replace_final_segments(
                    "m1",
                    1,
                    &[FinalSegment {
                        index: 0,
                        start_ms: 0,
                        end_ms: 1_000,
                        channel: AudioChannel::Mic,
                        speaker_id: String::new(),
                        speaker_pinned: false,
                        text: "упирается в юни-эф-эф-ай".to_string(),
                        text_edited: false,
                        original_text: String::new(),
                    }],
                )
                .unwrap();
            store
                .upsert_segment_edit(&domain::SegmentEdit {
                    id: "e1".to_string(),
                    meeting_id: "m1".to_string(),
                    channel: AudioChannel::Mic,
                    start_ms: 0,
                    end_ms: 1_000,
                    original_text: "упирается в юни-эф-эф-ай".to_string(),
                    edited_text: "упирается в UniFFI".to_string(),
                    created_at_ms: 10,
                    applied_version: Some(1),
                })
                .unwrap();

            let segments = store.list_final_segments("m1", 1).unwrap();
            assert_eq!(segments[0].text, "упирается в UniFFI");
            assert_eq!(segments[0].original_text, "упирается в юни-эф-эф-ай");
            assert!(segments[0].text_edited);
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Неправленый сегмент не выдумывает исходный текст.
    #[test]
    fn unedited_segment_has_empty_original_text() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store
                .replace_final_segments(
                    "m1",
                    1,
                    &[FinalSegment {
                        index: 0,
                        start_ms: 0,
                        end_ms: 1_000,
                        channel: AudioChannel::Mic,
                        speaker_id: String::new(),
                        speaker_pinned: false,
                        text: "обычная реплика".to_string(),
                        text_edited: false,
                        original_text: String::new(),
                    }],
                )
                .unwrap();

            let segments = store.list_final_segments("m1", 1).unwrap();
            assert!(segments[0].original_text.is_empty());
            assert!(!segments[0].text_edited);
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Удаление встречи уносит и сегменты, и журнал правок — обещание
    /// architecture.md: удаление удаляет всё. Журнал хранит дословные
    /// реплики, поэтому его остаток — не мусор, а неудалённый разговор.
    #[test]
    fn delete_meeting_removes_final_segments() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            store
                .replace_final_segments("m1", 1, &[segment(0, 0, AudioChannel::Mic, "текст")])
                .unwrap();
            store
                .upsert_segment_edit(&domain::SegmentEdit {
                    id: "e1".into(),
                    meeting_id: "m1".into(),
                    channel: AudioChannel::Mic,
                    start_ms: 0,
                    end_ms: 1000,
                    original_text: "текст".into(),
                    edited_text: "правленый текст".into(),
                    created_at_ms: 5,
                    applied_version: Some(1),
                })
                .unwrap();
            store.end_session(10).unwrap();

            store.delete_meeting("m1").unwrap();

            assert!(store.list_final_segments("m1", 1).unwrap().is_empty());
            assert!(
                store.list_segment_edits("m1").unwrap().is_empty(),
                "дословные реплики не должны пережить удаление встречи"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Чанки склеиваются в порядке seq, каналы не смешиваются.
    /// Секунда записи чанками по 100 мс: три чанка по 1600 отсчётов.
    fn seed_second(store: &mut AudioManifestStore, channel: AudioChannel, chunks: u64) {
        for index in 0..chunks {
            // Каждый чанк заполнен своим номером — так видно, откуда срез.
            let sample = (index as i16) + 1;
            let bytes: Vec<u8> = (0..1_600).flat_map(|_| sample.to_le_bytes()).collect();
            store
                .append_chunk(channel, &bytes, 16_000, index * 100)
                .unwrap();
        }
    }

    /// Фрагмент вырезается по времени, а не по границам чанков: реплика
    /// начинается там, где начинается, а не там, где удобно хранилищу.
    #[test]
    fn pcm_range_cuts_inside_chunks() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            seed_second(&mut store, AudioChannel::Mic, 10);

            let fragment = store
                .read_pcm_range("m1", AudioChannel::Mic, 150, 250)
                .unwrap();

            assert_eq!(fragment.sample_rate, 16_000);
            assert_eq!(fragment.pcm.len(), 1_600, "100 мс на 16 кГц");
            assert_eq!(fragment.duration_ms(), 100);
            // Половина второго чанка и половина третьего.
            assert_eq!(fragment.pcm[0], 2);
            assert_eq!(fragment.pcm[1_599], 3);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Соседний канал в срез попадать не должен: слушают конкретного
    /// говорящего, а не всё сразу.
    #[test]
    fn pcm_range_keeps_channels_apart() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            seed_second(&mut store, AudioChannel::Mic, 4);

            let fragment = store
                .read_pcm_range("m1", AudioChannel::System, 0, 400)
                .unwrap();

            assert!(fragment.pcm.is_empty());
            assert_eq!(
                fragment.sample_rate, 0,
                "нечего играть — нечего и настраивать"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pcm_range_rejects_empty_and_inverted_ranges() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            seed_second(&mut store, AudioChannel::Mic, 4);

            assert!(
                store
                    .read_pcm_range("m1", AudioChannel::Mic, 200, 200)
                    .unwrap()
                    .pcm
                    .is_empty()
            );
            assert!(
                store
                    .read_pcm_range("m1", AudioChannel::Mic, 300, 100)
                    .unwrap()
                    .pcm
                    .is_empty()
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Запрос «дай всю встречу» ограничивается потолком: фрагмент едет
    /// через границу UniFFI целиком.
    #[test]
    fn pcm_range_is_capped() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            seed_second(&mut store, AudioChannel::Mic, 10);

            let fragment = store
                .read_pcm_range("m1", AudioChannel::Mic, 0, 60 * 60 * 1000)
                .unwrap();

            assert_eq!(
                fragment.pcm.len(),
                16_000,
                "секунда записи, а не час запроса"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_session_pcm_concatenates_channel_in_order() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
            store
                .append_chunk(AudioChannel::Mic, &[3, 0, 4, 0], 16_000, 100)
                .unwrap();
            store
                .append_chunk(AudioChannel::System, &[9, 0], 16_000, 0)
                .unwrap();

            assert_eq!(
                store.read_session_pcm("m1", AudioChannel::Mic).unwrap(),
                vec![1, 2, 3, 4]
            );
            assert_eq!(
                store.read_session_pcm("m1", AudioChannel::System).unwrap(),
                vec![9]
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_session_pcm_of_empty_session_is_empty() {
        let root = tmp_root();
        {
            let store = AudioManifestStore::open(&root).unwrap();
            assert!(
                store
                    .read_session_pcm("missing", AudioChannel::Mic)
                    .unwrap()
                    .is_empty()
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Пропавший файл чанка — ошибка, а не дыра тишины в транскрипте.
    #[test]
    fn read_session_pcm_fails_on_missing_chunk_file() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            let chunk = store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
            fs::remove_file(&chunk.path).unwrap();

            let err = store.read_session_pcm("m1", AudioChannel::Mic).unwrap_err();

            assert!(matches!(err, AudioManifestError::Io(_)), "{err:?}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Обрезанный файл тоже ловится: manifest знает ожидаемую длину.
    #[test]
    fn read_session_pcm_fails_on_truncated_chunk() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();
            let chunk = store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
            fs::write(&chunk.path, [1_u8, 0]).unwrap();

            let err = store.read_session_pcm("m1", AudioChannel::Mic).unwrap_err();

            assert!(
                matches!(err, AudioManifestError::ChunkTruncated { .. }),
                "{err:?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_finds_russian_prefix_with_snippet() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "Встреча").unwrap();
            store
                .upsert_final_transcript(&FinalTranscript {
                    meeting_id: "m1".into(),
                    version: 1,
                    body_markdown: "Обсудили изменения биллинга и сроки".into(),
                    created_at_ms: 10,
                })
                .unwrap();

            let hits = store.search("биллин", 10).unwrap();

            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].meeting_id, "m1");
            assert_eq!(hits[0].kind, SearchHitKind::Final);
            assert!(hits[0].snippet.contains("биллинга"), "{}", hits[0].snippet);
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Спецсимволы FTS в пользовательском вводе не должны ронять запрос.
    #[test]
    fn search_survives_fts_special_characters() {
        let root = tmp_root();
        {
            let store = AudioManifestStore::open(&root).unwrap();
            for query in ["\"", "*", "(", "NEAR(", "a OR", ""] {
                assert!(store.search(query, 10).is_ok(), "запрос {query:?} упал");
            }
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Только финальные captions попадают в индекс.
    #[test]
    fn search_indexes_final_captions_only() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let mut partial = CaptionEvent::new(
                "p".into(),
                "черновик про кофе".into(),
                CaptionPhase::Partial,
            );
            partial.channel = AudioChannel::Mic;
            let final_event =
                CaptionEvent::new("f".into(), "решили про кофе".into(), CaptionPhase::Final);
            store.append_caption("m1", &partial, 1).unwrap();
            store.append_caption("m1", &final_event, 2).unwrap();

            let hits = store.search("кофе", 10).unwrap();

            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].ref_id, "f");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_meeting_removes_rows_index_and_chunks() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "Встреча").unwrap();
            store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
            store
                .append_caption(
                    "m1",
                    &CaptionEvent::new("c1".into(), "уникальное слово".into(), CaptionPhase::Final),
                    5,
                )
                .unwrap();
            store.end_session(10).unwrap();

            store.delete_meeting("m1").unwrap();

            assert!(store.list_meeting_summaries().unwrap().is_empty());
            assert!(store.list_captions("m1").unwrap().is_empty());
            assert!(store.search("уникальное", 10).unwrap().is_empty());
            assert!(!root.join("sessions").join("m1").exists());
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_meeting_rejects_active_session_and_unknown_id() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1, "").unwrap();

            let active = store.delete_meeting("m1").unwrap_err();
            assert!(matches!(active, AudioManifestError::SessionActive(_)));

            let unknown = store.delete_meeting("missing").unwrap_err();
            assert!(matches!(unknown, AudioManifestError::MeetingNotFound(_)));
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Название и длительность переживают переоткрытие базы.
    #[test]
    fn title_and_duration_survive_reopen() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1_000, "Weekly sync").unwrap();
            store.end_session(4_000).unwrap();
        }

        {
            let store = AudioManifestStore::open(&root).unwrap();
            let summary = store.list_meeting_summaries().unwrap().remove(0);
            assert_eq!(summary.title, "Weekly sync");
            assert_eq!(summary.ended_at_ms, Some(4_000));
            assert_eq!(summary.duration_ms(), Some(3_000));
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Незавершённая встреча не имеет длительности.
    #[test]
    fn running_meeting_has_no_duration() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1_000, "").unwrap();
            let summary = store.list_meeting_summaries().unwrap().remove(0);
            assert_eq!(summary.ended_at_ms, None);
            assert_eq!(summary.duration_ms(), None);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_meeting_persists_and_rejects_unknown_id() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("m1", 1_000, "Черновик").unwrap();
            store.set_meeting_title("m1", "Ретро спринта").unwrap();
            assert_eq!(
                store.list_meeting_summaries().unwrap()[0].title,
                "Ретро спринта"
            );

            let err = store.set_meeting_title("missing", "x").unwrap_err();
            assert!(matches!(err, AudioManifestError::MeetingNotFound(_)));
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Канал говорящего переживает запись и переоткрытие базы (ADR-009).
    #[test]
    fn caption_channel_round_trips() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let mut mine = CaptionEvent::new("mine".into(), "я".into(), CaptionPhase::Final);
            mine.channel = AudioChannel::Mic;
            let mut theirs = CaptionEvent::new("theirs".into(), "они".into(), CaptionPhase::Final);
            theirs.channel = AudioChannel::System;
            store.append_caption("s1", &mine, 10).unwrap();
            store.append_caption("s1", &theirs, 20).unwrap();
        }

        {
            let store = AudioManifestStore::open(&root).unwrap();
            let channels: Vec<AudioChannel> = store
                .list_captions("s1")
                .unwrap()
                .iter()
                .map(|event| event.channel)
                .collect();
            assert_eq!(channels, vec![AudioChannel::Mic, AudioChannel::System]);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_captions_reads_reopened_session_in_creation_order() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store
                .append_caption(
                    "s1",
                    &CaptionEvent::new("later".into(), "готово".into(), CaptionPhase::Final),
                    20,
                )
                .unwrap();
            store
                .append_caption(
                    "s1",
                    &CaptionEvent::new("earlier".into(), "черновик".into(), CaptionPhase::Partial),
                    10,
                )
                .unwrap();
        }

        {
            let store = AudioManifestStore::open(&root).unwrap();
            assert_eq!(
                store.list_captions("s1").unwrap(),
                vec![
                    CaptionEvent::new("earlier".into(), "черновик".into(), CaptionPhase::Partial,),
                    CaptionEvent::new("later".into(), "готово".into(), CaptionPhase::Final),
                ]
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn next_list_and_get_final_versions() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            assert_eq!(store.next_final_version("m1").unwrap(), 1);
            store
                .upsert_final_transcript(&FinalTranscript {
                    meeting_id: "m1".into(),
                    version: 1,
                    body_markdown: "one".into(),
                    created_at_ms: 10,
                })
                .unwrap();
            store
                .upsert_final_transcript(&FinalTranscript {
                    meeting_id: "m1".into(),
                    version: 2,
                    body_markdown: "two".into(),
                    created_at_ms: 20,
                })
                .unwrap();
            assert_eq!(store.next_final_version("m1").unwrap(), 3);
            let list = store.list_final_transcripts("m1").unwrap();
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].version, 2);
            assert_eq!(list[0].body_markdown, "two");
            assert_eq!(list[1].version, 1);
            assert_eq!(
                store.get_final_transcript("m1").unwrap().unwrap().version,
                2
            );
            assert_eq!(
                store
                    .get_final_transcript_version("m1", 1)
                    .unwrap()
                    .unwrap()
                    .body_markdown,
                "one"
            );
            assert_eq!(store.get_final_transcript_version("m1", 9).unwrap(), None);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn upsert_final_transcript_overwrites_same_version() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let mut transcript = FinalTranscript {
                meeting_id: "meeting-1".into(),
                version: 1,
                body_markdown: "Первая версия".into(),
                created_at_ms: 100,
            };
            store.upsert_final_transcript(&transcript).unwrap();

            transcript.body_markdown = "Исправленная версия".into();
            transcript.created_at_ms = 200;
            store.upsert_final_transcript(&transcript).unwrap();

            assert_eq!(
                store.get_final_transcript("meeting-1").unwrap(),
                Some(transcript)
            );
            assert_eq!(store.get_final_transcript("missing").unwrap(), None);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn insert_and_list_artifacts_maps_kinds() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let brief = Artifact {
                id: "artifact-1".into(),
                meeting_id: "meeting-1".into(),
                kind: ArtifactKind::Brief,
                template_id: "builtin.brief".into(),
                body_markdown: "# Brief".into(),
                created_at_ms: 100,
            };
            let follow_up = Artifact {
                id: "artifact-2".into(),
                meeting_id: "meeting-1".into(),
                kind: ArtifactKind::FollowUp,
                template_id: "builtin.follow_up".into(),
                body_markdown: "Итоги встречи".into(),
                created_at_ms: 200,
            };
            store.insert_artifact(&follow_up).unwrap();
            store.insert_artifact(&brief).unwrap();

            assert_eq!(
                store.list_artifacts("meeting-1").unwrap(),
                vec![brief, follow_up]
            );
            assert!(store.list_artifacts("missing").unwrap().is_empty());
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn meeting_summaries_include_final_and_artifact_flags() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            store.begin_session("meeting-1", 100, "").unwrap();
            store.begin_session("meeting-2", 200, "").unwrap();
            store
                .upsert_final_transcript(&FinalTranscript {
                    meeting_id: "meeting-1".into(),
                    version: 1,
                    body_markdown: "Финал".into(),
                    created_at_ms: 300,
                })
                .unwrap();
            for (id, kind) in [
                ("artifact-1", ArtifactKind::Brief),
                ("artifact-2", ArtifactKind::FollowUp),
            ] {
                store
                    .insert_artifact(&Artifact {
                        id: id.into(),
                        meeting_id: "meeting-1".into(),
                        kind,
                        template_id: "builtin".into(),
                        body_markdown: "Текст".into(),
                        created_at_ms: 400,
                    })
                    .unwrap();
            }

            let summaries = store.list_meeting_summaries().unwrap();
            let meeting_1 = summaries
                .iter()
                .find(|summary| summary.id == "meeting-1")
                .unwrap();
            assert_eq!(meeting_1.started_at_ms, 100);
            assert!(meeting_1.has_final);
            assert_eq!(meeting_1.artifact_count, 2);

            let meeting_2 = summaries
                .iter()
                .find(|summary| summary.id == "meeting-2")
                .unwrap();
            assert_eq!(meeting_2.started_at_ms, 200);
            assert!(!meeting_2.has_final);
            assert_eq!(meeting_2.artifact_count, 0);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn glossary_upsert_list_delete() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let mut term = GlossaryTerm {
                id: "term-1".into(),
                surface: "униффи".into(),
                canonical: "UniFFI".into(),
                language: SpeechLanguage::Ru,
                scope: GlossaryScope::Global,
                kind: GlossaryKind::Replacement,
            };

            store.upsert_glossary_term(&term, 1).unwrap();
            assert_eq!(store.list_glossary_terms().unwrap(), vec![term.clone()]);

            term.canonical = "UniFFI Framework".into();
            store.upsert_glossary_term(&term, 2).unwrap();
            assert_eq!(store.list_glossary_terms().unwrap(), vec![term]);

            store.delete_glossary_term("term-1").unwrap();
            assert!(store.list_glossary_terms().unwrap().is_empty());
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn glossary_kind_round_trips() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let term = GlossaryTerm {
                id: "t1".into(),
                surface: "интра ру".into(),
                canonical: "intra.ru".into(),
                language: SpeechLanguage::Ru,
                scope: GlossaryScope::Global,
                kind: GlossaryKind::Hint,
            };
            store.upsert_glossary_term(&term, 0).unwrap();

            let read = store.list_glossary_terms().unwrap();
            assert_eq!(read.len(), 1);
            assert_eq!(read[0].kind, GlossaryKind::Hint);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn speakers_crud_and_meeting_isolation() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let a = domain::Speaker {
                id: "s1".into(),
                meeting_id: "m1".into(),
                display_name: "Алиса".into(),
                sort_index: 0,
            };
            let b = domain::Speaker {
                id: "s2".into(),
                meeting_id: "m2".into(),
                display_name: "Bob".into(),
                sort_index: 0,
            };
            store.upsert_speaker(&a).unwrap();
            store.upsert_speaker(&b).unwrap();
            assert_eq!(store.list_speakers("m1").unwrap(), vec![a.clone()]);
            let mut renamed = a.clone();
            renamed.display_name = "Алиса К.".into();
            store.upsert_speaker(&renamed).unwrap();
            assert_eq!(
                store.list_speakers("m1").unwrap()[0].display_name,
                "Алиса К."
            );
            store.delete_speaker("s1").unwrap();
            assert!(store.list_speakers("m1").unwrap().is_empty());
            assert_eq!(store.list_speakers("m2").unwrap().len(), 1);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn speakers_ordered_by_sort_index() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let speakers = [
                domain::Speaker {
                    id: "s2".into(),
                    meeting_id: "m1".into(),
                    display_name: "Second".into(),
                    sort_index: 2,
                },
                domain::Speaker {
                    id: "s0".into(),
                    meeting_id: "m1".into(),
                    display_name: "First".into(),
                    sort_index: 0,
                },
                domain::Speaker {
                    id: "s1".into(),
                    meeting_id: "m1".into(),
                    display_name: "Middle".into(),
                    sort_index: 1,
                },
            ];
            for speaker in &speakers {
                store.upsert_speaker(speaker).unwrap();
            }
            let listed = store.list_speakers("m1").unwrap();
            assert_eq!(
                listed
                    .iter()
                    .map(|speaker| speaker.sort_index)
                    .collect::<Vec<_>>(),
                vec![0, 1, 2]
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn glossary_import_upserts_without_deleting_unrelated_terms() {
        let root = tmp_root();
        {
            let mut store = AudioManifestStore::open(&root).unwrap();
            let unrelated = GlossaryTerm {
                id: "term-1".into(),
                surface: "рафт".into(),
                canonical: "Raft".into(),
                language: SpeechLanguage::Ru,
                scope: GlossaryScope::Global,
                kind: GlossaryKind::Replacement,
            };
            let imported = GlossaryTerm {
                id: "term-2".into(),
                surface: "meeting raft".into(),
                canonical: "MeetingRaft".into(),
                language: SpeechLanguage::En,
                scope: GlossaryScope::Meeting {
                    meeting_id: "meeting-1".into(),
                },
                kind: GlossaryKind::Replacement,
            };
            store.upsert_glossary_term(&unrelated, 1).unwrap();

            store
                .replace_glossary_from_import(std::slice::from_ref(&imported), 2)
                .unwrap();

            assert_eq!(
                store.list_glossary_terms().unwrap(),
                vec![imported, unrelated]
            );
        }
        let _ = fs::remove_dir_all(&root);
    }
}
