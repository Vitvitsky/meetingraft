# Epic 19, ядро на Rust — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Правка сегмента Final живёт отдельным журналом поверх версий транскрипта, переживает пересбор и автоматически пополняет глоссарий безопасными подсказками.

**Architecture:** Сегменты остаются производными от распознавания — правки хранятся в новой таблице `segment_edits` и накладываются при чтении. Глоссарий получает вид записи: `Hint` идёт только в `initial_prompt`, `Replacement` ещё и переписывает текст. Из правки автоматически рождается `Hint`, повышение до `Replacement` требует явного жеста из интерфейса.

**Tech Stack:** Rust, rusqlite, UniFFI. Крейты `domain`, `storage`, `glossary`, `postcall`, `ffi`.

Спека: `docs/superpowers/specs/2026-08-04-epic-19-transcript-edits-design.md`.

## Global Constraints

- Миграции только добавлением шага в `STEPS` (`rust/crates/storage/src/migrations.rs`); существующие шаги не редактируются — база у пользователя уже на них стоит.
- Существующие термины глоссария после миграции ведут себя как раньше: `kind` по умолчанию `Replacement`.
- Бизнес-логика в Rust, слой FFI только отдаёт данные (`AGENTS.md`, done criteria).
- Комментарии и документация в коде — по-русски, как в остальном проекте.
- Сообщения коммитов — по-английски.
- Проверка после каждой задачи: `cd rust && cargo test -p <крейт>`; перед коммитом `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- Полный `cargo test` по всему workspace на этой машине не влезает в память — гонять по крейтам.

---

### Task 1: Вид записи глоссария

**Files:**
- Modify: `rust/crates/domain/src/glossary.rs`
- Modify: `rust/crates/storage/src/migrations.rs` (добавить шаг 7 в конец `STEPS`)
- Modify: `rust/crates/storage/src/audio_manifest.rs:874` (`list_glossary_terms`), `:984` (`write_glossary_term`)
- Modify (компиляция): `rust/crates/glossary/src/lib.rs`, `rust/crates/glossary/src/csv_import.rs`, `rust/crates/ffi/src/lib.rs` — всего 17 мест конструирования `GlossaryTerm`

**Interfaces:**
- Produces: `domain::GlossaryKind { Hint, Replacement }`, поле `GlossaryTerm.kind: GlossaryKind`, методы `GlossaryKind::code(self) -> i64` и `GlossaryKind::from_code(i64) -> GlossaryKind`

- [ ] **Step 1: Написать падающий тест на умолчание миграции**

В `rust/crates/storage/src/migrations.rs`, в `mod tests`:

```rust
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
        .query_row("SELECT kind FROM glossary_terms WHERE id = 't1'", [], |r| r.get(0))
        .expect("kind");
    assert_eq!(kind, 1, "существующий термин остаётся заменой");
}
```

- [ ] **Step 2: Убедиться, что тест падает**

Run: `cd rust && cargo test -p meetingraft-storage existing_glossary_terms_become_replacements`
Expected: FAIL — `no such column: kind`

- [ ] **Step 3: Добавить шаг миграции**

В конец массива `STEPS` в `rust/crates/storage/src/migrations.rs`:

```rust
    // 7 — вид записи глоссария (Epic 19). Подсказка идёт только в
    // initial_prompt, замена ещё и переписывает готовый текст.
    // Умолчание 1 = Replacement: существующие термины писал человек
    // руками, и менять их поведение миграцией нельзя.
    "
    ALTER TABLE glossary_terms
        ADD COLUMN kind INTEGER NOT NULL DEFAULT 1;
    ",
```

- [ ] **Step 4: Убедиться, что тест проходит**

Run: `cd rust && cargo test -p meetingraft-storage existing_glossary_terms_become_replacements`
Expected: PASS

- [ ] **Step 5: Добавить тип в domain**

В `rust/crates/domain/src/glossary.rs`, перед `GlossaryTerm`:

```rust
/// Что термин делает с текстом.
///
/// Разделение вынужденное: `normalize_caption` заменяет безусловно и
/// везде, поэтому термин, родившийся из грамматической правки, переписывал
/// бы все будущие тексты. Подсказка такого сделать не может — цена ошибки
/// в `initial_prompt` мизерная и обратимая.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlossaryKind {
    /// Только подсказка Whisper.
    Hint,
    /// Замена surface → canonical в готовом тексте.
    Replacement,
}

impl GlossaryKind {
    pub fn code(self) -> i64 {
        match self {
            Self::Hint => 0,
            Self::Replacement => 1,
        }
    }

