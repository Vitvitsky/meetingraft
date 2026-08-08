# Audio Retention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Дать удалить аудио встречи, не тронув ничего другого, и сказать человеку, что оно удалено.

**Architecture:** Удаление живёт в `storage` рядом с `delete_meeting`, но сносит только каталог `sessions/<id>` и строки `audio_manifest`, проставляя `sessions.audio_deleted_at_ms`. Потребители аудио не трогаются: они уже деградируют правильно. `ffi` добавляет отказ при идущей пересборке и считает размер по файлам. Swift получает действие в карточке встречи и раздел пакетной чистки в настройках.

**Tech Stack:** Rust (`meetingraft-domain`, `meetingraft-storage`, `meetingraft-ffi`), SQLite через `rusqlite`, SwiftUI.

Спека: `docs/superpowers/specs/2026-08-08-audio-retention-design.md`.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; сообщения коммитов — **по-английски** (`CLAUDE.md`).
- Тесты гонять по крейтам: `cd rust && cargo test -p meetingraft-domain -p meetingraft-storage -p meetingraft-ffi`. Полный `cargo test` по workspace не влезает в память VPS.
- Имена пакетов с префиксом: крейт `storage` — это пакет `meetingraft-storage`.
- Перед коммитом: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`. **Каждый коммит зелёный.**
- Swift собирается только на Mac. Задача 6 уходит на проверку человеку, заявлять её проверенной нельзя.
- **Никакой автоматики.** Ни по расписанию, ни при запуске. Предпросмотр обязан быть чистой функцией: считает и показывает, не удаляет.
- Удаление аудио **не трогает** ни одну таблицу, кроме `audio_manifest` и колонки `sessions.audio_deleted_at_ms`.
- Каждый тест обязан падать без своей ветки. На этом Epic 19 обжёгся трижды.

---

## File Structure

| Файл | Ответственность |
|---|---|
| `rust/crates/storage/src/migrations.rs` | шаг 11 — `audio_deleted_at_ms` |
| `rust/crates/domain/src/postcall.rs` | поле в `MeetingSummary` |
| `rust/crates/storage/src/audio_manifest.rs` | удаление аудио, размер на диске, отдача метки в сводке |
| `rust/crates/ffi/src/lib.rs` | граница: удаление, размер, отказ при идущей пересборке, пакет |
| `apps/macos/Sources/Meetings/` | действие в карточке встречи |
| `apps/macos/Sources/Settings/` | раздел пакетной чистки |
| `docs/backlog.md` | закрыть пункт эпика |

**Ловушка Swift:** `MeetingsCoreProviding` (`MeetingsViewModel.swift:19`) — протокол, и его реализуют два тестовых дубля: `MeetingsCoreSpy` (`Tests/MeetingsViewModelTests.swift:447`) и `LibraryCoreSpy` (`Tests/MeetingsLibraryTests.swift:150`). Любой новый метод протокола ломает оба, и это обнаружится только на Маке. Добавлять — сразу с заглушками в обоих дублях.

---

### Task 1: Метка удаления в схеме и в сводке

Поведение не меняется: колонка появляется, всегда NULL, и доезжает до Swift.

**Files:**
- Modify: `rust/crates/storage/src/migrations.rs` (шаг 11 в `STEPS`), `rust/crates/domain/src/postcall.rs:47-56` (`MeetingSummary`), `rust/crates/storage/src/audio_manifest.rs:771-800` (`list_meeting_summaries`), `rust/crates/ffi/src/lib.rs:112-121` (`FfiMeetingSummary`), `:602-610` (`meeting_summary_to_ffi`)
- Test: `migrations.rs`, `audio_manifest.rs` — модули `mod tests`

**Interfaces:**
- Produces:
  - шаг миграции 11; `schema_version()` становится 11
  - `MeetingSummary.audio_deleted_at_ms: Option<u64>`
  - `FfiMeetingSummary.audio_deleted_at_ms: u64` — **0 означает «не удаляли»**, как `ended_at_ms` уже кодирует «не завершена»

- [ ] **Step 1: Падающий тест на миграцию поверх существующей базы**

В `mod tests` в `migrations.rs`, рядом с `a_database_already_at_step_nine_migrates_the_rest_of_the_way`:

```rust
/// База с уже созданной встречей поднимается так, что запись считается
/// неудалённой. Выдать неизвестное за «удалили» — соврать в другую
/// сторону, как и с `source_version` артефактов.
#[test]
fn migration_leaves_existing_meetings_marked_as_not_deleted() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch(baseline_schema()).expect("baseline");
    conn.execute(
        "INSERT INTO sessions (id, started_at_ms) VALUES ('m1', 1)",
        [],
    )
    .expect("legacy row");

    migrate(&conn).expect("migrate");

    let deleted: Option<i64> = conn
        .query_row(
            "SELECT audio_deleted_at_ms FROM sessions WHERE id = 'm1'",
            [],
            |row| row.get(0),
        )
        .expect("column");
    assert_eq!(deleted, None, "старая встреча помечена удалённой");
}
```

- [ ] **Step 2: Шаг миграции 11**

В конец `STEPS`:

```rust
    // 11 — когда у встречи удалили аудио (Epic 22). NULL — не удаляли:
    // отсутствие записи иначе неотличимо от её отсутствия по другой
    // причине, и человек, у которого не запустился микрофон, и человек,
    // сам удаливший запись, видели бы одно и то же.
    "
    ALTER TABLE sessions ADD COLUMN audio_deleted_at_ms INTEGER;
    ",
