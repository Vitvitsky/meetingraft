# Audio Chunk Size Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Писать аудио на диск чанками по 2 с вместо 100 мс, не меняя ни задержку живых субтитров, ни читателей записи.

**Architecture:** `AudioManifestStore` получает накопитель на канал. `append_chunk` копит байты и пишет файл только когда набралось 2 с, при разрыве тайм-кодов, при смене частоты или при `end_session`. Живой путь не затрагивается: `ingest_audio_chunk` как и сейчас отдаёт свои 100 мс в `ChannelMixer` сразу.

**Tech Stack:** Rust (крейт `meetingraft-storage`, `meetingraft-ffi`), SQLite через `rusqlite`, Swift (две строки в `AudioCaptureCoordinator`).

Спека: `docs/superpowers/specs/2026-08-06-audio-chunk-size-design.md`.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; сообщения коммитов — **по-английски** (`CLAUDE.md`).
- Тесты гонять по крейтам, полный `cargo test` по workspace не влезает в память VPS: `cd rust && cargo test -p meetingraft-storage -p meetingraft-ffi`.
- Имя пакета с префиксом: крейт `storage` — это пакет `meetingraft-storage`. `-p storage` молча не найдёт ничего.
- Перед коммитом: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- **Миграции схемы нет.** Ни одна колонка не добавляется, старые записи читаются как есть.
- Целевой размер чанка на диске: **2 000 мс**. Живой кадр остаётся **100 мс** — `AudioChunkPipeline.chunkDurationMs` не трогать.
- Swift собирается только на Mac. Задача 5 уходит на проверку человеку, заявлять её проверенной нельзя.
- Каждый тест обязан падать без своей ветки. На этом Epic 19 обжёгся трижды: тест бывает зелёным, проверяя не то.

---

## File Structure

| Файл | Ответственность |
|---|---|
| `rust/crates/storage/src/audio_manifest.rs` | накопитель, правила сброса, запись файла и строки манифеста. Вся суть работы здесь |
| `rust/crates/ffi/src/lib.rs` | `stop_recording` начинает возвращать `String`; правка тестов, зовущих `append_chunk` |
| `apps/macos/Sources/Audio/AudioCaptureCoordinator.swift` | непустой результат `stopRecording` → `lastError` |
| `docs/backlog.md` | закрыть пункт про укрупнение |

`audio_manifest.rs` — 2360 строк, и это существующий уклад крейта; дробить его в рамках этой работы не нужно, добавляется около 90 строк в область, где уже живёт `append_chunk`.

---

### Task 1: Накопитель и сброс по целевому размеру

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs:83-88` (поле `pending` в структуре), `:116-133` (`begin_session`), `:136-144` (`end_session`), `:163-218` (`append_chunk`)
- Test: тот же файл, модуль `mod tests` внизу

**Interfaces:**
- Produces:
  - `const CHUNK_TARGET_MS: u64 = 2_000;`
  - `struct PendingChunk { pcm: Vec<u8>, sample_rate: u32, timestamp_ms: u64 }` — приватная
  - `fn channel_index(channel: AudioChannel) -> usize` — приватная свободная функция
  - `AudioManifestStore::append_chunk(&mut self, channel: AudioChannel, pcm: &[u8], sample_rate: u32, timestamp_ms: u64) -> Result<Option<ManifestChunk>, AudioManifestError>` — **тип возврата меняется**: `Some` только когда этим вызовом записан файл
  - `AudioManifestStore::flush_pending_chunks(&mut self) -> Result<Vec<ManifestChunk>, AudioManifestError>` — публичный
  - `AudioManifestStore::write_pending(&mut self, channel: AudioChannel) -> Result<Option<ManifestChunk>, AudioManifestError>` — приватный

- [ ] **Step 1: Написать падающий тест на укрупнение**

В `mod tests` в `rust/crates/storage/src/audio_manifest.rs`:

```rust
/// Двадцать кадров живого пути по 100 мс ложатся на диск одним файлом.
///
/// Смысл всей работы: 100 мс — размер кадра STT, у хранения причин быть
/// таким же нет.
#[test]
fn twenty_live_frames_become_one_file() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        // 1600 кадров = ровно 100 мс на 16 кГц.
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
        for index in 0..20 {
            store
                .append_chunk(AudioChannel::Mic, &frame, 16_000, index * 100)
                .unwrap();
        }

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 1, "20 кадров по 100 мс = один файл на 2 с");
        assert_eq!(chunks[0].frame_count, 32_000, "2 с на 16 кГц");
        assert_eq!(chunks[0].timestamp_ms, 0, "время первого кадра пачки");
        assert_eq!(store.chunk_count("s1").unwrap(), 1);
    }
    let _ = fs::remove_dir_all(&root);
}