    /// Неизвестный код читается как подсказка: она безопаснее замены.
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => Self::Replacement,
            _ => Self::Hint,
        }
    }
}
```

И поле в структуру:

```rust
pub struct GlossaryTerm {
    pub id: String,
    pub surface: String,
    pub canonical: String,
    pub language: SpeechLanguage,
    pub scope: GlossaryScope,
    pub kind: GlossaryKind,
}
```

- [ ] **Step 6: Прокинуть поле через storage**

В `list_glossary_terms` (`audio_manifest.rs:874`) добавить `kind` в SELECT и в конструктор:

```rust
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
```

В `write_glossary_term` (`audio_manifest.rs:984`) — колонка в INSERT:

```rust
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
```

Импорт `GlossaryKind` добавить в `use domain::{...}` в начале файла.

- [ ] **Step 7: Починить оставшиеся места конструирования**

Run: `cd rust && cargo build -p meetingraft-glossary -p meetingraft-ffi -p meetingraft-storage 2>&1 | grep "missing field"`

В каждое место добавить `kind: GlossaryKind::Replacement` — кроме `csv_import.rs`, где импорт тоже даёт замены (человек привёз готовый словарь замен).

- [ ] **Step 8: Тест на круговой обход**

В `mod tests` файла `audio_manifest.rs`, рядом с `glossary_upsert_list_delete`:

```rust
#[test]
fn glossary_kind_round_trips() {
    let mut store = AudioManifestStore::open(tmp_root()).expect("store");
    let term = GlossaryTerm {
        id: "t1".into(),
        surface: "интра ру".into(),
        canonical: "intra.ru".into(),
        language: SpeechLanguage::Ru,
        scope: GlossaryScope::Global,
        kind: GlossaryKind::Hint,
    };
    store.upsert_glossary_term(&term, 0).expect("upsert");

    let read = store.list_glossary_terms().expect("list");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].kind, GlossaryKind::Hint);
}
```

- [ ] **Step 9: Прогон и коммит**

```bash
cd rust && cargo test -p meetingraft-domain -p meetingraft-storage -p meetingraft-glossary -p meetingraft-ffi \
  && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/
git commit -m "feat: glossary term kind — hint or replacement"
```

---

### Task 2: Замену применяет только Replacement

**Files:**
- Modify: `rust/crates/glossary/src/engine.rs:21-24` (`normalize_caption`)
- Test: `rust/crates/glossary/src/lib.rs` (`mod tests`)

**Interfaces:**
- Consumes: `GlossaryKind` из Task 1
- Produces: поведение `GlossaryEngine::normalize_caption` — только `Replacement`; `build_whisper_prompt` — оба вида, без изменений

- [ ] **Step 1: Написать падающие тесты**

В `rust/crates/glossary/src/lib.rs`, в `mod tests`:

```rust
#[test]
fn hint_does_not_rewrite_text() {
    let engine = GlossaryEngine::from_terms(vec![GlossaryTerm {
        id: "1".into(),
        surface: "пошли".into(),
        canonical: "пошёл".into(),
        language: SpeechLanguage::Ru,
        scope: GlossaryScope::Global,
        kind: GlossaryKind::Hint,
    }]);

    assert_eq!(engine.normalize_caption("пошли дальше"), "пошли дальше");
}

#[test]
fn hint_still_reaches_whisper_prompt() {
    let engine = GlossaryEngine::from_terms(vec![GlossaryTerm {
        id: "1".into(),
        surface: "интра ру".into(),
        canonical: "intra.ru".into(),
        language: SpeechLanguage::Ru,
        scope: GlossaryScope::Global,
        kind: GlossaryKind::Hint,
    }]);

    assert_eq!(engine.build_whisper_prompt(100), "intra.ru");
}
```

- [ ] **Step 2: Убедиться, что первый падает, второй проходит**

Run: `cd rust && cargo test -p meetingraft-glossary hint_`
Expected: `hint_does_not_rewrite_text` FAIL (вернулось «пошёл дальше»), `hint_still_reaches_whisper_prompt` PASS

- [ ] **Step 3: Отфильтровать вид в normalize_caption**

В `rust/crates/glossary/src/engine.rs`:

```rust
    /// Заменяет целые surface-фразы на canonical-формы.
    ///
    /// Подсказки не участвуют: они существуют ради `initial_prompt` и
    /// готовый текст не трогают (Epic 19).
    pub fn normalize_caption(&self, text: &str) -> String {
        let replacements: Vec<GlossaryTerm> = self
            .terms
            .iter()
            .filter(|term| term.kind == GlossaryKind::Replacement)
            .cloned()
            .collect();
        normalize::normalize(text, &replacements)
    }
```

Импорт `GlossaryKind` добавить в `use domain::{...}`.

- [ ] **Step 4: Прогон**

Run: `cd rust && cargo test -p meetingraft-glossary`
Expected: PASS

- [ ] **Step 5: Коммит**

```bash
git add rust/crates/glossary
git commit -m "feat: glossary hints no longer rewrite text"
```

---

### Task 3: Таблица журнала правок и доступ к ней

**Files:**
- Modify: `rust/crates/domain/src/postcall.rs` (структура `SegmentEdit`)
- Modify: `rust/crates/storage/src/migrations.rs` (шаг 8)
- Create: `rust/crates/storage/src/segment_edits.rs`
- Modify: `rust/crates/storage/src/lib.rs` (`mod segment_edits;`)

`audio_manifest.rs` уже за две тысячи строк — журнал кладём отдельным модулем с `impl AudioManifestStore`, а не дописываем туда же.

**Interfaces:**
- Produces: `domain::SegmentEdit`, методы `AudioManifestStore::upsert_segment_edit`, `delete_segment_edit`, `list_segment_edits`, `list_unapplied_segment_edits`

- [ ] **Step 1: Структура в domain**

В `rust/crates/domain/src/postcall.rs`:

```rust
/// Ручная правка текста сегмента.
///
/// Живёт отдельно от сегментов: сегменты производны от распознавания, а
/// пересбор создаёт новую версию с другой нарезкой. Журнал переживает
/// пересбор, таблица сегментов — нет (Epic 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentEdit {
    pub id: String,
    pub meeting_id: String,
    pub channel: AudioChannel,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Что распознала модель.
    pub original_text: String,
    /// Что ввёл человек.
    pub edited_text: String,
    pub created_at_ms: u64,
    /// Версия, в которой правка сейчас применена. `None` — не применилась.
    pub applied_version: Option<u32>,
}
```

Экспортировать из `rust/crates/domain/src/lib.rs` рядом с `FinalSegment`.

- [ ] **Step 2: Написать падающий тест доступа**

Создать `rust/crates/storage/src/segment_edits.rs` с одним лишь `mod tests`:

```rust
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

        store.upsert_segment_edit(&edit("e1", Some(1))).expect("upsert");
        store.upsert_segment_edit(&edit("e2", None)).expect("upsert");

        let all = store.list_segment_edits("m1").expect("list");
        assert_eq!(all.len(), 2);

        let unapplied = store.list_unapplied_segment_edits("m1").expect("list");
        assert_eq!(unapplied.len(), 1);
        assert_eq!(unapplied[0].id, "e2");

        store.delete_segment_edit("e1").expect("delete");
        assert_eq!(store.list_segment_edits("m1").expect("list").len(), 1);
    }
}
```

В `rust/crates/storage/src/lib.rs` добавить `mod segment_edits;`.

Плюс отдельный тест на ветку `ON CONFLICT` — повторная запись с тем же `id`:

```rust
    #[test]
    fn repeated_upsert_keeps_recognized_text() {
        let mut store = AudioManifestStore::open(tmp_root()).expect("store");
        store.upsert_segment_edit(&edit("e1", Some(1))).expect("upsert");

        let mut again = edit("e1", Some(2));
        again.edited_text = "intra.ru точно".into();
        again.original_text = "подменённое".into();
        store.upsert_segment_edit(&again).expect("upsert");

        let all = store.list_segment_edits("m1").expect("list");
        assert_eq!(all.len(), 1, "правка того же места не копится");
        assert_eq!(all[0].edited_text, "intra.ru точно");
        assert_eq!(all[0].applied_version, Some(2));
        assert_eq!(
            all[0].original_text, "интра ру",
            "распознанное переживает перезапись: иначе вернуть текст к исходному будет нечем"
        );
    }
