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
    // 5 — сегменты финального транскрипта (Phase 10). Истина живёт здесь,
    // а body_markdown остаётся рендером: Phase 11 привяжет спикера к
    // сегменту, и по markdown это было бы невозможно без парсинга
    // собственного формата обратно. speaker_id заводится сразу пустым,
    // чтобы схему потом не менять.
    "
    CREATE TABLE IF NOT EXISTS final_segments (
        meeting_id TEXT NOT NULL,
        version INTEGER NOT NULL,
        idx INTEGER NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        channel TEXT NOT NULL,
        speaker_id TEXT NOT NULL DEFAULT '',
        text TEXT NOT NULL,
        PRIMARY KEY (meeting_id, version, idx)
    );
    ",
    // 6 — признак ручной правки спикера (Phase 11). Без него массовое
    // назначение по каналу затирало бы точечные исправления, и человек
    // терял бы свою работу молча.
    "
    ALTER TABLE final_segments
        ADD COLUMN speaker_pinned INTEGER NOT NULL DEFAULT 0;
    ",
    // 7 — вид записи глоссария (Epic 19). Подсказка идёт только в
    // initial_prompt, замена ещё и переписывает готовый текст.
    // Умолчание 1 = Replacement: существующие термины писал человек
    // руками, и менять их поведение миграцией нельзя.
    "
    ALTER TABLE glossary_terms
        ADD COLUMN kind INTEGER NOT NULL DEFAULT 1;
    ",
    // 8 — журнал ручных правок текста (Epic 19). Отдельно от сегментов:
    // пересбор создаёт новую версию с другой нарезкой, и правка,
    // лежащая в таблице сегментов, потерялась бы вместе со старой.
    "
    CREATE TABLE IF NOT EXISTS segment_edits (
        id TEXT PRIMARY KEY NOT NULL,
        meeting_id TEXT NOT NULL,
        channel TEXT NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        original_text TEXT NOT NULL,
        edited_text TEXT NOT NULL,
        created_at_ms INTEGER NOT NULL,
        applied_version INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_segment_edits_meeting
        ON segment_edits(meeting_id, applied_version);
    ",
    // 9 — формат файла чанка (Epic 22). Умолчание `pcm_s16le` — это и есть
    // отсутствие миграции данных: всё уже записанное описывается верно, а
    // сами файлы не трогаются.
    //
    // Колонка на строку, а не настройка на базу: в одной сессии могут
    // соседствовать пачки разных форматов, и это не случайность, а рабочий
    // путь отката при отказе кодера.
    "
    ALTER TABLE audio_manifest ADD COLUMN codec TEXT NOT NULL DEFAULT 'pcm_s16le';
    ",
    // 10 — из чего собран артефакт (Epic 8). Тело Final переписывается на
    // месте семью операциями — правкой текста и назначением спикеров, —
    // и собранный раньше Brief расходился с транскриптом молча. NULL
    // означает «собран до того, как приложение начало это отслеживать»:
    // выдать неизвестное за устаревшее значило бы соврать в другую
    // сторону.
    //
    // Порядок относительно шага 9 не переставлять: `codec` уже уехал в
    // `main` и мог примениться к настоящим базам. Поменяв их местами, мы
    // получили бы базу на версии 9 с колонкой `codec`, которой шаг 10
    // попытался бы добавить её второй раз — и миграция упала бы совсем.
    "
    ALTER TABLE artifacts ADD COLUMN source_version INTEGER;
    ALTER TABLE artifacts ADD COLUMN source_fingerprint TEXT;
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

    /// База, созданная до появления колонки, обязана подняться так, чтобы
    /// её чанки описывались честно: до сжатия формат был только сырой PCM.
    ///
    /// Соврав здесь, мы отдадим старый файл декодеру FLAC.
    #[test]
    fn migration_marks_existing_chunks_as_raw_pcm() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(baseline_schema()).expect("baseline");
        conn.execute(
            "INSERT INTO audio_manifest
             (session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms)
             VALUES ('s1', 'mic', 0, 'sessions/s1/mic/000000.pcm', 16000, 32000, 0)",
            [],
        )
        .expect("legacy row");

        migrate(&conn).expect("migrate");

        let codec: String = conn
            .query_row(
                "SELECT codec FROM audio_manifest WHERE seq = 0",
                [],
                |row| row.get(0),
            )
            .expect("codec");
        assert_eq!(
            codec, "pcm_s16le",
            "старая строка описана неверно — её файл прочитают как FLAC"
        );
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
    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        conn.prepare(&format!("PRAGMA table_info({table})"))
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .expect("table_info readable")
            .iter()
            .any(|name| name == column)
    }

    /// База, уже поднятая до версии 9 предыдущей сборкой, обязана
    /// домигрировать до конца.
    ///
    /// Так выглядит машина человека, который поставил сборку с FLAC раньше,
    /// чем сборку с метками артефактов. Шаги 9 и 10 добавились в разных
    /// ветках, и порядок между ними менять нельзя: если колонка `codec`
    /// станет шагом 10, эта база попробует добавить её второй раз,
    /// миграция упадёт, и приложение вообще не откроет базу.
    ///
    /// Состояние версии 9 здесь записано явно, а не выведено из `STEPS`:
    /// это исторический факт о том, что оставила та сборка, и он не должен
    /// меняться вместе с текущим списком шагов. Ни один тест «с нуля»
    /// такого не увидит — все они мигрируют от пустой базы.
    #[test]
    fn a_database_already_at_step_nine_migrates_the_rest_of_the_way() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(baseline_schema()).expect("baseline");
        for step in &STEPS[1..8] {
            conn.execute_batch(step).expect("шаги 2..8");
        }
        conn.execute_batch(
            "ALTER TABLE audio_manifest ADD COLUMN codec TEXT NOT NULL DEFAULT 'pcm_s16le';
             PRAGMA user_version = 9;",
        )
        .expect("состояние, оставленное сборкой с FLAC");

        migrate(&conn).expect("домигрировать поверх версии 9");

        assert_eq!(current_version(&conn).expect("version"), schema_version());
        assert!(
            has_column(&conn, "artifacts", "source_version"),
            "артефакты остались без source_version"
        );
        assert!(
            has_column(&conn, "artifacts", "source_fingerprint"),
            "артефакты остались без source_fingerprint"
        );
    }

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

    #[test]
    fn existing_glossary_terms_become_replacements() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(baseline_schema()).expect("baseline");
        conn.execute(
            "INSERT INTO glossary_terms
             (id, surface, canonical, language, scope, meeting_id, updated_at_ms)
             VALUES ('t1', 'униффи', 'UniFFI', 'ru', 'global', NULL, 0)",
            [],
        )
        .expect("insert");

        migrate(&conn).expect("migrate");

        let kind: i64 = conn
            .query_row("SELECT kind FROM glossary_terms WHERE id = 't1'", [], |r| {
                r.get(0)
            })
            .expect("kind");
        assert_eq!(kind, 1, "существующий термин остаётся заменой");
    }
}
