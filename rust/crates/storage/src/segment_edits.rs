//! Журнал ручных правок текста сегментов (Epic 19).
//!
//! Отдельный модуль, потому что `audio_manifest.rs` уже за две тысячи
//! строк, а правки — самостоятельная сущность со своим жизненным циклом.

use domain::{AudioChannel, SegmentEdit};
use rusqlite::params;

use crate::{AudioManifestError, AudioManifestStore};

impl AudioManifestStore {
    /// Записать правку. Повторный вызов с тем же `id` обновляет только
    /// `edited_text` и `applied_version` — остальные поля, включая
    /// `original_text`, `start_ms`/`end_ms`, `channel` и `created_at_ms`,
    /// не трогаются. `original_text` — то, что распознала модель при
    /// первой записи; если бы вторая правка того же места её
    /// перезаписывала, после второй правки подряд исходный текст был бы
    /// потерян безвозвратно, и сравнивать/откатывать стало бы не с чем.
    pub fn upsert_segment_edit(&mut self, edit: &SegmentEdit) -> Result<(), AudioManifestError> {
        self.connection().execute(
            "INSERT INTO segment_edits
             (id, meeting_id, channel, start_ms, end_ms, original_text,
              edited_text, created_at_ms, applied_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               edited_text = excluded.edited_text,
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

    fn query_segment_edits(
        &self,
        meeting_id: &str,
        only_unapplied: bool,
    ) -> Result<Vec<SegmentEdit>, AudioManifestError> {
        let sql = if only_unapplied {
            "SELECT id, meeting_id, channel, start_ms, end_ms, original_text,
                    edited_text, created_at_ms, applied_version
             FROM segment_edits
             WHERE meeting_id = ?1 AND applied_version IS NULL
             ORDER BY start_ms, id"
        } else {
            "SELECT id, meeting_id, channel, start_ms, end_ms, original_text,
                    edited_text, created_at_ms, applied_version
             FROM segment_edits
             WHERE meeting_id = ?1
             ORDER BY start_ms, id"
        };
        let mut statement = self.connection().prepare(sql)?;
        let rows = statement.query_map(params![meeting_id], |row| {
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
        })?;
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

    /// Повторный `upsert_segment_edit` с тем же `id` проходит ветку
    /// `ON CONFLICT ... DO UPDATE`: обновляются только `edited_text` и
    /// `applied_version`, остальные поля (включая `original_text`) должны
    /// остаться от первой записи, а строка — по-прежнему одна.
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
        // Не тронуты: значения остались от первой записи.
        assert_eq!(stored.original_text, "интра ру");
        assert_eq!(stored.start_ms, 1000);
        assert_eq!(stored.end_ms, 2000);
        assert_eq!(stored.channel, AudioChannel::Mic);
        assert_eq!(stored.created_at_ms, 5);
    }
}
