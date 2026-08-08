# Audio FLAC Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Хранить аудио в FLAC вместо сырого PCM, не потеряв ни одного сэмпла и не задев ни живой путь, ни старые записи.

**Architecture:** Формат описывается **строкой манифеста**, а не глобальной настройкой: новая колонка `codec` со значением по умолчанию `pcm_s16le`. Читатель диспетчеризуется по колонке, поэтому старые записи читаются как раньше, а в одной сессии могут соседствовать пачки обоих форматов. `write_pending` кодирует пачку в самостоятельный поток FLAC; при отказе кодера пачка ложится сырым PCM со своим значением `codec`, а ошибка уезжает наверх.

**Tech Stack:** Rust (крейт `meetingraft-storage`), SQLite через `rusqlite`, `flac-codec` 1.3.2.

Спека: `docs/superpowers/specs/2026-08-08-audio-flac-design.md`.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; сообщения коммитов — **по-английски** (`CLAUDE.md`).
- Тесты гонять по крейтам, полный `cargo test` по workspace не влезает в память VPS: `cd rust && cargo test -p meetingraft-storage -p meetingraft-ffi`.
- Имя пакета с префиксом: крейт `storage` — это пакет `meetingraft-storage`. `-p storage` молча не найдёт ничего.
- Перед коммитом: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`. **Каждый коммит зелёный**, откладывать починку чужих тестов на потом нельзя.
- **Порядок задач обязателен: читатель раньше писателя.** Задача 2 учит читать FLAC, задача 3 начинает его писать. Обратный порядок оставил бы между коммитами состояние, в котором записанное не читается.
- Опции кодера — **только** `Options::default().no_padding().no_seektable()`. Умолчание кладёт 4 КБ padding и таблицу перемотки, это 4 122 байта на пачку и 14.8 МБ в час мёртвого веса.
- `frame_count` в манифесте остаётся числом кадров **исходного PCM** и от кодека не зависит. Адресация (`timestamp_ms` + `frame_count`) не меняется ни в чём.
- Граница UniFFI не трогается: `ManifestChunk` за пределами `audio_manifest.rs` не используется, в `ffi` его нет ни разу. Swift в этой работе не участвует.
- Каждый тест обязан падать без своей ветки. На этом Epic 19 обжёгся трижды: тест бывает зелёным, проверяя не то.

---

## File Structure

| Файл | Ответственность |
|---|---|
| `rust/crates/storage/Cargo.toml` | зависимость `flac-codec` |
| `rust/crates/storage/src/migrations.rs` | шаг 9 — колонка `codec` |
| `rust/crates/storage/src/audio_manifest.rs` | кодирование при сбросе, декодирование у обоих читателей, откат при отказе кодера. Вся суть работы здесь |
| `docs/backlog.md` | закрыть пункт про FLAC, записать замеренное |

---

### Task 1: Колонка `codec`

Формат начинает быть описанным. Поведение не меняется ни в чём: пишем и читаем тот же сырой PCM, просто теперь говорим об этом вслух.

**Files:**
- Modify: `rust/crates/storage/src/migrations.rs` (шаг 9 в `STEPS`), `rust/crates/storage/src/audio_manifest.rs:325-338` (INSERT в `write_pending`), `:858-866` (SELECT в `read_session_pcm`), `:912-920` (SELECT в `read_pcm_range`)
- Test: `migrations.rs` (модуль `mod tests`), `audio_manifest.rs` (модуль `mod tests`)

**Interfaces:**
- Produces:
  - шаг миграции 9, `schema_version()` становится 9
  - `const CODEC_PCM: &str = "pcm_s16le";` — приватная константа в `audio_manifest.rs`
  - оба SELECT возвращают `codec`; значение пока только проверяется на равенство `CODEC_PCM`, ветки для `flac` ещё нет

- [ ] **Step 1: Падающий тест на миграцию старой базы**

В `mod tests` в `rust/crates/storage/src/migrations.rs`:

```rust
/// База, созданная до появления колонки, обязана подняться так, чтобы её
/// чанки описывались честно: до этой правки формат был только сырой PCM.
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
        .query_row("SELECT codec FROM audio_manifest WHERE seq = 0", [], |row| {
            row.get(0)
        })
        .expect("codec");
    assert_eq!(
        codec, "pcm_s16le",
        "старая строка описана неверно — её файл прочитают как FLAC"
    );
}
```

Тест обязан не компилироваться или падать до шага 2 (колонки нет).

- [ ] **Step 2: Шаг миграции 9**

В конец `STEPS` в `rust/crates/storage/src/migrations.rs`:

```rust
    // 9 — формат файла чанка (Epic 22). Умолчание `pcm_s16le` — это и есть
    // отсутствие миграции данных: всё уже записанное описывается верно, а
    // файлы не трогаются. Колонка на строку, а не настройка на базу: в
    // одной сессии могут соседствовать пачки разных форматов, и это не
    // случайность, а рабочий путь отката при отказе кодера.
    "
    ALTER TABLE audio_manifest ADD COLUMN codec TEXT NOT NULL DEFAULT 'pcm_s16le';
    ",