```

- [ ] **Step 3: Поле в домене и по всей цепочке**

`MeetingSummary` получает `pub audio_deleted_at_ms: Option<u64>`. Все конструкции структуры в тестах и в `postcall` придётся дополнить — их немного, компилятор покажет все.

`list_meeting_summaries`: `sessions.audio_deleted_at_ms` седьмой колонкой, `row.get::<_, Option<i64>>(6)?.map(|v| v as u64)`.

`FfiMeetingSummary` получает `pub audio_deleted_at_ms: u64` с доком «0 — аудио не удаляли», `meeting_summary_to_ffi` — `.unwrap_or(0)`.

- [ ] **Step 4: Тест на сводку**

```rust
/// Сводка доносит метку до Swift. Без этого карточка встречи не сможет
/// отличить «записи не было» от «запись удалили».
#[test]
fn a_summary_carries_the_audio_deletion_mark() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("m1", 1, "").unwrap();
        store.end_session(2).unwrap();

        let summary = store.list_meeting_summaries().unwrap().remove(0);
        assert_eq!(summary.audio_deleted_at_ms, None, "ничего не удаляли");
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 5: Проверка**

```
cd rust && cargo test -p meetingraft-domain -p meetingraft-storage -p meetingraft-ffi
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

**Мутация:** в `list_meeting_summaries` вернуть `Some(0)` вместо прочитанного значения → `a_summary_carries_the_audio_deletion_mark` обязан упасть. Приложить дословный вывод.

---

### Task 2: Удаление аудио в `storage`

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs` — новый метод рядом с `delete_meeting` (`:1095`)
- Test: тот же файл

**Interfaces:**
- Produces:
  - `AudioManifestStore::delete_meeting_audio(&mut self, meeting_id: &str) -> Result<(), AudioManifestError>`

- [ ] **Step 1: Главный падающий тест — что остаётся, а не что уходит**

Это тест, ради которого работа существует. Он должен ловить превращение задачи в `delete_meeting` под другим именем.

