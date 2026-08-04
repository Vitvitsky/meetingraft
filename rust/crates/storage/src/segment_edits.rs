//! Журнал ручных правок текста сегментов (Epic 19).
//!
//! Отдельный модуль, потому что `audio_manifest.rs` уже за две тысячи
//! строк, а правки — самостоятельная сущность со своим жизненным циклом.

use domain::{AudioChannel, SegmentEdit};
use rusqlite::{Row, params};

use crate::{AudioManifestError, AudioManifestStore};

/// Колонки журнала в том порядке, которого ждёт [`row_to_edit`].
///
/// Один список на все запросы: рассинхрон порядка колонок между копиями
/// SELECT не поймал бы ни компилятор, ни тест — типы совпадают, и правка
/// просто прочиталась бы с перепутанными полями.
const EDIT_COLUMNS: &str = "id, meeting_id, channel, start_ms, end_ms, original_text,
                            edited_text, created_at_ms, applied_version";

fn row_to_edit(row: &Row<'_>) -> rusqlite::Result<SegmentEdit> {
    let channel: String = row.get(2)?;
    Ok(SegmentEdit {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        channel: AudioChannel::from_code(&channel),
        start_ms: row.get::<_, i64>(3)? as u64,
        end_ms: row.get::<_, i64>(4)? as u64,
        original_text: row.get(5)?,
        edited_text: row.get(6)?,
        created_at_ms: row.get::<_, i64>(7)? as u64,
        applied_version: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
    })
}