```

- [ ] **Step 3: Убедиться, что не компилируется**

Run: `cd rust && cargo test -p meetingraft-storage upsert_list_and_delete_edits`
Expected: FAIL — `no method named upsert_segment_edit`

- [ ] **Step 4: Добавить таблицу**

В конец `STEPS` в `migrations.rs`:

```rust
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
```

- [ ] **Step 5: Реализовать доступ**

В начало `rust/crates/storage/src/segment_edits.rs`, перед `mod tests`:

```rust
//! Журнал ручных правок текста сегментов (Epic 19).
//!
//! Отдельный модуль, потому что `audio_manifest.rs` уже за две тысячи
//! строк, а правки — самостоятельная сущность со своим жизненным циклом.

use domain::{AudioChannel, SegmentEdit};
use rusqlite::params;

use crate::{AudioManifestError, AudioManifestStore};

impl AudioManifestStore {
    /// Записать правку; повторный вызов с тем же id перезаписывает.
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
```

Две точечные правки в `audio_manifest.rs`, без перекладывания кода.

Первая — видимость тестового помощника: `mod tests` (строка 1101) и `fn tmp_root()` (строка 1112) объявлены приватными, а модуль журнала им сосед, а не потомок. Обоим добавить `pub(crate)`. Копировать `tmp_root` к себе нельзя — это дословное дублирование, которое разбор диффа считает дефектом.

Вторая — доступ к соединению. Поле `conn` приватное, добавить внутрь `impl AudioManifestStore`:

```rust
    /// Соединение для модулей крейта, живущих в соседних файлах.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }
```

- [ ] **Step 6: Прогон**

Run: `cd rust && cargo test -p meetingraft-storage upsert_list_and_delete_edits`
Expected: PASS

- [ ] **Step 7: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/
git commit -m "feat: journal of manual segment edits"
```

---

### Task 4: Правки накладываются при чтении сегментов

**Files:**
- Modify: `rust/crates/domain/src/postcall.rs` (поле `FinalSegment.text_edited`)
- Modify: `rust/crates/storage/src/audio_manifest.rs:556` (`list_final_segments`)
- Modify: `rust/crates/postcall/src/merge.rs:49`, `rust/crates/postcall/src/speakers.rs:89` (конструирование `FinalSegment`)
- Test: `rust/crates/storage/src/segment_edits.rs`

**Interfaces:**
- Consumes: `list_segment_edits` из Task 3
- Produces: `FinalSegment.text_edited: bool`; `list_final_segments` возвращает правленый текст

- [ ] **Step 1: Написать падающий тест**

В `mod tests` файла `segment_edits.rs`:

```rust
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
    store.upsert_segment_edit(&edit("e1", Some(1))).expect("upsert");

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
```

- [ ] **Step 2: Убедиться, что не компилируется**

Run: `cd rust && cargo test -p meetingraft-storage edit_overrides_segment_text_of_its_version`
Expected: FAIL — `struct FinalSegment has no field named text_edited`

- [ ] **Step 3: Добавить поле**

В `rust/crates/domain/src/postcall.rs`, в `FinalSegment` после `text`:

```rust
    /// Текст заменён ручной правкой из журнала (Epic 19).
    ///
    /// Не хранится в таблице сегментов — вычисляется при чтении, потому
    /// что источником истины остаётся журнал.
    pub text_edited: bool,
```

В `merge.rs:49` и `speakers.rs:89` дописать `text_edited: false` — свежесобранные сегменты правок не несут.

- [ ] **Step 4: Наложить журнал при чтении**

В `list_final_segments` (`audio_manifest.rs:556`) после сборки `segments`:

```rust
        let mut segments = rows.collect::<Result<Vec<_>, _>>()?;

        // Правка перекрывает распознанное: журнал — источник истины для
        // текста, таблица сегментов хранит то, что выдала модель.
        let edits = self.list_segment_edits(meeting_id)?;
        for segment in &mut segments {
            let applied = edits.iter().find(|edit| {
                edit.applied_version == Some(version)
                    && edit.channel == segment.channel
                    && edit.start_ms == segment.start_ms
                    && edit.end_ms == segment.end_ms
            });
            if let Some(edit) = applied {
                segment.text = edit.edited_text.clone();
                segment.text_edited = true;
            }
        }
        Ok(segments)
```

Тело `query_map` при этом ставит `text_edited: false`.

- [ ] **Step 5: Прогон**

Run: `cd rust && cargo test -p meetingraft-storage -p postcall`
Expected: PASS

- [ ] **Step 6: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/
git commit -m "feat: apply edits to segments on read"
```

---

### Task 5: Сопоставление правок при пересборе

**Files:**
- Create: `rust/crates/postcall/src/edits.rs`
- Modify: `rust/crates/postcall/src/lib.rs` (`mod edits;` + реэкспорт)

**Interfaces:**
- Consumes: `domain::SegmentEdit`, `domain::FinalSegment`
- Produces: `postcall::reattach_edits(edits: &[SegmentEdit], segments: &[FinalSegment], version: u32) -> Vec<SegmentEdit>` — возвращает правки с обновлённым `applied_version` (`None`, если места не нашлось)

Чистая функция без базы: так она тестируется без временных каталогов, а вызов из FFI просто читает журнал, прогоняет и записывает обратно.

- [ ] **Step 1: Написать падающие тесты**

Создать `rust/crates/postcall/src/edits.rs`:

```rust
#[cfg(test)]
mod tests {
    use domain::{AudioChannel, FinalSegment, SegmentEdit};