/// Остаток меньше целевого размера обязан лечь на диск при закрытии
/// сессии: последние секунды записи нужны так же, как все прежние.
#[test]
fn end_session_writes_the_remainder() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
        // Пять кадров — 500 мс, до целевых 2 с не добирает.
        for index in 0..5 {
            store
                .append_chunk(AudioChannel::Mic, &frame, 16_000, index * 100)
                .unwrap();
        }
        assert!(
            store.list_chunks("s1").unwrap().is_empty(),
            "до закрытия остаток ещё в накопителе"
        );

        store.end_session(2).unwrap();

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 1, "остаток дописан");
        assert_eq!(chunks[0].frame_count, 8_000, "500 мс на 16 кГц");
    }
    let _ = fs::remove_dir_all(&root);
}

/// Хвост прошлой сессии не должен попасть в файлы новой.
#[test]
fn begin_session_drops_the_previous_remainder() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
        store.append_chunk(AudioChannel::Mic, &frame, 16_000, 0).unwrap();

        store.begin_session("s2", 2, "").unwrap();
        store.flush_pending_chunks().unwrap();

        assert!(
            store.list_chunks("s2").unwrap().is_empty(),
            "накопитель s1 не должен всплыть в s2"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Запустить и убедиться, что тесты падают**

Run: `cd rust && cargo test -p meetingraft-storage twenty_live_frames_become_one_file end_session_writes_the_remainder begin_session_drops_the_previous_remainder`

Expected: FAIL. `twenty_live_frames_become_one_file` — `assert_eq!(chunks.len(), 1)` получает 20; `flush_pending_chunks` вообще не существует, то есть третий тест не компилируется.

- [ ] **Step 3: Добавить константу, накопитель и индекс канала**

Рядом с `MAX_FRAGMENT_MS` (`:46`):

```rust
/// Целевая длительность чанка на диске.
///
/// 100 мс — размер кадра живого пути (`AudioChunkPipeline.chunkDurationMs`),
/// и на диске это 72 000 файлов в час на двух каналах. У хранения причин
/// быть такой же гранулярности нет.
const CHUNK_TARGET_MS: u64 = 2_000;
```

После `struct ManifestChunk` (`:72-81`):

```rust
/// Накопитель записи по каналу.
///
/// Живой путь получает свои 100 мс сразу, на диск они уходят пачкой.
struct PendingChunk {
    pcm: Vec<u8>,
    sample_rate: u32,
    /// Время первого кадра пачки.
    timestamp_ms: u64,
}

impl PendingChunk {
    fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.pcm.len() as u64 / 2) * 1000 / u64::from(self.sample_rate)
    }

    /// Время, которым обязан начинаться следующий кадр, чтобы пачка
    /// осталась непрерывной.
    fn next_timestamp_ms(&self) -> u64 {
        self.timestamp_ms + self.duration_ms()
    }
}

fn channel_index(channel: AudioChannel) -> usize {
    match channel {
        AudioChannel::Mic => 0,
        AudioChannel::System => 1,
    }
}
```

- [ ] **Step 4: Добавить поле в структуру**

В `struct AudioManifestStore` (`:83-88`) добавить последним полем:

```rust
    pending: [Option<PendingChunk>; 2],
```

В `open` (`:107-112`) в конструктор добавить:

```rust
            pending: [None, None],
```

- [ ] **Step 5: Переписать `append_chunk` на накопление**

Заменить тело `append_chunk` (`:163-218`) целиком:

```rust
    /// Принять кадр живого пути в накопитель канала.
    ///
    /// Файл пишется не на каждый вызов: `Some` возвращается, когда этим
    /// вызовом пачка ушла на диск по достижении целевого размера. Кому
    /// нужен записанный чанк наверняка — `flush_pending_chunks`.
    pub fn append_chunk(
        &mut self,
        channel: AudioChannel,
        pcm: &[u8],
        sample_rate: u32,
        timestamp_ms: u64,
    ) -> Result<Option<ManifestChunk>, AudioManifestError> {
        if self.active_session.is_none() {
            return Err(AudioManifestError::SessionNotOpen);
        }
        let idx = channel_index(channel);

        let entry = self.pending[idx].get_or_insert_with(|| PendingChunk {
            pcm: Vec::new(),
            sample_rate,
            timestamp_ms,
        });
        entry.pcm.extend_from_slice(pcm);
        // Через локальную переменную, а не прямо в `if`: иначе изменяемое
        // одолжение `entry` дожило бы до вызова `write_pending`.
        let is_full = entry.duration_ms() >= CHUNK_TARGET_MS;

        if is_full {
            return self.write_pending(channel);
        }
        Ok(None)
    }

    /// Записать накопленное по каналу файлом и строкой манифеста.
    ///
    /// Пустой накопитель — не ошибка и не пустой файл: `Ok(None)`.
    fn write_pending(
        &mut self,
        channel: AudioChannel,
    ) -> Result<Option<ManifestChunk>, AudioManifestError> {
        let idx = channel_index(channel);
        let Some(pending) = self.pending[idx].take() else {
            return Ok(None);
        };
        if pending.pcm.is_empty() {
            return Ok(None);
        }
        let session_id = self
            .active_session
            .clone()
            .ok_or(AudioManifestError::SessionNotOpen)?;
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
        fs::write(&abs, &pending.pcm)?;

        let frame_count = (pending.pcm.len() / 2) as u32;
        self.conn.execute(
            "INSERT INTO audio_manifest
             (session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session_id,
                channel.dir_name(),
                seq,
                rel,
                pending.sample_rate,
                frame_count,
                pending.timestamp_ms as i64
            ],
        )?;

        Ok(Some(ManifestChunk {
            session_id,
            channel,
            seq,
            path: abs,
            sample_rate: pending.sample_rate,
            frame_count,
            timestamp_ms: pending.timestamp_ms,
        }))
    }

    /// Дописать остатки обоих каналов.
    ///
    /// Публичный: `end_session` зовёт его сам, но тестам и будущему
    /// коду нужна явная точка сброса.
    pub fn flush_pending_chunks(
        &mut self,
    ) -> Result<Vec<ManifestChunk>, AudioManifestError> {
        let mut written = Vec::new();
        for channel in [AudioChannel::Mic, AudioChannel::System] {
            if let Some(chunk) = self.write_pending(channel)? {
                written.push(chunk);
            }
        }
        Ok(written)
    }
```

- [ ] **Step 6: Сбрасывать в `end_session`, чистить в `begin_session`**

В `begin_session` (`:130-131`) после `self.next_seq = [0, 0];` добавить:

```rust
        // Остаток прошлой сессии не должен попасть в файлы новой.
        // Штатно он уже сброшен в `end_session`; это страховка.
        self.pending = [None, None];
```

`end_session` (`:136-144`) заменить целиком:

```rust
    pub fn end_session(&mut self, ended_at_ms: u64) -> Result<(), AudioManifestError> {
        // Остаток на диск до закрытия: последние секунды записи нужны так
        // же, как все прежние. Сброс до `take()` — ему нужна активная
        // сессия, чтобы знать путь.
        self.flush_pending_chunks()?;
        if let Some(session_id) = self.active_session.take() {
            self.conn.execute(
                "UPDATE sessions SET ended_at_ms = ?2 WHERE id = ?1",
                params![session_id, ended_at_ms as i64],
            )?;
        }
        Ok(())
    }
```

- [ ] **Step 7: Починить существующие тесты `storage`, которым нужна точка сброса**

Четыре места. Первое — `append_two_channels_lists_and_counts` (`:1160`): после двух `append_chunk` добавить перед проверками:

```rust
            store.flush_pending_chunks().unwrap();
```

Второе — `seed_second` (`:1594-1603`), помощник тестов `read_pcm_range`. Добавить сброс в конец, после цикла:

```rust
        // Пачка укрупняется, а нарезка по времени обязана остаться той же.
        store.flush_pending_chunks().unwrap();
```

Третье и четвёртое — `read_session_pcm_fails_on_missing_chunk_file` (`:1746`) и `read_session_pcm_fails_on_truncated_chunk` (`:1765`): они берут `chunk.path` из возврата `append_chunk`, а он теперь `None`. В обоих заменить

```rust
            let chunk = store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
```

на

```rust
            store
                .append_chunk(AudioChannel::Mic, &[1, 0, 2, 0], 16_000, 0)
                .unwrap();
            let chunk = store.flush_pending_chunks().unwrap().remove(0);
```

`read_session_pcm_concatenates_channel_in_order` (`:1702`) тоже требует сброса — добавить `store.flush_pending_chunks().unwrap();` после трёх `append_chunk`, перед проверками. `append_without_session_fails` (`:1219`) менять не нужно: проверка сессии стоит до накопления.

- [ ] **Step 8: Запустить тесты крейта целиком**

Run: `cd rust && cargo test -p meetingraft-storage`

Expected: PASS, все тесты. Если падает `pcm_range_cuts_inside_chunks` — это сигнал, что нарезка по времени поехала, а не что тест устарел; разбираться, а не подгонять утверждения.

- [ ] **Step 9: Проверить, что тест на укрупнение ловит именно укрупнение**

Временно вернуть в `append_chunk` немедленную запись: сразу после `entry.pcm.extend_from_slice(pcm);` вставить `return self.write_pending(channel);`.

Run: `cd rust && cargo test -p meetingraft-storage twenty_live_frames_become_one_file`

Expected: FAIL — `chunks.len()` равен 20. Убрать вставленную строку и убедиться, что тест снова зелёный.

- [ ] **Step 10: Линты и коммит**

```bash
cd rust && cargo fmt && cargo clippy --all-targets -- -D warnings
git add rust/crates/storage/src/audio_manifest.rs
git commit -m "feat: write audio to disk in 2s chunks instead of 100ms

100 ms is the live path's frame size, not a storage requirement: on disk
it costs 72 000 files an hour across both channels."
```

---

### Task 2: Сброс по разрыву тайм-кодов

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs` — `append_chunk`
- Test: тот же файл, `mod tests`

**Interfaces:**
- Consumes: `PendingChunk::next_timestamp_ms`, `write_pending`, `channel_index`, `CHUNK_TARGET_MS` из задачи 1.
- Produces: ничего нового в API.

Зачем: строка манифеста утверждает, что с `timestamp_ms` идёт непрерывный кусок в `frame_count` кадров, и `read_pcm_range` адресует чанк именно так (`:753-759`). Склейка через пропуск объявила бы, что в дыре есть звук, и сдвинула бы всё после неё — то есть сломала бы прослушивание правленого сегмента (Epic 19), ради которого запись и держится.

Точности хватает без допуска, и это важно не сломать: `AudioChunkPipeline` режет ровно по `framesPerChunk = 1600`, `startFrame` растёт на 1600, а `1600 * 1000 / 16000` — целое. Тайм-коды внутри канала непрерывны точно. Допуск здесь был бы не страховкой, а способом склеить настоящий разрыв.

- [ ] **Step 1: Написать падающий тест**

```rust
/// Разрыв тайм-кодов рвёт пачку: строка манифеста не должна заявлять
/// непрерывность там, где её нет.
///
/// Без этой ветки два кадра склеятся в один чанк с `timestamp_ms = 0`,
/// и всё, что после дыры, поедет по времени.
#[test]
fn a_timestamp_gap_starts_a_new_chunk() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();

        // 0..100 мс, дальше дыра, и кадр с 500 мс.
        store.append_chunk(AudioChannel::Mic, &frame, 16_000, 0).unwrap();
        store.append_chunk(AudioChannel::Mic, &frame, 16_000, 500).unwrap();
        store.flush_pending_chunks().unwrap();

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 2, "через дыру склеивать нельзя");
        assert_eq!(chunks[0].timestamp_ms, 0);
        assert_eq!(chunks[0].frame_count, 1_600);
        assert_eq!(chunks[1].timestamp_ms, 500, "второй чанк со своего времени");
        assert_eq!(chunks[1].frame_count, 1_600);
    }
    let _ = fs::remove_dir_all(&root);
}