```

- [ ] **Step 3: Писать и читать колонку**

В `write_pending` (`audio_manifest.rs:325`) добавить колонку в INSERT:

```rust
        self.conn.execute(
            "INSERT INTO audio_manifest
             (session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms, codec)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id,
                channel.dir_name(),
                seq,
                rel,
                pending.sample_rate,
                frame_count,
                pending.timestamp_ms as i64,
                CODEC_PCM
            ],
        )?;
```

Рядом с `CHUNK_TARGET_MS` (`:63`):

```rust
/// Сырой PCM `i16` little-endian — формат до появления сжатия.
///
/// Значение по умолчанию колонки `codec`, поэтому им же описаны все
/// записи, сделанные до этой работы.
const CODEC_PCM: &str = "pcm_s16le";
```

В обоих читателях добавить `codec` в SELECT и в разбор строки. В `read_session_pcm` (`:858`):

```rust
        let mut statement = self.conn.prepare(
            "SELECT path, frame_count, codec
             FROM audio_manifest
             WHERE session_id = ?1 AND channel = ?2
             ORDER BY seq",
        )?;
```

В `read_pcm_range` (`:912`) — так же, пятой колонкой `codec` после `timestamp_ms`.

Пока обе ветки чтения одинаковы: значение читается, но использовать его нечем — декодера ещё нет. Городить `match` с одной рукой не надо, достаточно связать значение и оставить его для задачи 2.

- [ ] **Step 4: Тест на новые строки**

```rust
/// Пачка, записанная сейчас, описывает свой формат — иначе читатель
/// после задачи 3 не сможет отличить её от FLAC.
#[test]
fn a_written_chunk_records_its_codec() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..1_600).flat_map(|_| 7_i16.to_le_bytes()).collect();
        store
            .append_chunk(AudioChannel::Mic, &frame, 16_000, 0)
            .unwrap();
        store.flush_pending_chunks().unwrap();

        let codec: String = store
            .connection()
            .query_row("SELECT codec FROM audio_manifest", [], |row| row.get(0))
            .unwrap();
        assert_eq!(codec, "pcm_s16le");
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 5: Проверка**