```rust
/// Уходит только запись. Всё, ради чего встреча хранится, остаётся.
#[test]
fn deleting_audio_keeps_everything_else() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("m1", 1, "Планёрка").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
        store
            .append_chunk(AudioChannel::Mic, &frame, 16_000, 0)
            .unwrap();
        store.end_session(2).unwrap();
        // Титры, Final, сегменты, артефакт, спикер — всё, что должно выжить.
        // (наполнение по образцу существующих тестов delete_meeting)

        store.delete_meeting_audio("m1").unwrap();

        assert!(store.list_chunks("m1").unwrap().is_empty(), "строки манифеста");
        assert!(
            !root.join("sessions").join("m1").exists(),
            "каталог с чанками"
        );
        let summary = store.list_meeting_summaries().unwrap().remove(0);
        assert!(summary.audio_deleted_at_ms.is_some(), "метка не проставлена");

        // И то, ради чего всё затевалось.
        assert!(!store.list_final_transcripts("m1").unwrap().is_empty(), "Final");
        assert!(!store.list_final_segments("m1", 1).unwrap().is_empty(), "сегменты");
        assert!(!store.list_captions("m1").unwrap().is_empty(), "титры");
        assert!(!store.list_artifacts("m1").unwrap().is_empty(), "артефакты");
        assert!(!store.list_speakers("m1").unwrap().is_empty(), "спикеры");
        assert!(!store.search("планёрка", 10).unwrap().is_empty(), "поиск");
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Реализация**

```rust
    /// Удалить запись встречи, оставив всё остальное.
    ///
    /// Транскрипт нужен всегда, запись полугодовой давности — почти
    /// никогда (Epic 22). Уходят только файлы и строки манифеста;
    /// титры, Final, сегменты, журнал правок, артефакты, спикеры и
    /// поисковый индекс остаются нетронутыми.
    ///
    /// Строки манифеста удаляются вместе с файлами намеренно: строка без
    /// файла заставила бы `read_session_pcm` упасть с ошибкой
    /// ввода-вывода, то есть выглядеть поломкой. Их отсутствие даёт
    /// деградацию, которая уже построена, — пустой фрагмент и внятный
    /// отказ пересборки.
    ///
    /// Повторный вызов не ошибка и **не сдвигает метку**: дата удаления
    /// — та, когда запись действительно удалили.
    pub fn delete_meeting_audio(&mut self, meeting_id: &str) -> Result<(), AudioManifestError> {
        if self.active_session.as_deref() == Some(meeting_id) {
            return Err(AudioManifestError::SessionActive(meeting_id.to_owned()));
        }
        // ... существование встречи → MeetingNotFound
        // ... транзакция: DELETE FROM audio_manifest;
        //     UPDATE sessions SET audio_deleted_at_ms = ?2
        //       WHERE id = ?1 AND audio_deleted_at_ms IS NULL
        // ... после коммита: fs::remove_dir_all, если каталог есть
    }
```

Порядок «сначала транзакция, потом файлы» — как в `delete_meeting` (`:1124`): откат не должен оставить строки без чанков.

Время берётся аргументом, а не из системных часов: тесту нужно проверять, что повторный вызов метку не двигает. Сигнатура — `delete_meeting_audio(&mut self, meeting_id: &str, now_ms: u64)`.

- [ ] **Step 3: Тесты на отказы и повтор**

```rust
/// Удалять запись идущей встречи нельзя: потеряем звук прямо во время неё.
#[test]
fn deleting_audio_of_the_active_session_is_refused() { /* ... */ }

/// Дата удаления — первая. Повтор ничего не ломает и метку не двигает.
#[test]
fn deleting_audio_twice_keeps_the_first_date() { /* ... */ }