/// Непрерывные кадры по-прежнему склеиваются: ветка разрыва не должна
/// срабатывать на нормальном потоке, иначе укрупнения не будет вовсе.
#[test]
fn contiguous_frames_still_merge() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
        for index in 0..3 {
            store
                .append_chunk(AudioChannel::Mic, &frame, 16_000, index * 100)
                .unwrap();
        }
        store.flush_pending_chunks().unwrap();

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 1, "непрерывное склеивается");
        assert_eq!(chunks[0].frame_count, 4_800, "300 мс на 16 кГц");
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Запустить и убедиться, что первый тест падает**

Run: `cd rust && cargo test -p meetingraft-storage a_timestamp_gap_starts_a_new_chunk contiguous_frames_still_merge`

Expected: `a_timestamp_gap_starts_a_new_chunk` FAIL — `chunks.len()` равен 1. `contiguous_frames_still_merge` PASS уже сейчас.

- [ ] **Step 3: Добавить ветку разрыва**

В `append_chunk`, между проверкой сессии и `get_or_insert_with`, вставить:

```rust
        // Разрыв тайм-кодов: накопленное пишем как есть. Иначе строка
        // манифеста заявит непрерывный кусок, которого нет, и всё после
        // дыры поедет по времени.
        let gap = matches!(
            self.pending[idx].as_ref(),
            Some(p) if p.next_timestamp_ms() != timestamp_ms
        );
        if gap {
            // Записанный чанк здесь намеренно не возвращается: сброс
            // служебный, а `Some` из `append_chunk` означает «набрался
            // целевой размер». Кому нужен чанк наверняка — берёт его у
            // `flush_pending_chunks`.
            self.write_pending(channel)?;
        }
```