```
cd rust && cargo test -p meetingraft-storage -p meetingraft-ffi
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

**Мутация:** убрать `DEFAULT 'pcm_s16le'` из шага 9 → `migration_marks_existing_chunks_as_raw_pcm` обязан упасть (`NOT NULL` без умолчания вообще не даст мигрировать непустую таблицу). Приложить дословный вывод.

---

### Task 2: Читатели учатся декодировать FLAC

Писатель ещё не трогается. Тесты строят FLAC-чанк сами, кодером напрямую, и кладут строку в манифест руками — так читатель проверяется в отрыве от писателя, и между коммитами не возникает состояния «записали то, что не прочитаем».

**Files:**
- Modify: `rust/crates/storage/Cargo.toml`, `rust/crates/storage/src/audio_manifest.rs:17-50` (вариант ошибки), `:853-887` (`read_session_pcm`), `:937-973` (тело цикла `read_pcm_range`)
- Test: `audio_manifest.rs`, модуль `mod tests`

**Interfaces:**
- Produces:
  - `const CODEC_FLAC: &str = "flac";`
  - `AudioManifestError::Flac { path: String, message: String }`
  - `fn decode_chunk(bytes: &[u8], codec: &str, path: &str) -> Result<Vec<i16>, AudioManifestError>` — приватная свободная функция

- [ ] **Step 1: Зависимость**

В `rust/crates/storage/Cargo.toml`:

```toml
flac-codec = "1.3.2"
```

Дерево целиком: `arrayvec`, `bitstream-io` → `no_std_io2` → `memchr`, `md5`. Ни одного системного крейта — это условие, а не совпадение: `cmake` уже нужен `whisper.cpp`, вторая системная библиотека удвоила бы поверхность, на которой ломается сборка.

- [ ] **Step 2: Падающий тест на чтение FLAC-чанка**

```rust
/// FLAC-пачка, положенная в манифест руками, читается как обычная
/// дорожка. Пишет её кодер напрямую — писателя в этой задаче ещё нет.
#[test]
fn a_flac_chunk_reads_back_sample_for_sample() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();

        // Пила, а не константа: константу «декодировал бы» и сломанный
        // декодер, вернувший нули.
        let samples: Vec<i16> = (0..32_000).map(|n| ((n % 4_000) as i16) - 2_000).collect();
        let encoded = encode_for_test(&samples);

        let rel = "sessions/s1/mic/000000.flac";
        let abs = root.join(rel);
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(&abs, &encoded).unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO audio_manifest
                 (session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms, codec)
                 VALUES ('s1', 'mic', 0, ?1, 16000, 32000, 0, 'flac')",
                params![rel],
            )
            .unwrap();

        let read = store.read_session_pcm("s1", AudioChannel::Mic).unwrap();
        assert_eq!(read, samples, "FLAC обязан быть без потерь");
        assert!(
            encoded.len() < samples.len() * 2,
            "поток не сжался — проверять нечего"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
```

Хелпер там же, в `mod tests` — он понадобится и задаче 3:

```rust
/// Кодирование пачки для тестов. Опции те же, что в проде: тест на
/// умолчаниях проверял бы не тот формат, который мы пишем.
pub(crate) fn encode_for_test(samples: &[i16]) -> Vec<u8> {
    let widened: Vec<i32> = samples.iter().map(|&s| i32::from(s)).collect();
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = flac_codec::encode::FlacSampleWriter::new(
            &mut buffer,
            flac_codec::encode::Options::default()
                .no_padding()
                .no_seektable(),
            16_000,
            16,
            1,
            Some(widened.len() as u64),
        )
        .unwrap();
        writer.write(&widened).unwrap();
        writer.finalize().unwrap();
    }
    buffer.into_inner()
}
```

- [ ] **Step 3: Вариант ошибки**

В `AudioManifestError` (`:17-50`):

```rust
    #[error("flac {path}: {message}")]
    Flac { path: String, message: String },
```

Отдельный вариант, а не `Io`: испорченный поток и недоступный файл — разные беды, и человеку, который смотрит на ошибку, разница важна.

- [ ] **Step 4: Декодер**

Свободной функцией рядом с `channel_index` (`:124`):

```rust
/// Кадры чанка из его байтов, по формату из строки манифеста.
///
/// Неизвестный кодек — ошибка, а не «попробуем как PCM»: строка манифеста
/// единственное, что говорит о формате файла, и угадывать здесь значит
/// отдать мусор под видом звука.
fn decode_chunk(bytes: &[u8], codec: &str, path: &str) -> Result<Vec<i16>, AudioManifestError> {
    match codec {
        CODEC_PCM => Ok(bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()),
        CODEC_FLAC => {
            let mut reader = flac_codec::decode::FlacSampleReader::new(std::io::Cursor::new(bytes))
                .map_err(|error| AudioManifestError::Flac {
                    path: path.to_string(),
                    message: error.to_string(),
                })?;
            let mut out = Vec::new();
            let mut buf = vec![0i32; 4_096];
            loop {
                let read = reader.read(&mut buf).map_err(|error| AudioManifestError::Flac {
                    path: path.to_string(),
                    message: error.to_string(),
                })?;
                if read == 0 {
                    break;
                }
                out.extend(buf[..read].iter().map(|&sample| sample as i16));
            }
            Ok(out)
        }
        other => Err(AudioManifestError::Flac {
            path: path.to_string(),
            message: format!("unknown codec {other}"),
        }),
    }
}
```

- [ ] **Step 5: Оба читателя через `decode_chunk`**

В `read_session_pcm` заменить разбор байтов и проверку длины:

```rust
            let samples = decode_chunk(&bytes, &codec, &relative)?;
            if samples.len() != frame_count {
                return Err(AudioManifestError::ChunkTruncated {
                    path: relative,
                    expected_frames: frame_count,
                    actual_frames: samples.len(),
                });
            }
            pcm.extend_from_slice(&samples);