    use super::reattach_edits;

    fn segment(index: u32, start_ms: u64, end_ms: u64, text: &str) -> FinalSegment {
        FinalSegment {
            index,
            start_ms,
            end_ms,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_pinned: false,
            text: text.into(),
            text_edited: false,
        }
    }

    fn edit(start_ms: u64, end_ms: u64, original: &str) -> SegmentEdit {
        SegmentEdit {
            id: "e1".into(),
            meeting_id: "m1".into(),
            channel: AudioChannel::Mic,
            start_ms,
            end_ms,
            original_text: original.into(),
            edited_text: "intra.ru".into(),
            created_at_ms: 0,
            applied_version: Some(1),
        }
    }

    #[test]
    fn attaches_to_overlapping_segment_containing_original_text() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![segment(0, 900, 2100, "смотри интра ру там")];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].applied_version, Some(2));
        assert_eq!(result[0].start_ms, 900, "диапазон переезжает на новый сегмент");
        assert_eq!(result[0].end_ms, 2100);
    }

    #[test]
    fn drops_when_original_text_is_gone() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![segment(0, 900, 2100, "совсем другое распознавание")];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].applied_version, None);
    }

    #[test]
    fn picks_candidate_with_largest_overlap() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![
            segment(0, 900, 1200, "интра ру"),
            segment(1, 1100, 2100, "интра ру ещё раз"),
        ];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].start_ms, 1100, "победил больший перекрыв");
    }

    #[test]
    fn ignores_segments_of_other_channel() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let mut other = segment(0, 900, 2100, "интра ру");
        other.channel = AudioChannel::System;

        let result = reattach_edits(&edits, &[other], 2);

        assert_eq!(result[0].applied_version, None);
    }
}
```

- [ ] **Step 2: Убедиться, что не компилируется**

Run: `cd rust && cargo test -p meetingraft-postcall reattach`
Expected: FAIL — `cannot find function reattach_edits`

- [ ] **Step 3: Реализовать**

В начало `rust/crates/postcall/src/edits.rs`:

```rust
//! Перенос ручных правок на новую версию Final (Epic 19).
//!
//! Пересбор нарезает сегменты заново: границы и индексы другие, поэтому
//! правку нельзя привязать к номеру. Привязка идёт по перекрытию времени
//! и наличию исходного текста — если модель распознала это место иначе,
//! правка не применяется и человек видит её в отдельном разделе.

use domain::{FinalSegment, SegmentEdit};

/// Пересадить правки на сегменты версии `version`.
///
/// Правка без подходящего сегмента получает `applied_version = None`:
/// молча терять ручную работу нельзя.
///
/// Две правки могут сесть на один сегмент — если новая нарезка слила
/// два ранее правленых сегмента в один. Это разрешено: побеждать при
/// чтении будет более поздняя по `created_at_ms`, как и при обычной
/// повторной правке одного места.
pub fn reattach_edits(
    edits: &[SegmentEdit],
    segments: &[FinalSegment],
    version: u32,
) -> Vec<SegmentEdit> {
    edits
        .iter()
        .map(|edit| {
            let best = segments
                .iter()
                .filter(|segment| segment.channel == edit.channel)
                .filter(|segment| segment.text.contains(edit.original_text.as_str()))
                .filter_map(|segment| overlap_ms(edit, segment).map(|ms| (ms, segment)))
                .max_by_key(|(ms, segment)| (*ms, std::cmp::Reverse(segment.index)));

            let mut moved = edit.clone();
            match best {
                Some((_, segment)) => {
                    moved.start_ms = segment.start_ms;
                    moved.end_ms = segment.end_ms;
                    moved.applied_version = Some(version);
                }
                None => moved.applied_version = None,
            }
            moved
        })
        .collect()
}

