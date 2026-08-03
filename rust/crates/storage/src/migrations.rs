//! Миграции схемы SQLite по `PRAGMA user_version` (ADR-006).
//!
//! Шаг N приводит базу к версии N. Шаг 1 — базовая схема фаз 0–6; он
//! написан через `CREATE TABLE IF NOT EXISTS`, поэтому база, созданная до
//! появления версионирования (`user_version = 0`, таблицы уже на месте),
//! поднимается без потери данных.

use rusqlite::Connection;

use crate::AudioManifestError;

/// Шаги миграции; индекс + 1 = версия схемы, к которой шаг приводит.
const STEPS: &[&str] = &[
    // 1 — базовая схема (фазы 0–6).
    "
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
    CREATE TABLE IF NOT EXISTS final_transcripts (
        meeting_id TEXT NOT NULL,
        version INTEGER NOT NULL,
        body_markdown TEXT NOT NULL,
        created_at_ms INTEGER NOT NULL,
        PRIMARY KEY (meeting_id, version)
    );
    CREATE TABLE IF NOT EXISTS artifacts (
        id TEXT PRIMARY KEY NOT NULL,
        meeting_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        template_id TEXT NOT NULL,
        body_markdown TEXT NOT NULL,
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
    CREATE TABLE IF NOT EXISTS speakers (
        id TEXT PRIMARY KEY NOT NULL,
        meeting_id TEXT NOT NULL,
        display_name TEXT NOT NULL,
        sort_index INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_speakers_meeting
        ON speakers(meeting_id, sort_index);
    ",
    // 2 — канал говорящего в caption_events (ADR-009). Существующие записи
    // сделаны только с микрофона, поэтому DEFAULT 'mic' корректен.
    "
    ALTER TABLE caption_events ADD COLUMN channel TEXT NOT NULL DEFAULT 'mic';
    ",
    // 3 — название и время окончания встречи. Пустое название допустимо:
    // fallback рисует презентационный слой.
    "
    ALTER TABLE sessions ADD COLUMN title TEXT NOT NULL DEFAULT '';
    ALTER TABLE sessions ADD COLUMN ended_at_ms INTEGER;
    ",
    // 4 — полнотекстовый поиск. unicode61 нормально режет русский текст;
    // backfill поднимает уже накопленные материалы.
    "
    CREATE VIRTUAL TABLE meeting_fts USING fts5(
        meeting_id UNINDEXED,
        kind UNINDEXED,
        ref_id UNINDEXED,
        body,
        tokenize = 'unicode61 remove_diacritics 2'
    );
    INSERT INTO meeting_fts (meeting_id, kind, ref_id, body)
        SELECT session_id, 'caption', id, text
        FROM caption_events WHERE phase = 'final';
    INSERT INTO meeting_fts (meeting_id, kind, ref_id, body)
        SELECT meeting_id, 'final', CAST(version AS TEXT), body_markdown
        FROM final_transcripts;
    INSERT INTO meeting_fts (meeting_id, kind, ref_id, body)
        SELECT meeting_id, 'artifact', id, body_markdown
        FROM artifacts;
    ",
];

/// Версия схемы, к которой приводит полный набор шагов.
pub fn schema_version() -> u32 {
    STEPS.len() as u32
}

/// Схема шага 1 — то, как выглядит база, созданная до версионирования.
#[cfg(test)]
pub(crate) fn baseline_schema() -> &'static str {
    STEPS[0]
}

/// Текущая версия схемы в базе.
fn current_version(conn: &Connection) -> Result<u32, AudioManifestError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(version.max(0) as u32)
}

/// Привести схему к `schema_version()`; уже применённые шаги пропускаются.
///
/// `PRAGMA journal_mode` сюда не входит: он не выполняется внутри
/// транзакции и остаётся заботой `AudioManifestStore::open`.
pub fn migrate(conn: &Connection) -> Result<(), AudioManifestError> {
    let current = current_version(conn)?;
    for (index, step) in STEPS.iter().enumerate() {
        let version = index as u32 + 1;
        if version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(step)?;
        // user_version не принимает параметр; значение — внутренний u32.
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .expect("sqlite_master readable")
            > 0
    }

    #[test]
    fn migrate_new_database_creates_schema_and_sets_version() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        assert_eq!(current_version(&conn).expect("version"), 0);

        migrate(&conn).expect("migrate");

        assert_eq!(current_version(&conn).expect("version"), schema_version());
        for table in [
            "sessions",
            "audio_manifest",
            "caption_events",
            "final_transcripts",
            "artifacts",
            "glossary_terms",
            "speakers",
        ] {
            assert!(table_exists(&conn, table), "нет таблицы {table}");
        }
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        migrate(&conn).expect("first migrate");
        migrate(&conn).expect("second migrate");
        assert_eq!(current_version(&conn).expect("version"), schema_version());
    }

    /// База до версионирования: таблицы есть, `user_version` нулевой.
    #[test]
    fn migrate_legacy_database_keeps_rows() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(STEPS[0]).expect("legacy schema");
        conn.execute(
            "INSERT INTO sessions (id, started_at_ms) VALUES (?1, ?2)",
            rusqlite::params!["legacy-session", 42_i64],
        )
        .expect("insert legacy row");
        assert_eq!(current_version(&conn).expect("version"), 0);

        migrate(&conn).expect("migrate legacy");

        assert_eq!(current_version(&conn).expect("version"), schema_version());
        let started: i64 = conn
            .query_row(
                "SELECT started_at_ms FROM sessions WHERE id = ?1",
                ["legacy-session"],
                |row| row.get(0),
            )
            .expect("legacy row survived");
        assert_eq!(started, 42);
    }
}