- [ ] **Step 4: Запустить оба теста**

Run: `cd rust && cargo test -p meetingraft-storage a_timestamp_gap_starts_a_new_chunk contiguous_frames_still_merge`

Expected: оба PASS.

- [ ] **Step 5: Убедиться, что тест падает без ветки**

Закомментировать `if gap { self.write_pending(channel)?; }`.

Run: `cd rust && cargo test -p meetingraft-storage a_timestamp_gap_starts_a_new_chunk`

Expected: FAIL. Вернуть ветку.

- [ ] **Step 6: Прогнать крейт целиком и закоммитить**

```bash
cd rust && cargo test -p meetingraft-storage && cargo fmt && cargo clippy --all-targets -- -D warnings
git add rust/crates/storage/src/audio_manifest.rs
git commit -m "fix: never merge audio chunks across a timestamp gap

A manifest row claims a contiguous span from timestamp_ms; merging across
a hole would shift every segment after it, which is what final transcript
timecodes map through."
```

---

### Task 3: Сброс по смене частоты

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs` — `append_chunk`
- Test: тот же файл, `mod tests`

**Interfaces:**
- Consumes: `PendingChunk`, `write_pending`, `channel_index` из задачи 1; ветка разрыва из задачи 2.
- Produces: ничего нового в API.

Зачем: `read_pcm_range` уже пропускает чанки с частотой, отличной от первой (`:787-792`) — пересэмплировать там значило бы тихо менять звук, который человек пришёл сверить с текстом. Смешать две частоты внутри одного файла значит испортить его целиком, и никакой отбор на чтении этого не поймает.

- [ ] **Step 1: Написать падающий тест**

```rust
/// Смена частоты рвёт пачку: в одном файле две частоты — испорченный
/// звук, и отбор на чтении такого уже не поймает.
#[test]
fn a_sample_rate_change_starts_a_new_chunk() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();

        store.append_chunk(AudioChannel::Mic, &frame, 16_000, 0).unwrap();
        // Продолжение по времени, но другая частота.
        store.append_chunk(AudioChannel::Mic, &frame, 48_000, 100).unwrap();
        store.flush_pending_chunks().unwrap();

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 2, "две частоты в одном файле недопустимы");
        assert_eq!(chunks[0].sample_rate, 16_000);
        assert_eq!(chunks[1].sample_rate, 48_000);
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Запустить и убедиться, что тест падает**