/// Каталог снесли снаружи — не наша беда: строки убираем, метку ставим.
#[test]
fn deleting_audio_without_files_on_disk_still_marks_the_meeting() { /* ... */ }
```

- [ ] **Step 4: Тест на деградацию потребителей**

```rust
/// После удаления прослушивание отдаёт пустоту, а не ошибку: вью на
/// пустой фрагмент прячет кнопку, а на ошибке показать нечего.
#[test]
fn reading_a_range_after_deletion_returns_empty_not_an_error() { /* ... */ }
```

- [ ] **Step 5: Проверка**

**Мутации, обе обязательны:**
1. Добавить в удаление `DELETE FROM final_transcripts WHERE meeting_id = ?1` → `deleting_audio_keeps_everything_else` обязан упасть на строке про Final. Это проверка, что тест ловит превращение в `delete_meeting`.
2. Снять `AND audio_deleted_at_ms IS NULL` из UPDATE → `deleting_audio_twice_keeps_the_first_date` обязан упасть.

---

### Task 3: Размер аудио на диске

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs`
- Test: тот же файл

**Interfaces:**
- Produces:
  - `AudioManifestStore::meeting_audio_bytes(&self, meeting_id: &str) -> Result<u64, AudioManifestError>`

- [ ] **Step 1: Падающий тест**

```rust
/// Размер — то, что освободится, то есть сумма файлов на диске.
/// `frame_count * 2` после FLAC завышает больше чем вдвое, а завышенное
/// вдвое число — не оценка, а обещание, которое не выполнится.
#[test]
fn audio_bytes_counts_files_not_raw_frames() {
    // Записать пачку, посчитать сумму метаданных файлов, сверить с
    // meeting_audio_bytes, и **отдельно** убедиться, что она заметно
    // меньше frame_count * 2 — иначе тест прошёл бы и на сыром PCM.
}
```

Второе утверждение здесь несущее: без него тест зелёный при любой реализации.

- [ ] **Step 2: Реализация**

Обход `sessions/<id>` рекурсивно, сумма `metadata.len()`. Отсутствующий каталог — `Ok(0)`, не ошибка: удалённая или ещё не начатая запись занимает ноль.

- [ ] **Step 3: Проверка + мутация**

**Мутация:** вернуть `sum(frame_count) * 2` из манифеста вместо обхода файлов → тест обязан упасть на сравнении с реальным размером.

---

### Task 4: Граница UniFFI

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs` — рядом с `delete_meeting` (`:1397`)
- Test: тот же файл

**Interfaces:**
- Produces:
  - `MeetingCore::delete_meeting_audio(&self, meeting_id: String) -> String` — пустая строка успех, по конвенции границы
  - `MeetingCore::meeting_audio_bytes(&self, meeting_id: String) -> u64`

- [ ] **Step 1: Завести сеам — его нет**

Найдено при сверке плана с кодом: **удерживать пересборку живой в тесте нечем.** `MeetingCore::with_data_root` (`:879`) жёстко берёт `ThreadSpawner` (`:907`), а существующий `starting_rebuild_twice_returns_the_same_job` (`:3530`) прямо признаёт гонку — «проход без аудио падает сразу, поэтому первый может успеть завершиться» — и подстраивается под неё условием. Тест отказа, написанный так же, будет то проходить, то нет, в зависимости от того, кто успел.

`postcall::jobs` (`:66-86`) даёт `ThreadSpawner` и `InlineSpawner`. Второй выполняет работу на месте, то есть джоб завершается мгновенно — тоже не то. Нужен третий:

```rust
/// Не выполняет работу вовсе: задача остаётся заведённой навсегда.
///
/// Нужен там, где проверяется поведение **при идущем** проходе:
/// `ThreadSpawner` даёт гонку, `InlineSpawner` заканчивает работу до
/// первой проверки.
pub struct NeverSpawner;

