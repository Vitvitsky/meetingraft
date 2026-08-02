//! SQLite audio_manifest + файлы чанков на диске.

use std::fs;
use std::path::{Path, PathBuf};

use domain::{AudioChannel, GlossaryScope, GlossaryTerm, SpeechLanguage};
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
    /// Открыть/создать БД в `root/meetingraft.sqlite3`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, AudioManifestError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let db_path = root.join("meetingraft.sqlite3");
        let conn = Connection::open(&db_path)?;
        // Снижает flaky SQLITE_BUSY при параллельных тестах / WAL checkpoint.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY NOT NULL,
                started_at_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audio_manifest (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                seq INTEGER NOT NULL,
                path TEXT NOT NULL,
                sample_rate INTEGER NOT NULL,
                frame_count INTEGER NOT NULL,
                timestamp_ms INTEGER NOT NULL,
                UNIQUE(session_id, channel, seq)
            );
            CREATE TABLE IF NOT EXISTS caption_events (
                id TEXT PRIMARY KEY NOT NULL,
                session_id TEXT NOT NULL,
                text TEXT NOT NULL,
                phase TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS glossary_terms (
                id TEXT PRIMARY KEY NOT NULL,
                surface TEXT NOT NULL,
                canonical TEXT NOT NULL,
                language TEXT NOT NULL,
                scope TEXT NOT NULL,
                meeting_id TEXT,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_glossary_unique
                ON glossary_terms(surface, language, scope, ifnull(meeting_id, ''));
            ",
        )?;
        Ok(Self {
            root,
            conn,
            active_session: None,
            next_seq: [0, 0],
        })
    }

    /// Начать recording session.
    pub fn begin_session(
        &mut self,
        session_id: &str,
        started_at_ms: u64,
    ) -> Result<(), AudioManifestError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (id, started_at_ms) VALUES (?1, ?2)",
            params![session_id, started_at_ms as i64],
        )?;
        for channel in [AudioChannel::Mic, AudioChannel::System] {
            fs::create_dir_all(self.chunk_dir(session_id, channel))?;
        }
        self.active_session = Some(session_id.to_string());
        self.next_seq = [0, 0];
        Ok(())
    }

    /// Закончить session (сброс active).
    pub fn end_session(&mut self) {
        self.active_session = None;
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
            "INSERT INTO caption_events (id, session_id, text, phase, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.id,
                session_id,
                event.text,
                phase,
                created_at_ms as i64
            ],
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
            "SELECT id, surface, canonical, language, scope, meeting_id
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
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
             (id, surface, canonical, language, scope, meeting_id, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                term.id,
                term.surface,
                term.canonical,
                term.language.code(),
                scope,
                meeting_id,
                updated_at_ms as i64
            ],
        )?;
        Ok(())
    }

    fn parse_speech_language(value: &str) -> Result<SpeechLanguage, rusqlite::Error> {
        match value {
            "ru" => Ok(SpeechLanguage::Ru),
            "en" => Ok(SpeechLanguage::En),
            "es" => Ok(SpeechLanguage::Es),
            _ => Err(Self::invalid_glossary_value("language", value)),
        }
    }

    fn parse_glossary_scope(
        value: &str,
        meeting_id: Option<String>,
    ) -> Result<GlossaryScope, rusqlite::Error> {
        match (value, meeting_id) {
            ("global", _) => Ok(GlossaryScope::Global),
            ("meeting", Some(meeting_id)) => Ok(GlossaryScope::Meeting { meeting_id }),
            ("meeting", None) => Err(Self::invalid_glossary_value("scope", "meeting without id")),
            _ => Err(Self::invalid_glossary_value("scope", value)),
        }
    }

    fn invalid_glossary_value(field: &str, value: &str) -> rusqlite::Error {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid glossary {field}: {value}"),
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
mod tests {
    use super::*;
    use domain::{GlossaryScope, GlossaryTerm, SpeechLanguage};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp_root() -> PathBuf {
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
            store.begin_session("s1", 1).unwrap();
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
            store.begin_session("s1", 1).unwrap();
            let event = domain::CaptionEvent {
                id: "c1".into(),
                text: "привет".into(),
                phase: domain::CaptionPhase::Final,
            };
            store.append_caption("s1", &event, 42).unwrap();
            assert_eq!(store.caption_count("s1").unwrap(), 1);
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
            };
            let imported = GlossaryTerm {
                id: "term-2".into(),
                surface: "meeting raft".into(),
                canonical: "MeetingRaft".into(),
                language: SpeechLanguage::En,
                scope: GlossaryScope::Meeting {
                    meeting_id: "meeting-1".into(),
                },
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