Run: `cd rust && cargo test -p meetingraft-storage a_sample_rate_change_starts_a_new_chunk`

Expected: FAIL — `chunks.len()` равен 1, и `sample_rate` записан от первого кадра, то есть файл содержит две частоты под одной подписью.

- [ ] **Step 3: Дополнить условие сброса**

Заменить условие `gap` из задачи 2 на:

```rust
        // Разрыв тайм-кодов или смена частоты: накопленное пишем как есть.
        // Иначе строка манифеста заявит непрерывный кусок, которого нет,
        // а две частоты под одной подписью испортят и сам звук.
        let must_close = matches!(
            self.pending[idx].as_ref(),
            Some(p) if p.next_timestamp_ms() != timestamp_ms || p.sample_rate != sample_rate
        );
        if must_close {
            self.write_pending(channel)?;
        }
```

- [ ] **Step 4: Запустить тест**

Run: `cd rust && cargo test -p meetingraft-storage a_sample_rate_change_starts_a_new_chunk`

Expected: PASS.

- [ ] **Step 5: Убедиться, что тест падает без своей половины условия**

Временно убрать из условия `|| p.sample_rate != sample_rate`.

Run: `cd rust && cargo test -p meetingraft-storage a_sample_rate_change_starts_a_new_chunk a_timestamp_gap_starts_a_new_chunk`

Expected: `a_sample_rate_change_starts_a_new_chunk` FAIL, `a_timestamp_gap_starts_a_new_chunk` PASS — то есть каждая половина условия держится своим тестом. Вернуть условие.

- [ ] **Step 6: Прогнать крейт и закоммитить**

```bash
cd rust && cargo test -p meetingraft-storage && cargo fmt && cargo clippy --all-targets -- -D warnings
git add rust/crates/storage/src/audio_manifest.rs
git commit -m "fix: never merge audio chunks across a sample rate change

read_pcm_range already skips chunks whose rate differs from the first;
two rates inside one file would be corrupt audio no reader could sort out."
```

---

### Task 4: Доказать, что читатели не задеты

**Files:**
- Test: `rust/crates/storage/src/audio_manifest.rs`, `mod tests`