```

Проверка целостности **усиливается**, а не переезжает: сегодня `bytes.len() / 2` ловит только обрезанный файл, а число декодированных кадров поймает и порчу внутри потока — у FLAC есть CRC кадров и MD5 всего потока, и битый поток не досчитает кадров или откажет на чтении.

В `read_pcm_range` — то же преобразование, срез считается от `samples` как сейчас; логика `from`/`to` не трогается.

- [ ] **Step 6: Тест на битый поток**

```rust
/// Испорченный FLAC обязан стать ошибкой. Отдать короткий фрагмент
/// значило бы подсунуть человеку тишину вместо звука, и он решит, что
/// так и было записано.
#[test]
fn a_corrupt_flac_chunk_is_an_error_not_a_short_fragment() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();

        let samples: Vec<i16> = (0..32_000).map(|n| ((n % 4_000) as i16) - 2_000).collect();
        let mut encoded = encode_for_test(&samples);
        // Рвём середину звуковых кадров, а не заголовок: заголовок отсеял
        // бы и совсем наивный читатель.
        let middle = encoded.len() / 2;
        encoded[middle] ^= 0xFF;

        let rel = "sessions/s1/mic/000000.flac";
        let abs = root.join(rel);
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(&abs, &encoded).unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO audio_manifest
                 (session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms, codec)
                 VALUES ('s1', 'mic', 0, ?1, 16000, 32000, 0, 'flac')",
                params![rel],
            )
            .unwrap();

        let result = store.read_session_pcm("s1", AudioChannel::Mic);
        assert!(
            result.is_err(),
            "битый поток прочитан без ошибки — человек получит тишину вместо звука"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
```

Если окажется, что порча именно этого байта проходит контроль (CRC покрывает не каждый байт заголовка кадра), сдвинуть точку порчи и **зафиксировать в отчёте, какая именно порча ловится, а какая нет** — это свойство формата, и знать его надо. Молча подобрать «удобный» байт нельзя.

- [ ] **Step 7: Проверка**

```
cd rust && cargo test -p meetingraft-storage -p meetingraft-ffi
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

**Мутация:** заменить руку `CODEC_FLAC` в `decode_chunk` на разбор байтов как PCM → `a_flac_chunk_reads_back_sample_for_sample` обязан упасть на сравнении сэмплов. Приложить дословный вывод.

---

### Task 3: Кодирование при сбросе

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs:288-350` (`write_pending`)
- Test: `audio_manifest.rs`, модуль `mod tests`

**Interfaces:**
- Produces:
  - `fn encode_chunk(pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>, String>` — приватная свободная функция; ошибка текстом, потому что вызывающий её не отдаёт наверх как есть (задача 4)

- [ ] **Step 1: Падающий тест на круговой проход через писателя**

```rust
/// То, что записано живым путём, читается сэмпл в сэмпл. Главный тест
/// работы: всё остальное имеет смысл, только если сжатие без потерь.
#[test]
fn a_recorded_chunk_survives_the_round_trip_unchanged() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();

        // Двадцать кадров живого пути, каждый со своим наполнением.
        let mut expected: Vec<i16> = Vec::new();
        for index in 0..20u64 {
            let frame: Vec<i16> = (0..1_600)
                .map(|n| ((n + index as i32 * 1_600) % 4_000) as i16 - 2_000)
                .collect();
            let bytes: Vec<u8> = frame.iter().flat_map(|s| s.to_le_bytes()).collect();
            store
                .append_chunk(AudioChannel::Mic, &bytes, 16_000, index * 100)
                .unwrap();
            expected.extend_from_slice(&frame);
        }
        store.flush_pending_chunks().unwrap();

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 1, "пачка на 2 с");
        assert_eq!(chunks[0].frame_count, 32_000, "кадры считаются по PCM");

        let codec: String = store
            .connection()
            .query_row("SELECT codec FROM audio_manifest", [], |row| row.get(0))
            .unwrap();
        assert_eq!(codec, "flac");
        assert!(
            chunks[0].path.extension().unwrap() == "flac",
            "расширение обязано идти за кодеком"
        );

        let read = store.read_session_pcm("s1", AudioChannel::Mic).unwrap();
        assert_eq!(read, expected, "сжатие потеряло сэмплы");

        let on_disk = fs::metadata(root.join(&chunks[0].path)).unwrap().len();
        assert!(
            on_disk < 64_000,
            "файл не меньше сырого PCM — сжатия нет: {on_disk} байт"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Кодер**

Свободной функцией рядом с `decode_chunk`:

```rust
/// Пачка в самостоятельный поток FLAC.
///
/// Кодируется в память, а не прямо в файл: при отказе кодера на диске не
/// должно остаться недописанного файла, за который потом отвечает
/// манифест.
///
/// `no_padding().no_seektable()` обязательны. Умолчание крейта кладёт в
/// поток 4 КБ padding и таблицу перемотки — 4 122 байта на пачку и
/// 14.8 МБ в час мёртвого веса. Перематывать файл на 2 с не по чему: он
/// сам себе единица адресации, её даёт манифест.
fn encode_chunk(pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>, String> {
    let samples: Vec<i32> = pcm
        .chunks_exact(2)
        .map(|pair| i32::from(i16::from_le_bytes([pair[0], pair[1]])))
        .collect();

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = flac_codec::encode::FlacSampleWriter::new(
            &mut buffer,
            flac_codec::encode::Options::default()
                .no_padding()
                .no_seektable(),
            sample_rate,
            16,
            1,
            Some(samples.len() as u64),
        )
        .map_err(|error| error.to_string())?;
        writer.write(&samples).map_err(|error| error.to_string())?;
        writer.finalize().map_err(|error| error.to_string())?;
    }
    Ok(buffer.into_inner())
}
```

- [ ] **Step 3: `write_pending` пишет FLAC**

Между вычислением `seq` и формированием пути (`:309-322`):

```rust
        let encoded = encode_chunk(&pending.pcm, pending.sample_rate);
        let (codec, extension, payload) = match &encoded {
            Ok(bytes) => (CODEC_FLAC, "flac", bytes.as_slice()),
            // Откат — задача 4; пока отказ кодера ведёт себя как раньше.
            Err(error) => return Err(AudioManifestError::Flac {
                path: format!("sessions/{session_id}/{}/{seq:06}", channel.dir_name()),
                message: error.clone(),
            }),
        };

        let rel = format!(
            "sessions/{}/{}/{:06}.{}",
            session_id,
            channel.dir_name(),
            seq,
            extension
        );
```

`fs::write(&abs, payload)?` вместо `&pending.pcm`, `codec` в INSERT вместо `CODEC_PCM`.

**`frame_count` считается по-прежнему из `pending.pcm.len() / 2`** — по исходному PCM. Это не мелочь: на нём стоит вся адресация, и посчитать его от сжатых байтов значило бы сломать `read_pcm_range` и тайм-коды сегментов.

- [ ] **Step 4: Тест на отсутствие padding и seektable**

```rust
/// В потоке не должно быть ни padding, ни таблицы перемотки. Умолчание
/// крейта кладёт и то и другое: 4 122 байта на пачку, 14.8 МБ в час.
#[test]
fn the_written_stream_carries_no_padding_or_seektable() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();
        let frame: Vec<u8> = (0..32_000).flat_map(|n| ((n % 97) as i16).to_le_bytes()).collect();
        store
            .append_chunk(AudioChannel::Mic, &frame, 16_000, 0)
            .unwrap();
        store.flush_pending_chunks().unwrap();

        let path = store.list_chunks("s1").unwrap()[0].path.clone();
        let bytes = fs::read(root.join(&path)).unwrap();
        assert_eq!(&bytes[..4], b"fLaC");

        // Заголовки блоков метаданных: байт (last<<7 | type), затем длина
        // тремя байтами big-endian.
        let mut offset = 4;
        let mut types = Vec::new();
        loop {
            let header = bytes[offset];
            let block_type = header & 0x7F;
            let length = u32::from_be_bytes([0, bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
            types.push(block_type);
            offset += 4 + length as usize;
            if header & 0x80 != 0 {
                break;
            }
        }
        assert!(!types.contains(&1), "в потоке padding: {types:?}");
        assert!(!types.contains(&3), "в потоке таблица перемотки: {types:?}");
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 5: Проверка**

Существующие тесты `audio_manifest`, сверяющие содержимое файлов побайтно, придётся поправить — они писались под сырой PCM. Это правка тестов, а не ослабление: утверждения о **сэмплах** остаются те же, меняется только то, что содержимое достаётся декодером. Тесты, сверяющие `read_pcm_range` и `read_session_pcm`, меняться не должны вовсе — если такой тест потребовал правки, это сигнал, что читатель задет, и разбираться надо с читателем.

```
cd rust && cargo test -p meetingraft-storage -p meetingraft-ffi
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

**Мутации, обе обязательны:**
1. Убрать `.no_padding().no_seektable()` → `the_written_stream_carries_no_padding_or_seektable` обязан упасть.
2. В `encode_chunk` сдвинуть один сэмпл на единицу → `a_recorded_chunk_survives_the_round_trip_unchanged` обязан упасть на сравнении сэмплов, а не на размере файла. Это проверка, что тест сверяет звук, а не факт «что-то вернулось».

Приложить дословный вывод обеих.

---

### Task 4: Отказ кодера не теряет звук

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs` (ветка `Err` в `write_pending`)
- Test: `audio_manifest.rs`, модуль `mod tests`

- [ ] **Step 1: Падающий тест**

Сеам для отказа кодера: `sample_rate` больше 2²⁰ — `FlacSampleWriter::new` отвергает такую частоту по формату. Продакшн-путь этим не ходит (`append_chunk` уже отсеивает мусорные кадры), а тест получает честный отказ кодера без подмены зависимостей.

```rust
/// Отказ кодера не должен стоить человеку двух секунд записи. Пачка
/// ложится сырым PCM, строка описывает её честно, ошибка уезжает наверх.
#[test]
fn an_encoder_failure_falls_back_to_raw_pcm_and_still_reports() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        store.begin_session("s1", 1, "").unwrap();

        let samples: Vec<i16> = (0..1_600).map(|n| (n % 500) as i16).collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        // Частота вне допустимого для FLAC — кодер откажет.
        store
            .append_chunk(AudioChannel::Mic, &bytes, 2_000_000, 0)
            .unwrap();
        let error = store.flush_pending_chunks().unwrap_err();

        assert!(
            matches!(error, AudioManifestError::Flac { .. }),
            "отказ кодера промолчал: {error:?}"
        );

        let chunks = store.list_chunks("s1").unwrap();
        assert_eq!(chunks.len(), 1, "звук потерян из-за отказа кодера");
        let codec: String = store
            .connection()
            .query_row("SELECT codec FROM audio_manifest", [], |row| row.get(0))
            .unwrap();
        assert_eq!(codec, "pcm_s16le", "пачка описана не тем форматом");

        let read = store.read_session_pcm("s1", AudioChannel::Mic).unwrap();
        assert_eq!(read, samples, "откат исказил звук");
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Откат**

Ветка `Err` в `write_pending` перестаёт возвращать сразу: пачка пишется сырым PCM с `codec = CODEC_PCM` и расширением `.pcm`, строка манифеста вставляется как обычно, и **только после этого** возвращается `Err(AudioManifestError::Flac { .. })`.

Контракт необычный и обязан быть описан в доке метода: **ошибка возвращается при уже совершённой и зафиксированной записи.** Данные целы, человек предупреждён. Обратный порядок — вернуть ошибку и не записать ничего — потерял бы 2 с звука там, где терять было не нужно.

Известное следствие, которое надо назвать в отчёте, а не обойти молчанием: `ingest_audio_chunk` (`ffi/src/lib.rs`, ветка `append_chunk`) на ошибке возвращается сразу и **не толкает этот кадр в микшер**, то есть 100 мс не дойдут до живых субтитров. Это существующее поведение для любой ошибки хранения, и правка его не ухудшает — сегодня в такой ситуации терялся бы и звук на диске, и кадр в микшере. Менять `ingest_audio_chunk` в этой работе не нужно.

- [ ] **Step 3: Тест на смешанную сессию**

```rust
/// Пачки разных форматов в одной сессии читаются одной дорожкой. Это не
/// экзотика: ровно так выглядит запись, в которой кодер один раз отказал.
#[test]
fn a_session_mixing_flac_and_raw_chunks_reads_as_one_track() { /* ... */ }
```

Собрать сессию из двух пачек: одну через обычный путь (FLAC), вторую вставить руками с `codec = 'pcm_s16le'` и файлом сырого PCM, и сверить, что `read_session_pcm` отдаёт склейку сэмплов обеих в порядке `seq`.

- [ ] **Step 4: Проверка**

```
cd rust && cargo test -p meetingraft-storage -p meetingraft-ffi
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

**Мутация:** вернуть в ветке `Err` немедленный `return` вместо отката → `an_encoder_failure_falls_back_to_raw_pcm_and_still_reports` обязан упасть на `chunks.len()`, а не на типе ошибки. Приложить дословный вывод.

---

### Task 5: Замеры и документация

Задача не пишет продакшн-кода. Её смысл в том, чтобы в бэклоге стояли **замеренные** числа, а не переписанные из спеки ожидания.

**Files:**
- Modify: `docs/backlog.md` (пункт FLAC в Epic 22)

- [ ] **Step 1: Замерить сжатие на настоящей записи**

Число 62.6% из спеки получено на синтетическом сигнале и результатом не является. Прогнать по реальной записи со встречи: сумма размеров файлов сессии против `frame_count * 2`. Отдельно посмотреть, насколько каналы расходятся — микрофонный с паузами обязан сжиматься сильнее системного.

Замер делается на Маке, где есть записи. Если записи под рукой нет — **записать её**, а не подставить синтетику.

- [ ] **Step 2: Замерить кодирование на Маке**

Время `encode_chunk` на пачку против бюджета 50 мс, которым `drain_events` ходит через мьютекс ядра. На Linux-машине разработки — 3.7 мс. Если на Маке уложились с запасом — записать число и закрыть вопрос. Если нет — **не чинить здесь**: завести пункт на вынос кодирования из-под мьютекса, как Epic 21 вынес сетевые вызовы, и написать в бэклоге, почему это понадобилось.

- [ ] **Step 3: Живая проверка**

Идёт запись, субтитры не спотыкаются на границах пачек. Кодирование раз в 2 с на канал — всплеск, а не ровная нагрузка, и увидеть его можно только на живом пути.

- [ ] **Step 4: Бэклог**

Закрыть пункт `- FLAC вместо сырого PCM` в Epic 22, записав: замеренный коэффициент на настоящей записи, время кодирования на Маке, отказ от `Options::default()` с цифрой мёртвого веса, и то, что старые записи не мигрировали. Соседние пункты (порядок, срок хранения) не трогать.

Пункт **не закрывать**, пока замеры не сделаны: закрытый пункт с числами из спеки — это ровно тот зелёный тест, который проверяет не то.

---

## Self-Review

Что в этом плане легче всего сделать не так.

**Порядок задач.** Читатель раньше писателя. Если задачи 2 и 3 поменять местами, между коммитами возникнет состояние, где записанное не читается, и «зелёный коммит» перестанет что-либо значить.

**`frame_count`.** Считается по исходному PCM и только по нему. Соблазн посчитать его от того, что легло на диск, ломает адресацию, тайм-коды сегментов и прослушивание правленого куска — то есть ровно то, ради чего аудио хранится.

**`Options::default()`.** Пишется не глядя и стоит 14.8 МБ в час. Тест на это выглядит мелочью и ею не является.

**Тест кругового прохода.** Сверять сэмплы, а не факт, что что-то вернулось, и не только длину. Данные в тесте должны меняться по ходу пачки: на константе круговой проход пройдёт и у сломанного декодера, вернувшего нули.

**Откат при отказе кодера.** Ошибка возвращается **после** совершённой записи. Это необычно, и если реализующий «исправит» это на привычный ранний возврат, работа потеряет смысл — 2 с звука пропадут там, где пропадать не должны.

**Замеры в задаче 5.** Числа из спеки — ожидания, полученные на синтетике. Переписать их в бэклог как результат нельзя.