/// Длина пересечения диапазонов; `None`, если не пересекаются.
fn overlap_ms(edit: &SegmentEdit, segment: &FinalSegment) -> Option<u64> {
    let start = edit.start_ms.max(segment.start_ms);
    let end = edit.end_ms.min(segment.end_ms);
    (end > start).then(|| end - start)
}
```

В `rust/crates/postcall/src/lib.rs` добавить `mod edits;` и `pub use edits::reattach_edits;`.

- [ ] **Step 4: Прогон**

Run: `cd rust && cargo test -p meetingraft-postcall reattach`
Expected: PASS, все четыре

- [ ] **Step 5: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/postcall
git commit -m "feat: reattach manual edits to a new Final version"
```

---

### Task 6: Термин из правки

**Files:**
- Create: `rust/crates/postcall/src/term_from_edit.rs`
- Modify: `rust/crates/postcall/src/lib.rs`

**Interfaces:**
- Consumes: `postcall::diff_words`, `postcall::DiffOp`
- Produces: `postcall::term_from_edit(original: &str, edited: &str) -> Option<(String, String)>` — пара `(surface, canonical)`

- [ ] **Step 1: Написать падающие тесты**

Создать `rust/crates/postcall/src/term_from_edit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::term_from_edit;

    #[test]
    fn single_word_replacement_becomes_term() {
        let result = term_from_edit("зашли на интра ру вчера", "зашли на intra.ru вчера");
        assert_eq!(result, Some(("интра ру".into(), "intra.ru".into())));
    }

    #[test]
    fn rewritten_sentence_gives_nothing() {
        let result = term_from_edit(
            "ну вот это самое надо бы посмотреть наверное",
            "нужно проверить это на следующей неделе обязательно",
        );
        assert_eq!(result, None, "правка смысла термином не становится");
    }

    // Общих слов в заменяемой части быть не должно: иначе LCS схлопнет их
    // в совпадение, участков Removed не останется, и тест пройдёт ещё до
    // проверки порога — то есть не проверит ничего.
    #[test]
    fn long_side_gives_nothing() {
        let result = term_from_edit(
            "открой интра ру",
            "открой внутренний портал нашей компании",
        );
        assert_eq!(result, None, "больше трёх слов с одной стороны — не термин");
    }

    #[test]
    fn pure_insertion_gives_nothing() {
        let result = term_from_edit("зашли вчера", "зашли на intra.ru вчера");
        assert_eq!(result, None, "нечего заменять — нет surface");
    }

    #[test]
    fn identical_text_gives_nothing() {
        assert_eq!(term_from_edit("одно и то же", "одно и то же"), None);
    }
}
```

- [ ] **Step 2: Убедиться, что не компилируется**

Run: `cd rust && cargo test -p meetingraft-postcall term_from_edit`
Expected: FAIL — `cannot find function term_from_edit`

- [ ] **Step 3: Реализовать**

В начало `rust/crates/postcall/src/term_from_edit.rs`:

```rust
//! Извлечение термина глоссария из ручной правки (Epic 19).
//!
//! Распознанный текст и есть surface, введённый — canonical: оба поля
//! заполняются из действия, и человеку не приходится понимать схему.
//!
//! Термином становится только короткая замена. Длинная — это правка
//! смысла, а не словарная, и в глоссарии она стала бы мусором, который
//! через `initial_prompt` портит распознавание.

use crate::{DiffOp, diff_words};

/// Сколько слов с каждой стороны ещё считается термином.
const MAX_WORDS: usize = 3;

/// Пара `(surface, canonical)` или `None`, если правка не словарная.
///
/// Берётся ровно одна замена: несколько правок в одном сегменте
/// разобрать однозначно нельзя, и угадывать здесь хуже, чем промолчать.
pub fn term_from_edit(original: &str, edited: &str) -> Option<(String, String)> {
    let spans = diff_words(original, edited);

    // Порядок соседей не фиксирован: грубая ветка diff_words ставит
    // Removed перед Added, LCS может выдать наоборот. surface всегда
    // берётся из Removed — это то, что распознала модель.
    let mut pair: Option<(String, String)> = None;
    let mut index = 0;
    while index + 1 < spans.len() {
        let (left, right) = (&spans[index], &spans[index + 1]);
        let found = match (left.op, right.op) {
            (DiffOp::Removed, DiffOp::Added) => Some((&left.text, &right.text)),
            (DiffOp::Added, DiffOp::Removed) => Some((&right.text, &left.text)),
            _ => None,
        };
        if let Some((removed, added)) = found {
            if pair.is_some() {
                return None;
            }
            pair = Some((removed.trim().to_owned(), added.trim().to_owned()));
            index += 2;
            continue;
        }
        index += 1;
    }

    let (surface, canonical) = pair?;
    if surface.is_empty() || canonical.is_empty() {
        return None;
    }
    if word_count(&surface) > MAX_WORDS || word_count(&canonical) > MAX_WORDS {
        return None;
    }
    Some((surface, canonical))
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}
```

В `lib.rs` добавить `mod term_from_edit;` и `pub use term_from_edit::term_from_edit;`.

- [ ] **Step 4: Прогон**

Run: `cd rust && cargo test -p meetingraft-postcall term_from_edit`
Expected: PASS