**Interfaces:**
- Consumes: `flush_pending_chunks` из задачи 1.
- Produces: ничего. Задача целиком про проверку.

Зачем отдельной задачей: главное утверждение спеки — «читатели не меняются ни строкой». Оно держится на том, что строка манифеста описывает чанк через `timestamp_ms` и `frame_count`, а это описание не зависит от размера чанка. Утверждение надо проверить, а не объявить.

- [ ] **Step 1: Написать тест на совпадение среза с раскладкой по 100 мс**

Ключ: одна и та же секунда звука, записанная двумя раскладками — укрупнённой и по 100 мс на файл, — обязана давать **побайтово одинаковый** срез.

```rust
/// Одна и та же секунда звука, разложенная по-разному, обязана давать
/// один и тот же срез. Это и есть проверка утверждения «читатели не
/// задеты»: адресация идёт по `timestamp_ms` и `frame_count`, а не по
/// границам файлов.
#[test]
fn enlarged_and_legacy_layouts_read_identically() {
    // Секунда: десять кадров по 100 мс, каждый залит своим номером.
    let frames: Vec<Vec<u8>> = (0..10)
        .map(|index| {
            let sample = (index as i16) + 1;
            (0..1_600).flat_map(|_| sample.to_le_bytes()).collect()
        })
        .collect();

    // Раскладка через накопитель: один файл на всю секунду.
    let enlarged_root = tmp_root();
    let enlarged = {
        let mut store = AudioManifestStore::open(&enlarged_root).unwrap();
        store.begin_session("m1", 1, "").unwrap();
        for (index, frame) in frames.iter().enumerate() {
            store
                .append_chunk(AudioChannel::Mic, frame, 16_000, index as u64 * 100)
                .unwrap();
        }
        store.flush_pending_chunks().unwrap();
        assert_eq!(
            store.list_chunks("m1").unwrap().len(),
            1,
            "секунда легла одним файлом"
        );
        store.read_pcm_range("m1", AudioChannel::Mic, 150, 640).unwrap()
    };

    // Раскладка как до правки: файл на каждые 100 мс. Достигается
    // сбросом после каждого кадра.
    let legacy_root = tmp_root();
    let legacy = {
        let mut store = AudioManifestStore::open(&legacy_root).unwrap();
        store.begin_session("m1", 1, "").unwrap();
        for (index, frame) in frames.iter().enumerate() {
            store
                .append_chunk(AudioChannel::Mic, frame, 16_000, index as u64 * 100)
                .unwrap();
            store.flush_pending_chunks().unwrap();
        }
        assert_eq!(
            store.list_chunks("m1").unwrap().len(),
            10,
            "раскладка по 100 мс на файл"
        );
        store.read_pcm_range("m1", AudioChannel::Mic, 150, 640).unwrap()
    };

    assert_eq!(enlarged.sample_rate, legacy.sample_rate);
    assert_eq!(enlarged.pcm, legacy.pcm, "срез не зависит от раскладки");
    // И это не совпадение двух пустот.
    assert_eq!(enlarged.pcm.len(), 7_840, "490 мс на 16 кГц");
    assert_eq!(enlarged.pcm[0], 2, "срез начинается внутри второго кадра");
    assert_eq!(*enlarged.pcm.last().unwrap(), 7, "и кончается в седьмом");

    let _ = fs::remove_dir_all(&enlarged_root);
    let _ = fs::remove_dir_all(&legacy_root);
}
```

- [ ] **Step 2: Запустить тест**

Run: `cd rust && cargo test -p meetingraft-storage enlarged_and_legacy_layouts_read_identically`

Expected: PASS. Тест проверяет уже сделанное — падать ему не на чем. Если он падает, значит нарезка по времени зависит от размера чанка, и это дефект задачи 1, а не теста.

- [ ] **Step 3: Убедиться, что тест не пустой**

Временно изменить в укрупнённой ветке смещение среза: `read_pcm_range("m1", AudioChannel::Mic, 250, 640)`.

Run: `cd rust && cargo test -p meetingraft-storage enlarged_and_legacy_layouts_read_identically`

Expected: FAIL на `assert_eq!(enlarged.pcm, legacy.pcm)` — тест действительно сравнивает содержимое, а не только длины. Вернуть `150`.

- [ ] **Step 4: Закоммитить**

```bash
cd rust && cargo test -p meetingraft-storage && cargo fmt && cargo clippy --all-targets -- -D warnings
git add rust/crates/storage/src/audio_manifest.rs
git commit -m "test: pin that chunk size does not change what readers return

The same second of audio laid out one-file-per-second and one-file-per-100ms
must slice identically; that is what makes the readers untouched."
```