impl Spawner for NeverSpawner {
    fn spawn(&self, _work: Box<dyn FnOnce() + Send + 'static>) {}
}
```

И конструктор ядра, принимающий спавнер. Публичным его делать нечего — только для тестов крейта:

```rust
#[cfg(test)]
fn with_data_root_and_spawner(
    data_root: String,
    spawner: Box<dyn Spawner>,
) -> std::sync::Arc<Self>
```

`with_data_root` становится обёрткой над ним с `ThreadSpawner`, чтобы не разъезжались два списка полей `MeetingCoreInner`.

- [ ] **Step 2: Падающий тест на отказ при идущей пересборке**

```rust
/// Пересборка держит дорожки и читает манифест по ходу. Выдернуть файлы
/// из-под неё — получить непонятный отказ в середине долгого прохода.
#[test]
fn deleting_audio_during_a_rebuild_is_refused() {
    // ядро с NeverSpawner → start_final_rebuild оставляет задачу висеть
    // → delete_meeting_audio обязан вернуть непустую строку
    // → и, главное, файлы обязаны остаться на месте
}
```

Утверждение про файлы здесь несущее: проверять только текст ошибки значило бы поверить, что отказ произошёл до удаления, а не после.

- [ ] **Step 3: Реализация**

По образцу `delete_meeting` (`:1397`): проверка `recording_session_id` под гвардом, затем `drop(guard)`, затем работа со `store`. Добавляется проверка `active_final_rebuild`.

Отказы возвращаются строкой: «meeting is being recorded», «final rebuild in progress».

- [ ] **Step 4: Тесты на успешный путь и на размер**

- [ ] **Step 5: Проверка + мутация**

**Мутация:** снять проверку `active_final_rebuild` → тест из шага 2 обязан упасть **на существовании файлов**, а не только на пустой строке ошибки.

---

### Task 5: Пакетная чистка

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Test: тот же файл

**Interfaces:**
- Produces:
  - `FfiAudioSweepEntry { meeting_id, title, started_at_ms, bytes }`
  - `MeetingCore::preview_audio_sweep(&self, older_than_ms: u64) -> Vec<FfiAudioSweepEntry>` — **чистая функция**
  - `FfiAudioSweepResult { deleted_count, freed_bytes, skipped: Vec<String> }`
  - `MeetingCore::run_audio_sweep(&self, older_than_ms: u64) -> FfiAudioSweepResult`

- [ ] **Step 1: Главный падающий тест — предпросмотр без побочных эффектов**

```rust
/// Человек, решивший посмотреть, сколько освободится, не должен этим
/// ничего удалить. Самый опасный дефект этой работы.
#[test]
fn previewing_the_sweep_deletes_nothing() {
    // preview_audio_sweep дважды подряд возвращает одно и то же,
    // файлы на месте, строки манифеста на месте, метки нет.
}
```

- [ ] **Step 2: Порог по времени**

```rust
/// Пачка берёт только то, что старше порога. Свежую встречу не трогает.
#[test]
fn the_sweep_takes_only_meetings_older_than_the_threshold() { /* ... */ }
```

Порог — абсолютное время (`started_at_ms < older_than_ms`), а не «N месяцев»: перевод месяцев в миллисекунды делает Swift, где есть календарь. Ядру календарь не нужен и заводить его туда незачем.

- [ ] **Step 3: Реализация**

Предпросмотр: `list_meeting_summaries` → фильтр по порогу и по `audio_deleted_at_ms.is_none()` → `meeting_audio_bytes` на каждую. Уже удалённые в предпросмотр не попадают: показывать «освободится 0 Б» бессмысленно.

Чистка: тот же список, `delete_meeting_audio` на каждую; занятые (запись, пересборка) **пропускаются и попадают в `skipped`**. Тихо пропустить — соврать в отчёте о числе удалённых.

- [ ] **Step 4: Тест на пропуск занятой**

- [ ] **Step 5: Проверка + мутация**

**Мутация:** заставить `preview_audio_sweep` звать `delete_meeting_audio` → тест из шага 1 обязан упасть.

---

### Task 6: Swift

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift` (протокол + модель), `apps/macos/Sources/Meetings/MeetingDetailView.swift` (действие и состояние «запись удалена»), `apps/macos/Sources/Settings/SettingsSections.swift` (раздел чистки), `apps/macos/Tests/MeetingsViewModelTests.swift:447`, `apps/macos/Tests/MeetingsLibraryTests.swift:150` (дубли)

- [ ] **Step 1: Протокол и оба дубля разом**

Новые методы в `MeetingsCoreProviding`: `deleteMeetingAudio`, `meetingAudioBytes`, `previewAudioSweep`, `runAudioSweep`. **Сразу же** заглушки в `MeetingsCoreSpy` и `LibraryCoreSpy` — иначе тестовая цель не соберётся, и узнается это только на Маке.

- [ ] **Step 2: Карточка встречи**

Действие «Удалить запись» с размером рядом. Подтверждение через `.alert` — образец рядом, `MeetingDetailView.swift:78` и `:93`. Текст обязан говорить, что аудио не восстанавливается и что транскрипт остаётся.

Когда `audioDeletedAtMs != 0` — вместо действия строка «Запись удалена <дата>». Это то, ради чего колонка заводилась: отсутствие кнопки прослушивания перестаёт быть загадкой.

- [ ] **Step 3: Раздел настроек**

Рядом с `AudioSettingsSection` (`SettingsSections.swift:130`). Выбор порога, кнопка предпросмотра, список «что уйдёт» с общим размером, отдельная кнопка удаления. Отчёт после: сколько удалено, сколько освободилось, что пропущено и почему.

Перевод «старше N месяцев» в абсолютную метку времени — здесь, через `Calendar`.

- [ ] **Step 4: Проверка**

`apps/macos/Scripts/generate-ffi.sh`, затем сборка. **На VPS не проверяется** — уходит человеку с `scripts/verify-mac.sh`. Заявлять проверенным нельзя.

---

### Task 7: Бэклог

- [ ] Закрыть пункт «Срок хранения аудио отдельно от встречи» в Epic 22, записав: что удаляется и что остаётся, почему завели колонку-метку, почему размер по файлам, и что автоматики нет с обоснованием. Соседние пункты не трогать.

- [ ] Если все три пункта эпика закрыты — сказать это явно в шапке Epic 22.

---

## Self-Review

**Главный риск — что работа тихо станет `delete_meeting` под другим именем.** Тест `deleting_audio_keeps_everything_else` перечисляет то, что обязано выжить, поимённо, и мутация с добавленным `DELETE FROM final_transcripts` проверяет, что он это ловит. Без такой мутации тест выглядел бы убедительно и не значил бы ничего.

**Второй — предпросмотр с побочным эффектом.** Показ, который удаляет, — худшее, что здесь можно построить: человек нажимает «посмотреть» и теряет записи. Отдельный тест, отдельная мутация.

**Размер.** Считать по `frame_count * 2` соблазнительно (одна SQL-строка вместо обхода каталога) и неверно после FLAC. Тест обязан утверждать не только равенство сумме файлов, но и то, что число заметно меньше сырого, — иначе он пройдёт на любой реализации.

**Метка не двигается при повторе.** `UPDATE ... WHERE audio_deleted_at_ms IS NULL`, а не просто `UPDATE`. Иначе повторный вызов перепишет дату, и «удалено полгода назад» превратится в «удалено сегодня».

**Ошибка автора, найденная при сверке с кодом.** План сперва утверждал, что сеам для «идёт пересборка» уже есть. Его нет: спавнер в ядре зашит, а существующий тест на повторный запуск подстраивается под гонку вместо того, чтобы её устранять. Отсюда `NeverSpawner` и тестовый конструктор в задаче 4 — без них тест отказа был бы то зелёным, то красным, и его бы «починили» ослаблением.

**Ловушка Swift.** Два тестовых дубля протокола. Забыть их — значит сломать сборку тестов, и узнать об этом только на Маке, через полный прогон.

**Порог считает Swift, не ядро.** Календарь в ядре не нужен; граница принимает абсолютное время. Соблазн передать «6 месяцев» числом приведёт к календарю в Rust и расхождению с тем, что человек видел в предпросмотре.