- [ ] **Step 5: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/postcall
git commit -m "feat: derive glossary term from a manual edit"
```

---

### Task 7: Правка сохраняется и рождает подсказку

**Files:**
- Create: `rust/crates/postcall/src/edit_service.rs`
- Modify: `rust/crates/postcall/src/lib.rs`

**Interfaces:**
- Consumes: `term_from_edit` (Task 6), `GlossaryKind` (Task 1), `SegmentEdit` (Task 3)
- Produces: `postcall::plan_edit(...) -> EditOutcome` — чистая функция, решающая, что записать; запись делает FFI

```rust
pub struct EditOutcome {
    /// `None` — правку надо удалить: текст вернули к исходному.
    pub edit: Option<SegmentEdit>,
    /// Термин, который надо записать. Область уже выбрана.
    pub term: Option<GlossaryTerm>,
}
```

- [ ] **Step 1: Написать падающие тесты**

Создать `rust/crates/postcall/src/edit_service.rs`:

```rust
#[cfg(test)]
mod tests {
    use domain::{
        AudioChannel, FinalSegment, GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage,
    };

    use super::{EditOutcome, plan_edit};

    fn segment() -> FinalSegment {
        FinalSegment {
            index: 0,
            start_ms: 1000,
            end_ms: 2000,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_pinned: false,
            text: "зашли на интра ру".into(),
            text_edited: false,
        }
    }

    #[test]
    fn edit_produces_meeting_hint() {
        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на intra.ru",
            SpeechLanguage::Ru,
            &[],
            "edit-1",
            "term-1",
            42,
        );

        let edit = outcome.edit.expect("правка записывается");
        assert_eq!(edit.original_text, "зашли на интра ру");
        assert_eq!(edit.applied_version, Some(1));

        let term = outcome.term.expect("термин рождается сам");
        assert_eq!(term.kind, GlossaryKind::Hint, "автоматически только подсказка");
        assert_eq!(term.scope, GlossaryScope::Meeting { meeting_id: "m1".into() });
        assert_eq!(term.surface, "интра ру");
        assert_eq!(term.canonical, "intra.ru");
    }

    #[test]
    fn repeat_in_another_meeting_promotes_hint_to_global() {
        let existing = GlossaryTerm {
            id: "old".into(),
            surface: "Интра Ру".into(),
            canonical: "intra.ru".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting { meeting_id: "m0".into() },
            kind: GlossaryKind::Hint,
        };

        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на intra.ru",
            SpeechLanguage::Ru,
            &[existing],
            "edit-1",
            "term-1",
            42,
        );

        let term = outcome.term.expect("термин");
        assert_eq!(term.scope, GlossaryScope::Global, "повтор в другой встрече поднимает область");
        assert_eq!(term.kind, GlossaryKind::Hint, "вид не меняется — поднимается только область");
    }

    #[test]
    fn returning_original_text_removes_edit() {
        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "зашли на интра ру",
            SpeechLanguage::Ru,
            &[],
            "edit-1",
            "term-1",
            42,
        );

        assert!(outcome.edit.is_none(), "возврат к исходному — это отмена");
        assert!(outcome.term.is_none());
    }

    #[test]
    fn sentence_rewrite_saves_edit_without_term() {
        let outcome = plan_edit(
            "m1",
            1,
            &segment(),
            "надо будет посмотреть портал на следующей неделе",
            SpeechLanguage::Ru,
            &[],
            "edit-1",
            "term-1",
            42,
        );

        assert!(outcome.edit.is_some(), "правка сохраняется всегда");
        assert!(outcome.term.is_none(), "но термином не становится");
    }
}
```

- [ ] **Step 2: Убедиться, что не компилируется**

Run: `cd rust && cargo test -p meetingraft-postcall plan_edit`
Expected: FAIL — `cannot find function plan_edit`

- [ ] **Step 3: Реализовать**

В начало `rust/crates/postcall/src/edit_service.rs`:

```rust
//! Что происходит при правке текста сегмента (Epic 19).
//!
//! Чистая функция: решает, что записать, но ничего не пишет. Так решение
//! тестируется без базы, а слой FFI остаётся тонким (`AGENTS.md`).

use domain::{
    FinalSegment, GlossaryKind, GlossaryScope, GlossaryTerm, SegmentEdit, SpeechLanguage,
};

use crate::term_from_edit;

/// Что нужно записать по итогам правки.
pub struct EditOutcome {
    /// `None` — правку надо удалить: текст вернули к исходному.
    pub edit: Option<SegmentEdit>,
    /// Термин к записи. `None` — правка не словарная.
    pub term: Option<GlossaryTerm>,
}

/// Разобрать правку: журнал плюс, возможно, подсказка в глоссарий.
///
/// `existing_terms` нужны, чтобы поймать повтор той же пары в другой
/// встрече: подсказка при повторе поднимается в глобальную область сама,
/// потому что готовый текст она не трогает и ошибиться ею нечем.
#[allow(clippy::too_many_arguments)]
pub fn plan_edit(
    meeting_id: &str,
    version: u32,
    segment: &FinalSegment,
    edited_text: &str,
    language: SpeechLanguage,
    existing_terms: &[GlossaryTerm],
    edit_id: &str,
    term_id: &str,
    now_ms: u64,
) -> EditOutcome {
    let edited = edited_text.trim();
    if edited == segment.text.trim() {
        return EditOutcome { edit: None, term: None };
    }

    let edit = SegmentEdit {
        id: edit_id.to_owned(),
        meeting_id: meeting_id.to_owned(),
        channel: segment.channel,
        start_ms: segment.start_ms,
        end_ms: segment.end_ms,
        original_text: segment.text.clone(),
        edited_text: edited.to_owned(),
        created_at_ms: now_ms,
        applied_version: Some(version),
    };

    let term = term_from_edit(&segment.text, edited).map(|(surface, canonical)| {
        let seen_elsewhere = existing_terms.iter().any(|term| {
            term.kind == GlossaryKind::Hint
                && term.surface.to_lowercase() == surface.to_lowercase()
                && term.canonical.to_lowercase() == canonical.to_lowercase()
                && !matches!(&term.scope, GlossaryScope::Meeting { meeting_id: id } if id == meeting_id)
        });

        GlossaryTerm {
            id: term_id.to_owned(),
            surface,
            canonical,
            language,
            scope: if seen_elsewhere {
                GlossaryScope::Global
            } else {
                GlossaryScope::Meeting { meeting_id: meeting_id.to_owned() }
            },
            kind: GlossaryKind::Hint,
        }
    });

    EditOutcome { edit: Some(edit), term }
}
```

В `lib.rs` добавить `mod edit_service;` и `pub use edit_service::{EditOutcome, plan_edit};`.

- [ ] **Step 4: Прогон**

Run: `cd rust && cargo test -p meetingraft-postcall plan_edit`
Expected: PASS, все четыре

- [ ] **Step 5: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/postcall
git commit -m "feat: segment edit spawns a glossary hint"
```