---

### Task 5: `stop_recording` перестаёт терять ошибку финального сброса

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs:2299-2328` (`stop_recording`), плюс тесты в том же файле, зовущие `append_chunk`
- Modify: `apps/macos/Sources/Audio/AudioCaptureCoordinator.swift:112`, `:137`
- Test: `rust/crates/ffi/src/lib.rs`, `mod tests`

**Interfaces:**
- Consumes: `AudioManifestStore::end_session` (сбрасывает накопитель) и `flush_pending_chunks` из задачи 1.
- Produces: `MeetingCore::stop_recording(&self) -> String` — пустая строка успех, непустая текст ошибки.

Зачем: `stop_recording` делает `let _ = store.end_session(now_ms())` (`:2321`). До этой работы там терялась только запись времени окончания; теперь там же лежат последние 2 с записи, а молчаливый отказ хуже видимой ошибки.

- [ ] **Step 1: Написать падающий тест**

В `mod tests` в `rust/crates/ffi/src/lib.rs`:

```rust
/// Успешная остановка не должна выглядеть как ошибка.
#[test]
fn stop_recording_reports_success_as_empty_string() {
    let root = std::env::temp_dir().join(format!(
        "mr-ffi-stop-ok-{}-{:?}",
        now_ms(),
        std::thread::current().id()
    ));
    let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
    assert!(
        core.start_recording("stop-ok".to_string(), String::new())
            .is_empty()
    );

    assert_eq!(core.stop_recording(), "", "остановка прошла");

    let _ = std::fs::remove_dir_all(&root);
}