impl AudioManifestStore {
    /// Записать правку. Повторный вызов с тем же `id` обновляет текст
    /// правки, её границы и версию: пересбор пересаживает правку на
    /// сегмент новой нарезки, то есть меняет именно `start_ms`/`end_ms`, и
    /// без их обновления перенос был бы тихой пустышкой — версия новая,
    /// координаты старые, а наложение при чтении ищет по ключу «канал,
    /// начало, конец» и такую правку не найдёт никогда.
    ///
    /// `original_text`, `channel` и `created_at_ms` не трогаются.
    /// `original_text` — то, что распознала модель при первой записи; если
    /// бы вторая правка того же места её перезаписывала, после двух правок
    /// подряд исходный текст был бы потерян безвозвратно, и сравнивать или
    /// откатывать стало бы не с чем.
    pub fn upsert_segment_edit(&mut self, edit: &SegmentEdit) -> Result<(), AudioManifestError> {
        self.connection().execute(
            "INSERT INTO segment_edits
             (id, meeting_id, channel, start_ms, end_ms, original_text,
              edited_text, created_at_ms, applied_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               edited_text = excluded.edited_text,
               start_ms = excluded.start_ms,
               end_ms = excluded.end_ms,
               applied_version = excluded.applied_version",
            params![
                edit.id,
                edit.meeting_id,
                edit.channel.code(),
                edit.start_ms as i64,
                edit.end_ms as i64,
                edit.original_text,
                edit.edited_text,
                edit.created_at_ms as i64,
                edit.applied_version.map(|v| v as i64)
            ],
        )?;
        Ok(())
    }

    pub fn delete_segment_edit(&mut self, id: &str) -> Result<(), AudioManifestError> {
        self.connection()
            .execute("DELETE FROM segment_edits WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Все правки встречи по времени начала.
    pub fn list_segment_edits(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<SegmentEdit>, AudioManifestError> {
        self.query_segment_edits(meeting_id, false)
    }

    /// Правки, которые не легли ни на одну версию после пересбора.
    pub fn list_unapplied_segment_edits(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<SegmentEdit>, AudioManifestError> {
        self.query_segment_edits(meeting_id, true)
    }

    /// Правки, применённые именно к этой версии.
    ///
    /// Отдельно от `list_segment_edits`: журнал не чистится при пересборе
    /// и растёт без верхней границы, а наложению на сегменты при чтении
    /// (`list_final_segments`) нужны только правки текущей версии — тянуть
    /// весь журнал ради них было бы лишним чтением из базы.
    pub fn list_segment_edits_for_version(
        &self,
        meeting_id: &str,
        version: u32,
    ) -> Result<Vec<SegmentEdit>, AudioManifestError> {
        let sql = format!(
            "SELECT {EDIT_COLUMNS}
             FROM segment_edits
             WHERE meeting_id = ?1 AND applied_version = ?2
             ORDER BY start_ms, id"
        );
        let mut statement = self.connection().prepare(&sql)?;
        let rows = statement.query_map(params![meeting_id, version], row_to_edit)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn query_segment_edits(
        &self,
        meeting_id: &str,
        only_unapplied: bool,
    ) -> Result<Vec<SegmentEdit>, AudioManifestError> {
        let filter = if only_unapplied {
            "meeting_id = ?1 AND applied_version IS NULL"
        } else {
            "meeting_id = ?1"
        };
        let sql = format!(
            "SELECT {EDIT_COLUMNS}
             FROM segment_edits
             WHERE {filter}
             ORDER BY start_ms, id"
        );
        let mut statement = self.connection().prepare(&sql)?;
        let rows = statement.query_map(params![meeting_id], row_to_edit)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, SegmentEdit};

    use crate::AudioManifestStore;
    use crate::audio_manifest::tests::tmp_root;

    fn edit(id: &str, applied: Option<u32>) -> SegmentEdit {
        SegmentEdit {
            id: id.into(),
            meeting_id: "m1".into(),
            channel: AudioChannel::Mic,
            start_ms: 1000,
            end_ms: 2000,
            original_text: "интра ру".into(),
            edited_text: "intra.ru".into(),
            created_at_ms: 5,
            applied_version: applied,
        }
    }

    #[test]
    fn upsert_list_and_delete_edits() {
        let mut store = AudioManifestStore::open(tmp_root()).expect("store");

        store
            .upsert_segment_edit(&edit("e1", Some(1)))
            .expect("upsert");
        store
            .upsert_segment_edit(&edit("e2", None))
            .expect("upsert");

        let all = store.list_segment_edits("m1").expect("list");
        assert_eq!(all.len(), 2);

        let unapplied = store.list_unapplied_segment_edits("m1").expect("list");
        assert_eq!(unapplied.len(), 1);
        assert_eq!(unapplied[0].id, "e2");

        store.delete_segment_edit("e1").expect("delete");
        assert_eq!(store.list_segment_edits("m1").expect("list").len(), 1);
    }

    #[test]
    fn edit_overrides_segment_text_of_its_version() {
        use domain::FinalSegment;

        let mut store = AudioManifestStore::open(tmp_root()).expect("store");
        store
            .replace_final_segments(
                "m1",
                1,
                &[FinalSegment {
                    index: 0,
                    start_ms: 1000,
                    end_ms: 2000,
                    channel: AudioChannel::Mic,
                    speaker_id: String::new(),
                    speaker_pinned: false,
                    text: "интра ру".into(),
                    text_edited: false,
                }],
            )
            .expect("segments");
        store
            .upsert_segment_edit(&edit("e1", Some(1)))
            .expect("upsert");

        let segments = store.list_final_segments("m1", 1).expect("list");
        assert_eq!(segments[0].text, "intra.ru");
        assert!(segments[0].text_edited);

        // Правка другой версии не видна.
        store
            .replace_final_segments(
                "m1",
                2,
                &[FinalSegment {
                    index: 0,
                    start_ms: 1000,
                    end_ms: 2000,
                    channel: AudioChannel::Mic,
                    speaker_id: String::new(),
                    speaker_pinned: false,
                    text: "интра ру".into(),
                    text_edited: false,
                }],
            )
            .expect("segments");
        let v2 = store.list_final_segments("m1", 2).expect("list");
        assert_eq!(v2[0].text, "интра ру");
        assert!(!v2[0].text_edited);
    }

    /// Пересбор может пересадить на одну позицию сразу две правки (новая
    /// нарезка слила два ранее правленых сегмента в один — Epic 19): у
    /// обеих один и тот же ключ «канал, начало, конец». Побеждать должна
    /// более поздняя по `created_at_ms` — это последнее решение человека.
    /// `e1` создана позже `e2`, но выборка из базы идёт по `id` (`ORDER BY
    /// start_ms, id`), так что в порядке обработки старая правка (`e2`)
    /// идёт последней. Тест ловит наивную реализацию, которая доверяет
    /// порядку выборки, а не времени правки.
    #[test]
    fn edit_collision_on_same_position_prefers_the_latest_by_created_at() {
        use domain::FinalSegment;

        let mut store = AudioManifestStore::open(tmp_root()).expect("store");
        store
            .replace_final_segments(
                "m1",
                1,
                &[FinalSegment {
                    index: 0,
                    start_ms: 1000,
                    end_ms: 2000,
                    channel: AudioChannel::Mic,
                    speaker_id: String::new(),
                    speaker_pinned: false,
                    text: "интра ру".into(),
                    text_edited: false,
                }],
            )
            .expect("segments");

        let mut newer = edit("e1", Some(1));
        newer.created_at_ms = 200;
        newer.edited_text = "новая правка".into();
        store.upsert_segment_edit(&newer).expect("upsert");

        let mut older = edit("e2", Some(1));
        older.created_at_ms = 100;
        older.edited_text = "старая правка".into();
        store.upsert_segment_edit(&older).expect("upsert");

        let segments = store.list_final_segments("m1", 1).expect("list");
        assert_eq!(segments[0].text, "новая правка");
    }

    /// Повторный `upsert_segment_edit` с тем же `id` проходит ветку
    /// `ON CONFLICT ... DO UPDATE`: обновляются текст правки, границы и
    /// версия; `original_text`, канал и время создания остаются от первой
    /// записи, а строка — по-прежнему одна.
    #[test]
    fn upsert_same_id_overwrites_only_edited_fields() {
        let mut store = AudioManifestStore::open(tmp_root()).expect("store");

        store
            .upsert_segment_edit(&edit("e1", None))
            .expect("upsert");

        let mut second = edit("e1", Some(2));
        second.original_text = "другой оригинал".into();
        second.edited_text = "other edit".into();
        second.start_ms = 9000;
        second.end_ms = 9500;
        second.channel = AudioChannel::System;
        second.created_at_ms = 999;
        store.upsert_segment_edit(&second).expect("upsert");

        let all = store.list_segment_edits("m1").expect("list");
        assert_eq!(all.len(), 1, "конфликт по id не должен плодить строки");

        let stored = &all[0];
        // Обновились: то, что и должно.
        assert_eq!(stored.edited_text, "other edit");
        assert_eq!(stored.applied_version, Some(2));
        assert_eq!(stored.start_ms, 9000);
        assert_eq!(stored.end_ms, 9500);
        // Не тронуты: значения остались от первой записи. На них стоит
        // отмена правки и разрешение коллизий.
        assert_eq!(stored.original_text, "интра ру");
        assert_eq!(stored.channel, AudioChannel::Mic);
        assert_eq!(stored.created_at_ms, 5);
    }

    /// Перенос правки на новую версию двигает её границы на границы
    /// нового сегмента. Если запись их не сохранит, правка получит новую
    /// версию со старыми координатами — и наложение при чтении, которое
    /// ищет по ключу «канал, начало, конец», её не найдёт никогда.
    #[test]
    fn moved_edit_lands_on_the_segment_of_the_new_version() {
        use domain::FinalSegment;

        let mut store = AudioManifestStore::open(tmp_root()).expect("store");
        store
            .replace_final_segments(
                "m1",
                2,
                &[FinalSegment {
                    index: 0,
                    start_ms: 900,
                    end_ms: 2100,
                    channel: AudioChannel::Mic,
                    speaker_id: String::new(),
                    speaker_pinned: false,
                    text: "интра ру".into(),
                    text_edited: false,
                }],
            )
            .expect("segments");
        store
            .upsert_segment_edit(&edit("e1", Some(1)))
            .expect("upsert");

        // Так журнал переезжает после пересбора: тот же id, новые границы.
        let mut moved = edit("e1", Some(2));
        moved.start_ms = 900;
        moved.end_ms = 2100;
        store.upsert_segment_edit(&moved).expect("upsert");

        let segments = store.list_final_segments("m1", 2).expect("list");
        assert_eq!(segments[0].text, "intra.ru");
        assert!(segments[0].text_edited);
    }
}