---

### Task 8: Замена применяется ко всем вхождениям

По спеке кнопка «заменять всюду» не только меняет вид термина, но и правит остальные вхождения в этой же встрече: исправив «интра ру» один раз, человек ждёт, что исправились все.

Применение идёт **через журнал**, а не переписыванием таблицы сегментов. Иначе распознанное было бы потеряно, а инвариант «сегменты производны от распознавания» — нарушен.

**Files:**
- Modify: `rust/crates/postcall/src/edit_service.rs`
- Modify: `rust/crates/postcall/src/lib.rs` (реэкспорт)

**Interfaces:**
- Consumes: `GlossaryTerm` (Task 1), `SegmentEdit` (Task 3)
- Produces: `postcall::occurrences_to_edit(term: &GlossaryTerm, meeting_id: &str, version: u32, segments: &[FinalSegment], existing: &[SegmentEdit], now_ms: u64, ids: &mut dyn Iterator<Item = String>) -> Vec<SegmentEdit>`

- [ ] **Step 1: Написать падающие тесты**

В `mod tests` файла `edit_service.rs`:

```rust
#[test]
fn replacement_covers_other_occurrences() {
    use super::occurrences_to_edit;
    use domain::SegmentEdit;

    let term = GlossaryTerm {
        id: "t1".into(),
        surface: "интра ру".into(),
        canonical: "intra.ru".into(),
        language: SpeechLanguage::Ru,
        scope: GlossaryScope::Meeting { meeting_id: "m1".into() },
        kind: GlossaryKind::Replacement,
    };
    let segments = vec![
        FinalSegment { index: 0, start_ms: 0, end_ms: 100, channel: AudioChannel::Mic,
            speaker_id: String::new(), speaker_pinned: false,
            text: "открой интра ру".into(), text_edited: false },
        FinalSegment { index: 1, start_ms: 100, end_ms: 200, channel: AudioChannel::Mic,
            speaker_id: String::new(), speaker_pinned: false,
            text: "тут ничего нет".into(), text_edited: false },
    ];
    let existing: Vec<SegmentEdit> = Vec::new();
    let mut ids = ["n1".to_string()].into_iter();

    let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

    assert_eq!(created.len(), 1, "правится только сегмент с вхождением");
    assert_eq!(created[0].edited_text, "открой intra.ru");
    assert_eq!(created[0].original_text, "открой интра ру");
}

#[test]
fn replacement_skips_already_edited_places() {
    use super::occurrences_to_edit;

    let term = GlossaryTerm {
        id: "t1".into(),
        surface: "интра ру".into(),
        canonical: "intra.ru".into(),
        language: SpeechLanguage::Ru,
        scope: GlossaryScope::Meeting { meeting_id: "m1".into() },
        kind: GlossaryKind::Replacement,
    };
    let segments = vec![FinalSegment {
        index: 0, start_ms: 0, end_ms: 100, channel: AudioChannel::Mic,
        speaker_id: String::new(), speaker_pinned: false,
        text: "открой интра ру".into(), text_edited: false,
    }];
    let existing = vec![SegmentEdit {
        id: "e0".into(), meeting_id: "m1".into(), channel: AudioChannel::Mic,
        start_ms: 0, end_ms: 100,
        original_text: "открой интра ру".into(),
        edited_text: "открой портал".into(),
        created_at_ms: 0, applied_version: Some(1),
    }];
    let mut ids = ["n1".to_string()].into_iter();

    let created = occurrences_to_edit(&term, "m1", 1, &segments, &existing, 7, &mut ids);

    assert!(created.is_empty(), "ручная правка человека сильнее массовой замены");
}
```

- [ ] **Step 2: Убедиться, что не компилируется**

Run: `cd rust && cargo test -p meetingraft-postcall occurrences_to_edit`
Expected: FAIL — `cannot find function occurrences_to_edit`

- [ ] **Step 3: Реализовать**

В `edit_service.rs`, после `plan_edit`:

```rust
/// Правки, которые нужно завести, чтобы термин применился ко всем
/// вхождениям во встрече.
///
/// Идём через журнал, а не переписыванием таблицы сегментов: распознанное
/// должно остаться распознанным, иначе сравнить версии будет не с чем.
///
/// Места, уже правленные вручную, не трогаются — точечное решение
/// человека сильнее массовой замены, ровно как у `speaker_pinned`.
pub fn occurrences_to_edit(
    term: &GlossaryTerm,
    meeting_id: &str,
    version: u32,
    segments: &[FinalSegment],
    existing: &[SegmentEdit],
    now_ms: u64,
    ids: &mut dyn Iterator<Item = String>,
) -> Vec<SegmentEdit> {
    segments
        .iter()
        .filter(|segment| segment.text.contains(term.surface.as_str()))
        .filter(|segment| {
            !existing.iter().any(|edit| {
                edit.channel == segment.channel
                    && edit.start_ms == segment.start_ms
                    && edit.end_ms == segment.end_ms
            })
        })
        .filter_map(|segment| {
            let id = ids.next()?;
            Some(SegmentEdit {
                id,
                meeting_id: meeting_id.to_owned(),
                channel: segment.channel,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                original_text: segment.text.clone(),
                edited_text: segment.text.replace(term.surface.as_str(), &term.canonical),
                created_at_ms: now_ms,
                applied_version: Some(version),
            })
        })
        .collect()
}
```

В `lib.rs` добавить `occurrences_to_edit` в реэкспорт `edit_service`.

- [ ] **Step 4: Прогон**

Run: `cd rust && cargo test -p meetingraft-postcall`
Expected: PASS

- [ ] **Step 5: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/postcall
git commit -m "feat: apply replacement to all occurrences via the journal"
```

---

### Task 9: Граница FFI

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs:109` (`FfiFinalSegment`), `:1308` (`list_final_segments`), рядом с `:1424` (`assign_segment_speaker`)

**Interfaces:**
- Consumes: всё выше
- Produces: `edit_segment_text`, `list_unapplied_edits`, `promote_term_to_replacement`, поле `FfiFinalSegment.text_edited`

- [ ] **Step 1: Добавить поле в FfiFinalSegment**

В структуру `FfiFinalSegment` (`ffi/src/lib.rs:109`) — `pub text_edited: bool`, и в конструктор внутри `list_final_segments` (`:1324`) — `text_edited: segment.text_edited`.

- [ ] **Step 2: Добавить DTO правки**

Рядом с `FfiFinalSegment`:

```rust
/// Правка, не легшая ни на одну версию после пересбора.
#[derive(uniffi::Record)]
pub struct FfiSegmentEdit {
    pub id: String,
    pub channel: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub original_text: String,
    pub edited_text: String,
}
```

- [ ] **Step 3: Реализовать методы**

Рядом с `assign_segment_speaker` (`ffi/src/lib.rs:1424`), тем же стилем «ошибка строкой»:

```rust
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

        let segments = store
            .list_final_segments(&meeting_id, version)
            .unwrap_or_default();
        let Some(segment) = segments.into_iter().find(|s| s.index == index) else {
            return format!("сегмент {index} не найден");
        };

        // Предыдущая правка ищется до разбора: list_final_segments уже
        // отдал правленый текст, а сравнивать введённое надо с
        // распознанным. Иначе повторный ввод того же текста читался бы
        // как возврат к исходному и правка бы удалилась.
        let existing = store.list_segment_edits(&meeting_id).unwrap_or_default();
        let previous = existing.into_iter().find(|edit| {
            edit.channel == segment.channel
                && edit.start_ms == segment.start_ms
                && edit.end_ms == segment.end_ms
        });

        let mut recognized = segment.clone();
        if let Some(previous) = &previous {
            recognized.text = previous.original_text.clone();
            recognized.text_edited = false;
        }

        let terms = store.list_glossary_terms().unwrap_or_default();
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

        if let Some(term) = outcome.term {
            if let Err(error) = store.upsert_glossary_term(&term, now_ms()) {
                return error.to_string();
            }
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

    /// Превратить подсказку в замену: применять всюду.
    ///
    /// Единственный способ получить замену из правки — явный жест
    /// человека. Автоматически рождаются только подсказки.
    pub fn promote_term_to_replacement(
        &self,
        term_id: String,
        meeting_id: String,
        version: u32,
    ) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        let terms = store.list_glossary_terms().unwrap_or_default();
        let Some(mut term) = terms.into_iter().find(|term| term.id == term_id) else {
            return format!("термин {term_id} не найден");
        };
        term.kind = GlossaryKind::Replacement;
        if let Err(error) = store.upsert_glossary_term(&term, now_ms()) {
            return error.to_string();
        }

        let segments = store
            .list_final_segments(&meeting_id, version)
            .unwrap_or_default();
        let existing = store.list_segment_edits(&meeting_id).unwrap_or_default();
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
```

Имена `self.session_language()` и `now_ms()` взять те же, что у соседних методов файла — если названия другие, подставить существующие.

- [ ] **Step 4: Сборка и тесты**

Run: `cd rust && cargo test -p meetingraft-ffi && cargo clippy --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 5: Коммит**

```bash
cd rust && cargo fmt --check
git add rust/crates/ffi
git commit -m "feat: FFI for segment edits and term kind"
```

---

## Чего в этом плане нет

- **Интерфейс на Swift** — проигрывание фрагмента, правка на месте, раздел неприменившихся правок, кнопка «заменять всюду». Отдельный план: на этой машине Swift не собирается, и проверять его придётся на Маке.
- **Сжатие словаря через LLM** — отдельный план: это самостоятельная подсистема с вызовом провайдера и экраном разбора плана.
- **Вызов `reattach_edits` из пересбора** — точку вызова определит план по Swift-части вместе с местом, где запускается пересбор; функция из Task 5 к этому моменту готова и покрыта тестами.
- **Предложение поднять замену до глобальной.** Подсказка при повторе в другой встрече поднимается сама (Task 7), а замена по спеке только предлагается — а предложение это элемент интерфейса, и живёт оно в Swift-плане.
- **Уборка неприменившихся правок.** Показать их — задача интерфейса; удалять их автоматически нельзя, иначе ручная работа исчезнет молча.