/// Хвост записи, лежащий в накопителе, доходит до диска через
/// `stop_recording`: иначе последние секунды встречи теряются молча.
#[test]
fn stop_recording_flushes_the_buffered_tail() {
    let root = std::env::temp_dir().join(format!(
        "mr-ffi-stop-tail-{}-{:?}",
        now_ms(),
        std::thread::current().id()
    ));
    let core = MeetingCore::with_data_root(root.to_string_lossy().into_owned());
    assert!(
        core.start_recording("stop-tail".to_string(), String::new())
            .is_empty()
    );
    // Пять кадров по 100 мс — до целевых 2 с не добирает, значит лежит
    // в накопителе.
    let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
    for index in 0..5 {
        assert!(
            core.ingest_audio_chunk(
                FfiAudioChannel::Mic,
                frame.clone(),
                16_000,
                index * 100
            )
            .is_empty()
        );
    }

    assert_eq!(core.stop_recording(), "");

    assert_eq!(
        core.manifest_chunk_count("stop-tail".to_string()),
        1,
        "хвост дописан при остановке"
    );

    let _ = std::fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Запустить и убедиться, что тесты не компилируются**

Run: `cd rust && cargo test -p meetingraft-ffi stop_recording_reports_success_as_empty_string stop_recording_flushes_the_buffered_tail`

Expected: FAIL сборки — `stop_recording` возвращает `()`, сравнить его с `""` нельзя.

- [ ] **Step 3: Изменить `stop_recording`**

В `rust/crates/ffi/src/lib.rs` заменить сигнатуру и хвост метода (`:2299` и `:2320-2322`):

```rust
    /// Остановить запись. Пустая строка — успех, непустая — текст ошибки.
    ///
    /// Ошибку закрытия сессии здесь терять нельзя: вместе с ней на диск
    /// уходит остаток накопителя, то есть последние секунды записи.
    pub fn stop_recording(&self) -> String {
```

и вместо `let _ = store.end_session(now_ms());`:

```rust
        let mut error = String::new();
        if let Some(store) = guard.store.as_mut() {
            if let Err(err) = store.end_session(now_ms()) {
                error = err.to_string();
            }
        }
```

В конце метода, после `self.jobs.set_recording(false);`, вернуть значение:

```rust
        error
```

- [ ] **Step 4: Запустить тесты FFI**

Run: `cd rust && cargo test -p meetingraft-ffi`

Expected: PASS. Существующие тесты зовут `core.stop_recording();` — отброшенный `String` компилируется без предупреждения.

Если какой-то тест FFI падает на числе чанков, ему нужна та же поправка, что и тестам storage: он копил кадры и ждал файл на каждый. Тесты на `append_chunk` в этом файле — `:2432`, `:2497`, `:2921`, `:3439`. Смотреть по факту падения, а не править вслепую.

- [ ] **Step 5: Перегенерировать биндинги и правку Swift**

Run: `MEETINGRAFT_FFI_FEATURES= apps/macos/Scripts/generate-ffi.sh`

В `apps/macos/Sources/Audio/AudioCaptureCoordinator.swift` в `stopRecording()` (`:134-142`) заменить `core.stopRecording()` на:

```swift
        let error = core.stopRecording()
        if !error.isEmpty {
            lastError = "Не удалось сохранить хвост записи: \(error)"
        }
```

В аварийной ветке запуска микрофона (`:112`) результат намеренно отбрасывается:

```swift
            // lastError уже содержит настоящую причину — не перетирать.
            // Записывать нечего: запись не началась.
            _ = core.stopRecording()
```

- [ ] **Step 6: Коммит**

```bash
cd rust && cargo fmt && cargo clippy --all-targets -- -D warnings
git add rust/crates/ffi/src/lib.rs apps/macos/Sources/Audio/AudioCaptureCoordinator.swift
git commit -m "fix: surface the error from the final audio flush

stop_recording discarded end_session's error. That cost nothing before;
now the last two seconds of the recording leave through it."
```

- [ ] **Step 7: Отметить, что Swift не проверен**

Swift здесь **не собран** — на VPS его нет. В отчёте по задаче сказать прямо: правка `AudioCaptureCoordinator` и перегенерация биндингов требуют `scripts/verify-mac.sh` на Маке. Проверенной эту часть не заявлять.

---

### Task 6: Документация

**Files:**
- Modify: `docs/backlog.md:657-659` (пункт про укрупнение в Epic 22)

**Interfaces:**
- Consumes: ничего.
- Produces: ничего.

- [ ] **Step 1: Закрыть пункт в backlog**

Пункт `- Укрупнить чанк на диске: 100 мс — размер кадра живого пути, у хранения причин быть таким же нет. Секунда-две снимают порядок по числу файлов, не трогая ни микшер, ни read_pcm_range` заменить на:

```markdown
- [x] **Укрупнён чанк на диске.** 2 с вместо 100 мс: 3 600 файлов и строк
  в час вместо 72 000, плюс возвращается перерасход блока APFS (файл в
  3 200 байт занимал 4 КБ — около 65 МБ в час на округлении).

  Укрупнение живёт в `storage`, а не в Swift: `ingest_audio_chunk` на одном
  чанке и пишет на диск, и толкает в `ChannelMixer`, так что поднять
  `chunkDurationMs` значило бы добавить две секунды задержки субтитрам.
  Живой кадр остался 100 мс.

  Читатели не менялись ни строкой: строка манифеста описывает чанк через
  `timestamp_ms` и `frame_count`, и это описание не зависит от размера
  чанка. По той же причине старые записи не мигрировали. Спека и план — в
  `docs/superpowers/`
```

- [ ] **Step 2: Коммит**

```bash
git add docs/backlog.md
git commit -m "docs: close the audio chunk size item in Epic 22"
```

---

## Self-Review

**Покрытие спеки:**

| Требование спеки | Задача |
|---|---|
| накопитель на канал, сброс по 2 с | 1 |
| сброс в `end_session` | 1 |
| очистка в `begin_session` | 1 |
| сброс по разрыву тайм-кодов | 2 |
| сброс по смене частоты | 3 |
| `read_pcm_range` не задет | 4 |
| старые записи без миграции | 4 (раскладка по 100 мс читается тем же кодом) |
| `stop_recording -> String`, `lastError` в Swift | 5 |
| `fsync` не делаем | нигде — сознательно, обосновано в спеке |
| паузы не вырезаем | нигде — накопитель пишет всё, что пришло |
| backlog | 6 |

**Тип возврата:** `append_chunk` → `Result<Option<ManifestChunk>, AudioManifestError>` в задаче 1, и в задачах 2–4 используется именно так. `flush_pending_chunks` → `Result<Vec<ManifestChunk>, _>` всюду. `stop_recording` → `String` в задаче 5, тесты сравнивают с `""`.

**Отклонение от спеки, отмеченное при планировании.** Спека перечисляет тест «старая база с чанками по 100 мс читается как раньше» отдельным. В плане он слит с проверкой раскладок (задача 4): «старая база» и «раскладка по 100 мс на файл» — это одно и то же для читателя, потому что читатель адресует чанк по `timestamp_ms` и `frame_count` и о размере файла не знает. Отдельный тест на созданную вручную базу проверял бы миграцию, которой нет.

**Честная оговорка про задачу 2.** У текущего Swift-конвейера тайм-коды внутри канала непрерывны всегда: у каждого канала свой `AudioChunkPipeline` со своим `nextFrame`, растущим на постоянные 1600 кадров. То есть ветка разрыва — защита инварианта манифеста, а не рабочий путь, и её тест строит разрыв искусственно. Это не повод её не делать: без неё инвариант держится на честном слове вызывающего.
