# Интерфейс правки транскрипта (Epic 19) — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Правка сегмента Final делается прямо в транскрипте — с прослушиванием фрагмента, возвратом к исходному, повышением подсказки до замены и видимым разделом правок, не легших на версию.

**Architecture:** Ядро правки (PR #31) уже работает; здесь дозакрывается граница FFI и строится интерфейс. Домен получает `original_text` у сегмента, `postcall` — поиск подсказки, родившейся из правки, граница — два поля и один метод. Swift правит строку на месте в существующем `FinalSegmentsView`, где уже живёт переназначение спикера.

**Tech Stack:** Rust (`domain`, `storage`, `postcall`, `ffi`), UniFFI, SwiftUI, AVFoundation.

Спека: `docs/superpowers/specs/2026-08-05-transcript-edit-ui-design.md`.

## Состояние на 2026-08-05

Все семь задач выполнены на ветке `feat/epic-19-transcript-edit-ui`.

- **Задачи 1–3 (Rust) проверены здесь**: `meetingraft-storage` 57 тестов,
  `meetingraft-postcall` 100, `meetingraft-ffi` 50; `cargo fmt --check` и
  `cargo clippy --all-targets -- -D warnings` чисто. Ключевой тест
  `edited_segment_carries_recognized_text` отдельно проверен на падение
  без исправления.
- **Задачи 4–7 (Swift) не собирались**: на VPS нет ни `swift`, ни
  `xcodebuild`, ни `swiftformat`. Шаги «прогнать на Mac» остались
  неотмеченными намеренно — этот код не проверен.

Отступления от плана, сделанные по ходу:

1. **Task 2**, тестовый конструктор термина: у `domain::GlossaryTerm` нет
   поля `updated_at_ms`, в плане оно было. Убрано.
2. **Task 3**, вспомогательные в `mod tests`: `tmp_root_string`,
   `seed_two_edits`, `seed_final_segment` из плана в коде отсутствуют.
   Взяты фактические — `edits_root(name)`, `seed_segment_version`, — и
   дописан `seed_two_unapplied_edits`.
3. **Task 4**, подделка ядра: вместо нового `segmentsOverride`
   используется существующее свойство `segments` спая — оно и так
   питает `listFinalSegments`.
4. **Task 7**, чтение звука: план брал `audioFragment(for:)` на каждую
   строку в `body`, то есть чтение с диска на каждую перерисовку списка,
   а перерисовка идёт на каждое нажатие клавиши в поле. Кнопка живёт
   только в раскрытой строке, поэтому фрагмент читается один раз при
   входе в правку и хранится в `@State playableFragment`.

## Global Constraints

- Комментарии и документация в коде — по-русски; сообщения коммитов — по-английски.
- Бизнес-логика в Rust; слой FFI только отдаёт данные, в SwiftUI — ни разбора диффа, ни знания про виды записи глоссария (`AGENTS.md`).
- Мутирующие методы границы возвращают `String`: пусто — успех, непусто — текст ошибки. Соглашение не нарушать.
- Приватные хелперы в `ffi` пишутся свободными функциями, а не методами: приватный метод внутри `#[uniffi::export]`-блока всё равно пытается пройти через границу.
- Проверка после каждой задачи: `cd rust && cargo test -p <крейт>`. Полный `cargo test` по workspace в память VPS не влезает.
- Имена пакетов с префиксом: крейт `storage` — это пакет `meetingraft-storage`.
- Перед коммитом: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- Swift собирается только на Mac (`scripts/verify-mac.sh`). Задачи 4–7 уходят на проверку человеку; не объявлять их проверенными.
- Работа идёт веткой и pull request, в `main` не коммитить.

## Ловушка, из-за которой задачи 1 и 3 выглядят странно

`reattach_edits` (`rust/crates/postcall/src/edits.rs:37`) ищет сегмент по правилу `segment.text.contains(edit.original_text)` — **вхождение, а не равенство**. После пересбора распознанный текст сегмента может быть длиннее сохранённого в правке.

При этом `edit_segment_text` определяет возврат к исходному сравнением с `previous.original_text` (`rust/crates/ffi/src/lib.rs:1531–1537`).

Отсюда правило, которое нельзя нарушать: **`original_text` у сегмента — это `edit.original_text` из журнала, а не текст из таблицы `final_segments`.** Взять текст из таблицы кажется логичнее, но тогда кнопка «вернуть исходное» отправит текст, который не совпадёт с хранимым, и вместо удаления правки заведёт новую.

## Файлы

| Файл | Ответственность |
|---|---|
| `rust/crates/domain/src/postcall.rs` | поле `original_text` у `FinalSegment` |
| `rust/crates/storage/src/audio_manifest.rs` | заполнение `original_text` при чтении сегментов |
| `rust/crates/postcall/src/term_from_edit.rs` | поиск подсказки, родившейся из правки |
| `rust/crates/ffi/src/lib.rs` | два поля `FfiFinalSegment`, метод `delete_segment_edit` |
| `apps/macos/Sources/Meetings/SpeakerAttributionViewModel.swift` | контракт ядра и состояние правки |
| `apps/macos/Sources/Meetings/FinalSegmentsView.swift` | раскрытие строки и полоска действий |
| `apps/macos/Sources/Meetings/UnappliedEditsBanner.swift` | новый: баннер неприменившихся правок |
| `apps/macos/Sources/Meetings/SegmentAudioPlayer.swift` | новый: проигрывание фрагмента |

---

### Task 1: `original_text` у сегмента Final

**Files:**
- Modify: `rust/crates/domain/src/postcall.rs:83-103` (`FinalSegment`)
- Modify: `rust/crates/storage/src/audio_manifest.rs:565-610` (`list_final_segments`)
- Test: `rust/crates/storage/src/audio_manifest.rs` (`mod tests`)

**Interfaces:**
- Produces: поле `FinalSegment.original_text: String` — распознанный текст из журнала правок; пусто, когда правки нет.

- [x] **Step 1: Написать падающий тест**

В `rust/crates/storage/src/audio_manifest.rs`, в `mod tests`. Тесты в этом модуле работают через `tmp_root()` и `AudioManifestStore::open` — смотри соседний `list_final_segments_of_unknown_version_is_empty`.

```rust
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
            .upsert_segment_edit(&SegmentEdit {
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
```

Если имя `replace_final_segments` в коде другое — взять фактическое из соседних тестов этого модуля, остальное не менять.

- [x] **Step 2: Прогнать и убедиться, что падает**

Run: `cd rust && cargo test -p meetingraft-storage edited_segment_carries_recognized_text`
Expected: FAIL — компиляция, `FinalSegment` не имеет поля `original_text`.

- [x] **Step 3: Добавить поле в домен**

В `rust/crates/domain/src/postcall.rs`, в `FinalSegment` после `text_edited`:

```rust
    /// Что распознала модель на этом месте (Epic 19).
    ///
    /// Берётся из журнала правок, а не из таблицы сегментов: правка
    /// ищет своё место вхождением (`reattach_edits`), поэтому после
    /// пересбора текст сегмента бывает длиннее сохранённого в правке.
    /// Возврат к исходному сравнивается именно с журналом, и подмена
    /// источника завела бы новую правку вместо удаления старой.
    ///
    /// Пусто, когда правки нет.
    pub original_text: String,
```

- [x] **Step 4: Заполнить при чтении**

В `rust/crates/storage/src/audio_manifest.rs`, в `list_final_segments`: в конструкторе `FinalSegment` внутри `query_map` добавить `original_text: String::new(),`, а в цикле наложения правок:

```rust
        for segment in &mut segments {
            if let Some(edit) = by_position.get(&segment.position()) {
                segment.text = edit.edited_text.clone();
                segment.text_edited = true;
                segment.original_text = edit.original_text.clone();
            }
        }
```

- [x] **Step 5: Починить остальные места конструирования**

Run: `cd rust && cargo build -p meetingraft-storage -p meetingraft-postcall -p meetingraft-ffi`

Везде, где компилятор укажет на отсутствующее поле, добавить `original_text: String::new()`. В `ffi` это в том числе `recognized` внутри `edit_segment_text` — там поле тоже пустое, на разбор оно не влияет.

- [x] **Step 6: Прогнать тесты**

Run: `cd rust && cargo test -p meetingraft-storage -p meetingraft-postcall`
Expected: PASS

- [x] **Step 7: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/domain/src/postcall.rs rust/crates/storage/src/audio_manifest.rs
git commit -m "feat: carry recognized text on edited final segments"
```

---

### Task 2: Поиск подсказки, родившейся из правки

**Files:**
- Modify: `rust/crates/postcall/src/term_from_edit.rs`
- Modify: `rust/crates/postcall/src/lib.rs` (реэкспорт)
- Test: `rust/crates/postcall/src/term_from_edit.rs` (`mod tests`)

**Interfaces:**
- Consumes: `term_from_edit(original: &str, edited: &str) -> Option<(String, String)>` (уже есть в этом файле).
- Produces:

```rust
pub fn promotable_term<'a>(
    original_text: &str,
    edited_text: &str,
    terms: &'a [GlossaryTerm],
    meeting_id: &str,
    language: SpeechLanguage,
) -> Option<&'a GlossaryTerm>
```

Отдаёт подсказку, родившуюся из этой правки и действующую в этой встрече. `None`, если термина нет, он уже замена или принадлежит чужой встрече.

- [x] **Step 1: Написать падающие тесты**

В `rust/crates/postcall/src/term_from_edit.rs`, в `mod tests`:

```rust
use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};

fn term(id: &str, surface: &str, canonical: &str, kind: GlossaryKind, scope: GlossaryScope) -> GlossaryTerm {
    GlossaryTerm {
        id: id.to_string(),
        surface: surface.to_string(),
        canonical: canonical.to_string(),
        language: SpeechLanguage::Ru,
        scope,
        kind,
        updated_at_ms: 0,
    }
}

#[test]
fn hint_born_from_edit_is_promotable() {
    let terms = vec![term(
        "t1",
        "юни-эф-эф-ай",
        "UniFFI",
        GlossaryKind::Hint,
        GlossaryScope::Meeting { meeting_id: "m1".to_string() },
    )];
    let found = promotable_term(
        "упирается в юни-эф-эф-ай",
        "упирается в UniFFI",
        &terms,
        "m1",
        SpeechLanguage::Ru,
    );
    assert_eq!(found.map(|t| t.id.as_str()), Some("t1"));
}

/// Замена уже подтверждена человеком — повышать нечего.
///
/// В данных лежит именно `Replacement`: с подсказкой тест прошёл бы
/// вхолостую и ничего бы не проверил.
#[test]
fn replacement_is_not_promotable() {
    let terms = vec![term(
        "t1",
        "юни-эф-эф-ай",
        "UniFFI",
        GlossaryKind::Replacement,
        GlossaryScope::Global,
    )];
    let found = promotable_term(
        "упирается в юни-эф-эф-ай",
        "упирается в UniFFI",
        &terms,
        "m1",
        SpeechLanguage::Ru,
    );
    assert!(found.is_none());
}

/// Термин чужой встречи предлагать нельзя: замена там не применится.
#[test]
fn term_of_another_meeting_is_not_promotable() {
    let terms = vec![term(
        "t1",
        "юни-эф-эф-ай",
        "UniFFI",
        GlossaryKind::Hint,
        GlossaryScope::Meeting { meeting_id: "m2".to_string() },
    )];
    let found = promotable_term(
        "упирается в юни-эф-эф-ай",
        "упирается в UniFFI",
        &terms,
        "m1",
        SpeechLanguage::Ru,
    );
    assert!(found.is_none());
}

/// Длинная правка термином не становится — значит и повышать нечего.
#[test]
fn long_edit_yields_no_promotable_term() {
    let terms = vec![term(
        "t1",
        "что-то",
        "другое",
        GlossaryKind::Hint,
        GlossaryScope::Global,
    )];
    let found = promotable_term(
        "мы решили это на прошлой неделе целиком",
        "мы договорились обсудить это в следующий понедельник",
        &terms,
        "m1",
        SpeechLanguage::Ru,
    );
    assert!(found.is_none());
}
```

- [x] **Step 2: Прогнать и убедиться, что падает**

Run: `cd rust && cargo test -p meetingraft-postcall promotable`
Expected: FAIL — `promotable_term` не найдена.

- [x] **Step 3: Реализовать**

В конец `rust/crates/postcall/src/term_from_edit.rs`, перед `mod tests`:

```rust
use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};

/// Подсказка, родившаяся из этой правки и действующая в этой встрече.
///
/// Пара берётся тем же разбором, что и при заведении термина
/// (`plan_edit`), и сравнивается без учёта регистра — как в `normalize`.
///
/// Замена не возвращается: она уже подтверждена человеком, повышать
/// нечего. Термин чужой встречи — тоже: замена там не применится, и
/// следующий пересбор её не подтвердит.
pub fn promotable_term<'a>(
    original_text: &str,
    edited_text: &str,
    terms: &'a [GlossaryTerm],
    meeting_id: &str,
    language: SpeechLanguage,
) -> Option<&'a GlossaryTerm> {
    let (surface, canonical) = term_from_edit(original_text, edited_text)?;
    terms.iter().find(|term| {
        term.kind == GlossaryKind::Hint
            && term.language == language
            && term.surface.to_lowercase() == surface.to_lowercase()
            && term.canonical.to_lowercase() == canonical.to_lowercase()
            && match &term.scope {
                GlossaryScope::Global => true,
                GlossaryScope::Meeting { meeting_id: id } => id == meeting_id,
            }
    })
}
```

- [x] **Step 4: Реэкспортировать**

В `rust/crates/postcall/src/lib.rs` добавить `promotable_term` в тот же `pub use`, где уже стоит `term_from_edit`.

- [x] **Step 5: Прогнать тесты**

Run: `cd rust && cargo test -p meetingraft-postcall`
Expected: PASS

- [x] **Step 6: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/postcall/src
git commit -m "feat: find the hint term a segment edit produced"
```

---

### Task 3: Граница FFI

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs:125-140` (`FfiFinalSegment`)
- Modify: `rust/crates/ffi/src/lib.rs:1353-1382` (`list_final_segments`)
- Modify: `rust/crates/ffi/src/lib.rs` (новый метод рядом с `list_unapplied_edits:1593`)
- Test: `rust/crates/ffi/src/lib.rs` (`mod tests`)

**Interfaces:**
- Consumes: `FinalSegment.original_text` (Task 1), `postcall::promotable_term` (Task 2).
- Produces: `FfiFinalSegment.original_text: String`, `FfiFinalSegment.promotable_term_id: String`, метод `delete_segment_edit(edit_id: String) -> String`.

- [x] **Step 1: Написать падающий тест на удаление правки**

В `mod tests` файла `rust/crates/ffi/src/lib.rs`, рядом с существующими тестами правок:

```rust
/// Удаление снимает только названную правку.
///
/// Вторая правка в данных обязательна: без неё тест не отличит
/// «удалил нужную» от «вычистил журнал».
#[test]
fn delete_segment_edit_removes_only_named_one() {
    let core = MeetingCore::with_data_root(tmp_root_string());
    core.start_recording("s1".to_string(), "Встреча".to_string());
    core.stop_recording();

    seed_two_edits(&core, "s1");
    let before = core.list_unapplied_edits("s1".to_string());
    assert_eq!(before.len(), 2, "подготовка: две неприменившиеся правки");

    let error = core.delete_segment_edit(before[0].id.clone());
    assert!(error.is_empty(), "удаление: {error}");

    let after = core.list_unapplied_edits("s1".to_string());
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[1].id);
}
```

`tmp_root_string` и `seed_two_edits` — вспомогательные из этого же `mod tests`; если их нет, написать по образцу соседнего теста журнала правок, заведя две правки с `applied_version: None` и разными `id`.

- [x] **Step 2: Прогнать и убедиться, что падает**

Run: `cd rust && cargo test -p meetingraft-ffi delete_segment_edit_removes_only_named_one`
Expected: FAIL — метода `delete_segment_edit` нет.

- [x] **Step 3: Добавить метод**

В `rust/crates/ffi/src/lib.rs`, внутри `#[uniffi::export] impl MeetingCore`, сразу после `list_unapplied_edits`:

```rust
    /// Снять правку из журнала. Пустая строка — успех.
    ///
    /// Нужен неприменившимся правкам: показать их и не дать убрать —
    /// значит оставить человеку раздел, который никогда не опустеет.
    pub fn delete_segment_edit(&self, edit_id: String) -> String {
        let Some(mut store) = open_store(self) else {
            return "storage unavailable".to_string();
        };
        match store.delete_segment_edit(&edit_id) {
            Ok(()) => String::new(),
            Err(error) => error.to_string(),
        }
    }
```

- [x] **Step 4: Прогнать тест удаления**

Run: `cd rust && cargo test -p meetingraft-ffi delete_segment_edit_removes_only_named_one`
Expected: PASS

- [x] **Step 5: Написать падающий тест на два поля**

```rust
/// Правленый сегмент отдаёт распознанное и id подсказки для повышения.
#[test]
fn edited_segment_exposes_original_text_and_promotable_term() {
    let core = MeetingCore::with_data_root(tmp_root_string());
    core.start_recording("s1".to_string(), "Встреча".to_string());
    core.stop_recording();
    seed_final_segment(&core, "s1", 1, "упирается в юни-эф-эф-ай");

    let error = core.edit_segment_text(
        "s1".to_string(),
        1,
        0,
        "упирается в UniFFI".to_string(),
    );
    assert!(error.is_empty(), "правка: {error}");

    let segments = core.list_final_segments("s1".to_string(), 1);
    assert_eq!(segments[0].text, "упирается в UniFFI");
    assert_eq!(segments[0].original_text, "упирается в юни-эф-эф-ай");
    assert!(
        !segments[0].promotable_term_id.is_empty(),
        "из короткой правки родилась подсказка — её и повышаем"
    );
}

/// Неправленый сегмент не предлагает ни исходного текста, ни повышения.
#[test]
fn untouched_segment_exposes_neither_field() {
    let core = MeetingCore::with_data_root(tmp_root_string());
    core.start_recording("s1".to_string(), "Встреча".to_string());
    core.stop_recording();
    seed_final_segment(&core, "s1", 1, "обычная реплика");

    let segments = core.list_final_segments("s1".to_string(), 1);
    assert!(segments[0].original_text.is_empty());
    assert!(segments[0].promotable_term_id.is_empty());
}
```

`seed_final_segment` — вспомогательная из `mod tests`; если её нет, написать по образцу соседних тестов, кладущих сегменты версии через хранилище.

- [x] **Step 6: Прогнать и убедиться, что падает**

Run: `cd rust && cargo test -p meetingraft-ffi edited_segment_exposes`
Expected: FAIL — компиляция, полей нет.

- [x] **Step 7: Добавить поля в запись**

В `FfiFinalSegment` после `text_edited`:

```rust
    /// Что распознала модель; пусто, когда правки нет (Epic 19).
    pub original_text: String,
    /// id подсказки, родившейся из этой правки: кнопка «заменять
    /// всюду» показывается ровно когда поле непустое.
    ///
    /// Пусто, если термина нет, он уже замена или принадлежит чужой
    /// встрече. Решение принимает Rust: в Swift не должно уезжать ни
    /// знание про виды записи глоссария, ни разбор диффа.
    pub promotable_term_id: String,
```

- [x] **Step 8: Заполнить в `list_final_segments`**

Заменить тело `list_final_segments` на:

```rust
    pub fn list_final_segments(&self, meeting_id: String, version: u32) -> Vec<FfiFinalSegment> {
        let Some(store) = open_store(self) else {
            return Vec::new();
        };
        let segments = store
            .list_final_segments(&meeting_id, version)
            .unwrap_or_default();
        let speakers = store.list_speakers(&meeting_id).unwrap_or_default();
        // Словарь читается один раз на весь список: подсказка ищется для
        // каждого правленого сегмента, а чтение на строку превратило бы
        // открытие транскрипта в сотни запросов.
        let terms = store.list_glossary_terms().unwrap_or_default();
        let language = {
            let guard = self.inner.lock().expect("meeting core poisoned");
            guard.language_policy.primary
        };
        segments
            .into_iter()
            .map(|segment| {
                let speaker_name = speakers
                    .iter()
                    .find(|speaker| speaker.id == segment.speaker_id)
                    .map(|speaker| speaker.display_name.clone())
                    .unwrap_or_default();
                let promotable_term_id = if segment.text_edited {
                    promotable_term(
                        &segment.original_text,
                        &segment.text,
                        &terms,
                        &meeting_id,
                        language,
                    )
                    .map(|term| term.id.clone())
                    .unwrap_or_default()
                } else {
                    String::new()
                };
                FfiFinalSegment {
                    index: segment.index,
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    channel: segment.channel.code().to_string(),
                    speaker_id: segment.speaker_id,
                    speaker_name,
                    speaker_pinned: segment.speaker_pinned,
                    text: segment.text,
                    text_edited: segment.text_edited,
                    original_text: segment.original_text,
                    promotable_term_id,
                }
            })
            .collect()
    }
```

Добавить `promotable_term` в `use` из `postcall` в шапке файла.

- [x] **Step 9: Прогнать тесты крейта**

Run: `cd rust && cargo test -p meetingraft-ffi`
Expected: PASS. Соседние тесты, конструирующие `FfiFinalSegment`, придётся дополнить двумя полями.

- [x] **Step 10: Коммит**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add rust/crates/ffi/src/lib.rs
git commit -m "feat: expose recognized text, promotable term and edit deletion"
```

---

### Task 4: Контракт ядра и состояние правки в Swift

**Files:**
- Modify: `apps/macos/Sources/Meetings/SpeakerAttributionViewModel.swift:5-24` (протокол), тело модели, `reload()`
- Test: `apps/macos/Tests/SpeakerAttributionViewModelTests.swift`

**Interfaces:**
- Consumes: сгенерированные биндинги из Task 3 — `editSegmentText`, `listUnappliedEdits`, `deleteSegmentEdit`, `promoteTermToReplacement`, `segmentAudio`, поля `originalText` и `promotableTermId` у `FfiFinalSegment`.
- Produces: `editingIndex: UInt32?`, `draftText: String`, `unappliedEdits: [FfiSegmentEdit]`, `canPromote(index:) -> Bool`, `audioFragment(for:) -> FfiAudioFragment`, методы `beginEdit(index:)`, `cancelEdit()`, `commitEdit()`, `revertToOriginal(index:)`, `promoteTerm(index:)`, `dismissUnapplied(id:)`.

- [x] **Step 1: Расширить протокол**

В `SpeakerAttributionCoreProviding` (`SpeakerAttributionViewModel.swift:5`) после `unpinSegmentSpeaker`:

```swift
    func editSegmentText(meetingId: String, version: UInt32, index: UInt32, text: String) -> String
    func listUnappliedEdits(meetingId: String) -> [FfiSegmentEdit]
    func deleteSegmentEdit(editId: String) -> String
    func promoteTermToReplacement(termId: String, meetingId: String, version: UInt32) -> String
    func segmentAudio(
        meetingId: String,
        channelCode: String,
        startMs: UInt64,
        endMs: UInt64
    ) -> FfiAudioFragment
```

`extension MeetingCore: SpeakerAttributionCoreProviding {}` (строка 26) подхватит их без изменений.

- [x] **Step 2: Дополнить подделку в тестах**

В `apps/macos/Tests/SpeakerAttributionViewModelTests.swift`, в `AttributionCoreSpy` — свойства записи вызовов и заглушки:

```swift
    // MARK: - Правка (Epic 19)
    var segmentsOverride: [FfiFinalSegment] = []
    var unapplied: [FfiSegmentEdit] = []
    var editError = ""
    private(set) var editedTexts: [String] = []
    private(set) var deletedEditIds: [String] = []
    private(set) var promotedTermIds: [String] = []

    func editSegmentText(meetingId: String, version: UInt32, index: UInt32, text: String) -> String {
        editedTexts.append(text)
        return editError
    }

    func listUnappliedEdits(meetingId: String) -> [FfiSegmentEdit] { unapplied }

    func deleteSegmentEdit(editId: String) -> String {
        deletedEditIds.append(editId)
        return ""
    }

    func promoteTermToReplacement(termId: String, meetingId: String, version: UInt32) -> String {
        promotedTermIds.append(termId)
        return ""
    }

    func segmentAudio(
        meetingId: String,
        channelCode: String,
        startMs: UInt64,
        endMs: UInt64
    ) -> FfiAudioFragment {
        FfiAudioFragment(pcm: Data(), sampleRate: 0, durationMs: 0)
    }
```

Существующий `listFinalSegments` в подделке должен возвращать `segmentsOverride`.

Вспомогательные конструкторы в конец файла:

```swift
private func segment(
    _ index: UInt32,
    text: String,
    originalText: String = "",
    promotableTermId: String = "",
    channel: String = "mic"
) -> FfiFinalSegment {
    FfiFinalSegment(
        index: index,
        startMs: UInt64(index) * 1_000,
        endMs: UInt64(index) * 1_000 + 900,
        channel: channel,
        speakerId: "",
        speakerName: "",
        speakerPinned: false,
        text: text,
        textEdited: !originalText.isEmpty,
        originalText: originalText,
        promotableTermId: promotableTermId
    )
}

private func edit(_ id: String) -> FfiSegmentEdit {
    FfiSegmentEdit(
        id: id,
        channel: "mic",
        startMs: 0,
        endMs: 900,
        originalText: "юни-эф-эф-ай",
        editedText: "UniFFI"
    )
}
```

- [x] **Step 3: Написать падающие тесты**

В тот же файл, новой секцией `// MARK: - Правка текста`:

```swift
/// Esc не трогает ядро: иначе отказ от правки её бы и сохранял.
func testCancelEditDoesNotCallCore() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [segment(0, text: "упирается в UniFFI",
                                     originalText: "упирается в юни-эф-эф-ай")]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.beginEdit(index: 0)
    viewModel.draftText = "совсем другое"
    viewModel.cancelEdit()

    XCTAssertTrue(core.editedTexts.isEmpty)
    XCTAssertNil(viewModel.editingIndex)
}

/// Сохранение отдаёт ядру ровно введённое.
func testCommitEditSendsDraftToCore() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [segment(0, text: "упирается в юни-эф-эф-ай")]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.beginEdit(index: 0)
    viewModel.draftText = "упирается в UniFFI"
    viewModel.commitEdit()

    XCTAssertEqual(core.editedTexts, ["упирается в UniFFI"])
    XCTAssertNil(viewModel.editingIndex)
}

/// Возврат к исходному отправляет распознанное — ядро само удалит
/// правку из журнала. Тексты в данных заведомо разные: совпади они,
/// тест прошёл бы, ничего не проверив.
func testRevertSendsRecognizedText() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [segment(0, text: "упирается в UniFFI",
                                     originalText: "упирается в юни-эф-эф-ай")]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.revertToOriginal(index: 0)

    XCTAssertEqual(core.editedTexts, ["упирается в юни-эф-эф-ай"])
}

/// Неправленый сегмент возвращать не к чему — ядро не дёргаем.
func testRevertOnUntouchedSegmentDoesNothing() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [segment(0, text: "обычная реплика")]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.revertToOriginal(index: 0)

    XCTAssertTrue(core.editedTexts.isEmpty)
}

/// Ошибка ядра видна, а не проглочена.
func testCommitEditSurfacesCoreError() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [segment(0, text: "реплика")]
    core.editError = "сегмент 0 не найден"
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.beginEdit(index: 0)
    viewModel.draftText = "другое"
    viewModel.commitEdit()

    XCTAssertEqual(viewModel.errorMessage, "сегмент 0 не найден")
}

/// «Заменять всюду» предлагается ровно когда ядро дало id подсказки.
func testCanPromoteFollowsCoreDecision() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [
        segment(0, text: "упирается в UniFFI",
                originalText: "упирается в юни-эф-эф-ай", promotableTermId: "t1"),
        segment(1, text: "правленое, но термина нет",
                originalText: "распознанное длиннее трёх слов совсем"),
    ]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    XCTAssertTrue(viewModel.canPromote(index: 0))
    XCTAssertFalse(viewModel.canPromote(index: 1), "пустой id — кнопки быть не должно")
}

/// Повышение уходит в ядро с тем id, что оно само и дало.
func testPromoteTermSendsCoreProvidedId() {
    let core = AttributionCoreSpy(speakers: [])
    core.segmentsOverride = [segment(0, text: "упирается в UniFFI",
                                     originalText: "упирается в юни-эф-эф-ай",
                                     promotableTermId: "t1")]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.promoteTerm(index: 0)

    XCTAssertEqual(core.promotedTermIds, ["t1"])
}
```

- [ ] **Step 4: Прогнать и убедиться, что падает**

Run на Mac: `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -only-testing:MeetingRaftTests/SpeakerAttributionViewModelTests test CODE_SIGNING_ALLOWED=NO`
Expected: FAIL — компиляция, у модели нет `beginEdit` и остальных.

- [x] **Step 5: Реализовать состояние в модели**

В `SpeakerAttributionViewModel` после `private(set) var errorMessage: String?`:

```swift
    /// Индекс правящейся реплики; `nil` — никто не правится.
    private(set) var editingIndex: UInt32?
    /// Черновик правки живёт в модели, а не во вью: список
    /// перерисовывается на каждое обновление, и набранное терялось бы.
    var draftText = ""
    /// Правки, не легшие ни на одну версию после пересбора.
    private(set) var unappliedEdits: [FfiSegmentEdit] = []
```

Методы — в конец класса, перед `private func reload()`:

```swift
    /// Предлагать ли «заменять всюду» для этой реплики.
    ///
    /// Решает ядро: непустой `promotableTermId` означает, что подсказка
    /// родилась из правки, действует в этой встрече и ещё не стала
    /// заменой. Повторять этот разбор в Swift нельзя (`AGENTS.md`).
    func canPromote(index: UInt32) -> Bool {
        guard let segment = segments.first(where: { $0.index == index }) else { return false }
        return !segment.promotableTermId.isEmpty
    }

    func beginEdit(index: UInt32) {
        guard let segment = segments.first(where: { $0.index == index }) else { return }
        editingIndex = index
        draftText = segment.text
    }

    /// Esc: ядро не трогаем — от правки отказались.
    func cancelEdit() {
        editingIndex = nil
        draftText = ""
    }

    /// Enter или потеря фокуса.
    ///
    /// Состояние сбрасывается до вызова ядра: `finish` перечитывает
    /// сегменты, и оставленный индекс открыл бы поле заново поверх уже
    /// сохранённого текста.
    func commitEdit() {
        guard let index = editingIndex, let version else {
            cancelEdit()
            return
        }
        let text = draftText
        editingIndex = nil
        draftText = ""
        finish(error: core.editSegmentText(
            meetingId: meetingId,
            version: version,
            index: index,
            text: text
        ))
    }

    /// Вернуть распознанное. Это отмена, а не ещё одна правка: получив
    /// исходный текст обратно, ядро удаляет запись из журнала.
    func revertToOriginal(index: UInt32) {
        guard let version,
              let segment = segments.first(where: { $0.index == index }),
              !segment.originalText.isEmpty
        else { return }
        finish(error: core.editSegmentText(
            meetingId: meetingId,
            version: version,
            index: index,
            text: segment.originalText
        ))
    }

    func promoteTerm(index: UInt32) {
        guard let version,
              let segment = segments.first(where: { $0.index == index }),
              !segment.promotableTermId.isEmpty
        else { return }
        finish(error: core.promoteTermToReplacement(
            termId: segment.promotableTermId,
            meetingId: meetingId,
            version: version
        ))
    }

    func dismissUnapplied(id: String) {
        finish(error: core.deleteSegmentEdit(editId: id))
    }

    /// Звук реплики. Пустой фрагмент (`sampleRate == 0`) означает, что
    /// записи за диапазон нет, — вью на это прячет кнопку.
    func audioFragment(for segment: FfiFinalSegment) -> FfiAudioFragment {
        core.segmentAudio(
            meetingId: meetingId,
            channelCode: segment.channel,
            startMs: segment.startMs,
            endMs: segment.endMs
        )
    }
```

- [x] **Step 6: Тянуть неприменившиеся правки при перечитывании**

В `reload()`, сразу после `speakers = core.listSpeakers(meetingId: meetingId)`:

```swift
        unappliedEdits = core.listUnappliedEdits(meetingId: meetingId)
```

Именно здесь, а не под `guard let version`: правка без версии — как раз та, которую надо показать.

- [ ] **Step 7: Прогнать тесты**

Run на Mac: `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -only-testing:MeetingRaftTests/SpeakerAttributionViewModelTests test CODE_SIGNING_ALLOWED=NO`
Expected: PASS

- [x] **Step 8: Коммит**

```bash
cd apps/macos && swiftformat Sources Tests --lint
git add apps/macos/Sources/Meetings/SpeakerAttributionViewModel.swift apps/macos/Tests/SpeakerAttributionViewModelTests.swift
git commit -m "feat: add segment edit state to the attribution model"
```

---

### Task 5: Правка в строке

**Files:**
- Modify: `apps/macos/Sources/Meetings/FinalSegmentsView.swift`

**Interfaces:**
- Consumes: `editingIndex`, `draftText`, `canPromote(index:)`, `beginEdit`, `cancelEdit`, `commitEdit`, `revertToOriginal`, `promoteTerm` (Task 4); поля `originalText`, `promotableTermId`, `textEdited` (Task 3).

- [x] **Step 1: Передать состояние правки в строку**

В `FinalSegmentsView` заменить содержимое `List` на:

```swift
    var body: some View {
        List(viewModel.segments, id: \.index) { segment in
            FinalSegmentRow(
                segment: segment,
                speakers: viewModel.speakers,
                isEditing: viewModel.editingIndex == segment.index,
                canPromote: viewModel.canPromote(index: segment.index),
                draft: Bindable(viewModel).draftText,
                onAssign: { viewModel.assignSegment(index: segment.index, to: $0) },
                onUnpin: { viewModel.unpinSegment(index: segment.index) },
                onBeginEdit: { viewModel.beginEdit(index: segment.index) },
                onCommitEdit: { viewModel.commitEdit() },
                onCancelEdit: { viewModel.cancelEdit() },
                onRevert: { viewModel.revertToOriginal(index: segment.index) },
                onPromote: { viewModel.promoteTerm(index: segment.index) }
            )
            .listRowSeparator(.hidden)
        }
    }
```

И объявить новые параметры в `FinalSegmentRow`:

```swift
    let isEditing: Bool
    let canPromote: Bool
    @Binding var draft: String
    let onBeginEdit: () -> Void
    let onCommitEdit: () -> Void
    let onCancelEdit: () -> Void
    let onRevert: () -> Void
    let onPromote: () -> Void
    @FocusState private var isFieldFocused: Bool
    @State private var isConfirmingPromote = false
```

- [x] **Step 2: Раскрыть строку в поле**

В `FinalSegmentRow.body` заменить `Text(segment.text)` на:

```swift
                if isEditing {
                    TextField("", text: $draft, axis: .vertical)
                        .textFieldStyle(.plain)
                        .font(Theme.Text.body)
                        .foregroundStyle(Theme.textPrimary)
                        .padding(Theme.Space.xs)
                        .background(Theme.surfaceElevated, in: RoundedRectangle(cornerRadius: Theme.Radius.sm))
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.Radius.sm)
                                .stroke(Theme.accent, lineWidth: 1)
                        )
                        .focused($isFieldFocused)
                        .onAppear { isFieldFocused = true }
                        // Enter сохраняет, Esc откатывает, уход фокуса
                        // сохраняет: набранное не должно теряться от
                        // клика мимо поля.
                        .onSubmit(onCommitEdit)
                        .onExitCommand(perform: onCancelEdit)
                        .onChange(of: isFieldFocused) { _, focused in
                            if !focused, isEditing { onCommitEdit() }
                        }
                    editingBar
                } else {
                    Text(segment.text)
                        .font(Theme.Text.body)
                        .foregroundStyle(Theme.textPrimary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .contentShape(Rectangle())
                        .onTapGesture(perform: onBeginEdit)
                }
                if segment.textEdited, !isEditing {
                    Text("было: \(segment.originalText)")
                        .font(Theme.Text.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
```

- [x] **Step 3: Полоска действий под полем**

Новое свойство в `FinalSegmentRow`. Кнопка воспроизведения появится в Task 7 — пока полоска несёт две кнопки:

```swift
    /// Действия видны только пока строка правится: постоянный ряд кнопок
    /// на каждой реплике превратил бы транскрипт в панель управления.
    private var editingBar: some View {
        HStack(spacing: Theme.Space.sm) {
            if !segment.originalText.isEmpty {
                Button("Вернуть исходное", action: onRevert)
                    .buttonStyle(.themedSecondary)
            }
            if canPromote {
                Button("Заменять всюду") { isConfirmingPromote = true }
                    .buttonStyle(.themedSecondary)
            }
            Spacer()
        }
        .padding(.top, Theme.Space.xxs)
        .confirmationDialog(
            "Заменять всюду в этой встрече?",
            isPresented: $isConfirmingPromote,
            titleVisibility: .visible
        ) {
            Button("Заменять всюду", role: .destructive, action: onPromote)
            Button("Отмена", role: .cancel) {}
        } message: {
            Text(
                "Все совпадения в этой встрече будут заменены. "
                    + "Каждая изменённая реплика получит пометку вашей правки, "
                    + "и отменить их можно будет только по одной."
            )
        }
    }
```

Числа затронутых реплик не показываем: сухого прогона на границе нет, а формулировка не даёт нажать вслепую.

- [x] **Step 4: Пометить правленый текст отдельно от правленого спикера**

В `speakerMenu` рядом с существующим `Chip(text: "правка")` для `segment.speakerPinned`:

```swift
            // Две пометки различаются словом: «правка» уже занята
            // ручным назначением спикера, и одинаковые чипы рядом были
            // бы неразличимы.
            if segment.textEdited {
                Chip(text: "текст")
            }
```

- [ ] **Step 5: Собрать и прогнать**

Run на Mac: `scripts/verify-mac.sh`
Expected: сборка и тесты зелёные.

- [x] **Step 6: Коммит**

```bash
git add apps/macos/Sources/Meetings/FinalSegmentsView.swift
git commit -m "feat: edit segment text in place"
```

---

### Task 6: Баннер неприменившихся правок

**Files:**
- Create: `apps/macos/Sources/Meetings/UnappliedEditsBanner.swift`
- Modify: `apps/macos/Sources/Meetings/FinalSegmentsView.swift`
- Test: `apps/macos/Tests/SpeakerAttributionViewModelTests.swift`

**Interfaces:**
- Consumes: `unappliedEdits`, `dismissUnapplied(id:)` (Task 4).

- [x] **Step 1: Написать падающие тесты**

```swift
// MARK: - Неприменившиеся правки

/// Пустой список — пустая плашка, а заглушек в интерфейсе быть не должно.
func testNoUnappliedEditsMeansNothingToShow() {
    let core = AttributionCoreSpy(speakers: [])
    core.unapplied = []
    let viewModel = SpeakerAttributionViewModel(core: core)

    viewModel.load(meetingId: "m1", version: 1)

    XCTAssertTrue(viewModel.unappliedEdits.isEmpty)
}

/// Правки без версии показываются и тогда, когда версии Final нет
/// вовсе: именно в этом случае их больше негде увидеть.
func testUnappliedEditsLoadWithoutVersion() {
    let core = AttributionCoreSpy(speakers: [])
    core.unapplied = [edit("e1")]
    let viewModel = SpeakerAttributionViewModel(core: core)

    viewModel.load(meetingId: "m1", version: nil)

    XCTAssertEqual(viewModel.unappliedEdits.map(\.id), ["e1"])
}

/// Снятие правки уходит в ядро именно тем id, по которому нажали.
func testDismissUnappliedCallsCoreWithThatId() {
    let core = AttributionCoreSpy(speakers: [])
    core.unapplied = [edit("e1"), edit("e2")]
    let viewModel = SpeakerAttributionViewModel(core: core)
    viewModel.load(meetingId: "m1", version: 1)

    viewModel.dismissUnapplied(id: "e2")

    XCTAssertEqual(core.deletedEditIds, ["e2"])
}
```

- [ ] **Step 2: Прогнать и убедиться, что падает**

Run на Mac: `xcodebuild … -only-testing:MeetingRaftTests/SpeakerAttributionViewModelTests test CODE_SIGNING_ALLOWED=NO`
Expected: FAIL — `unappliedEdits` пуст либо метода нет.

- [x] **Step 3: Написать вью баннера**

Создать `apps/macos/Sources/Meetings/UnappliedEditsBanner.swift`:

```swift
import AppKit
import SwiftUI

/// Правки, не легшие ни на одну версию после пересбора.
///
/// Стоит над списком и появляется только когда есть что показать:
/// постоянный раздел был бы пустым почти всегда. Прятать это в конец
/// списка нельзя — сообщение означает «часть вашей ручной работы
/// отвалилась», и оно должно попадаться на глаза само.
struct UnappliedEditsBanner: View {
    let edits: [FfiSegmentEdit]
    let onCopy: (FfiSegmentEdit) -> Void
    let onDismiss: (FfiSegmentEdit) -> Void

    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Button {
                isExpanded.toggle()
            } label: {
                HStack(spacing: Theme.Space.xs) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(Theme.warning)
                    Text(title)
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textPrimary)
                    Spacer()
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            .buttonStyle(.plain)

            if isExpanded {
                ForEach(edits, id: \.id) { edit in
                    card(edit)
                }
            }
        }
        .padding(Theme.Space.sm)
        .background(Theme.warning.opacity(0.10), in: RoundedRectangle(cornerRadius: Theme.Radius.md))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.md)
                .stroke(Theme.warning.opacity(0.45), lineWidth: 1)
        )
    }

    private var title: String {
        edits.count == 1
            ? "1 правка не легла на текущую версию"
            : "\(edits.count) правок не легли на текущую версию"
    }

    private func card(_ edit: FfiSegmentEdit) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            HStack(spacing: Theme.Space.xs) {
                Text(SpeakerFormat.timecode(ms: edit.startMs))
                    .font(Theme.Text.mono())
                Text(SpeakerFormat.channelLabel(edit.channel))
                    .font(Theme.Text.caption)
            }
            .foregroundStyle(Theme.textTertiary)

            Text("было: \(edit.originalText)")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textTertiary)
            Text("стало: \(edit.editedText)")
                .font(Theme.Text.bodySmall)
                .foregroundStyle(Theme.textPrimary)

            HStack(spacing: Theme.Space.xs) {
                // Перенести правку на место нельзя: `originalText` служит
                // и поиском при пересборе, и признаком возврата к
                // исходному. Поэтому копируем текст, а правится нужный
                // сегмент обычным путём.
                Button("Скопировать текст") { onCopy(edit) }
                    .buttonStyle(.themedSecondary)
                Button("Удалить правку") { onDismiss(edit) }
                    .buttonStyle(.themedSecondary)
                Spacer()
            }
            .padding(.top, Theme.Space.xxs)
        }
        .padding(.vertical, Theme.Space.xs)
    }
}
```

- [x] **Step 4: Вставить над списком**

В `FinalSegmentsView` обернуть `List`:

```swift
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.sm) {
            if !viewModel.unappliedEdits.isEmpty {
                UnappliedEditsBanner(
                    edits: viewModel.unappliedEdits,
                    onCopy: { edit in
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(edit.editedText, forType: .string)
                    },
                    onDismiss: { viewModel.dismissUnapplied(id: $0.id) }
                )
                .padding(.horizontal, Theme.Space.sm)
            }
            List(viewModel.segments, id: \.index) { segment in
                // …тело строки из Task 5 без изменений
            }
        }
    }
```

- [ ] **Step 5: Прогнать**

Run на Mac: `scripts/verify-mac.sh`
Expected: PASS

- [x] **Step 6: Коммит**

```bash
git add apps/macos/Sources/Meetings/UnappliedEditsBanner.swift apps/macos/Sources/Meetings/FinalSegmentsView.swift apps/macos/Tests/SpeakerAttributionViewModelTests.swift
git commit -m "feat: show edits that no longer land on a version"
```

---

### Task 7: Проигрывание фрагмента

**Files:**
- Create: `apps/macos/Sources/Meetings/SegmentAudioPlayer.swift`
- Create: `apps/macos/Tests/SegmentAudioPlayerTests.swift`
- Modify: `apps/macos/Sources/Meetings/FinalSegmentsView.swift`

**Interfaces:**
- Consumes: `audioFragment(for:)` (Task 4).
- Produces: `SegmentAudioPlayer.buffer(from:) -> AVAudioPCMBuffer?`, `play(fragment:)`, `stop()`, `isPlaying`.

- [x] **Step 1: Написать падающие тесты**

Разбор байтов — чистая функция и проверяется без звуковой карты. Создать `apps/macos/Tests/SegmentAudioPlayerTests.swift`:

```swift
@testable import MeetingRaft
import AVFoundation
import XCTest

@MainActor
final class SegmentAudioPlayerTests: XCTestCase {
    /// i16 little-endian разбирается в кадры без сдвига и потерь.
    func testBufferKeepsFrameCountAndScale() {
        let samples: [Int16] = [0, 1, -1, 32_767]
        var bytes: [UInt8] = []
        for sample in samples {
            bytes.append(UInt8(truncatingIfNeeded: sample))
            bytes.append(UInt8(truncatingIfNeeded: sample >> 8))
        }
        let fragment = FfiAudioFragment(pcm: Data(bytes), sampleRate: 16_000, durationMs: 1)

        let buffer = SegmentAudioPlayer.buffer(from: fragment)

        XCTAssertEqual(buffer?.frameLength, 4)
        XCTAssertEqual(buffer?.format.sampleRate, 16_000)
        XCTAssertEqual(buffer?.floatChannelData?[0][3] ?? 0, 1.0, accuracy: 0.001)
        XCTAssertEqual(buffer?.floatChannelData?[0][2] ?? 0, -0.00003, accuracy: 0.0001)
    }

    /// `sampleRate == 0` — ответ ядра «записи здесь нет», а не сбой.
    /// Кнопка воспроизведения на это не показывается вовсе.
    func testEmptyFragmentYieldsNoBuffer() {
        let fragment = FfiAudioFragment(pcm: Data(), sampleRate: 0, durationMs: 0)
        XCTAssertNil(SegmentAudioPlayer.buffer(from: fragment))
    }

    /// Обрезанный хвост не должен ронять разбор.
    func testOddByteCountIsTruncatedNotCrashed() {
        let fragment = FfiAudioFragment(pcm: Data([0, 0, 5]), sampleRate: 16_000, durationMs: 1)
        XCTAssertEqual(SegmentAudioPlayer.buffer(from: fragment)?.frameLength, 1)
    }
}
```

- [ ] **Step 2: Прогнать и убедиться, что падает**

Run на Mac: `xcodebuild … -only-testing:MeetingRaftTests/SegmentAudioPlayerTests test CODE_SIGNING_ALLOWED=NO`
Expected: FAIL — типа `SegmentAudioPlayer` нет.

- [x] **Step 3: Написать проигрыватель**

Создать `apps/macos/Sources/Meetings/SegmentAudioPlayer.swift`:

```swift
import AVFoundation
import Observation

/// Проигрывание фрагмента реплики.
///
/// Отдельный объект, а не метод presentation model: держит
/// `AVAudioEngine`, который обязан пережить перерисовку списка. Внутри
/// модели движок дёргался бы на каждое обновление строки.
@Observable
@MainActor
final class SegmentAudioPlayer {
    private(set) var isPlaying = false

    private let engine = AVAudioEngine()
    private let node = AVAudioPlayerNode()
    private var isAttached = false

    /// `nil`, когда играть нечего: `sampleRate == 0` — это ответ ядра
    /// «записи за диапазон нет», а не сбой.
    static func buffer(from fragment: FfiAudioFragment) -> AVAudioPCMBuffer? {
        guard fragment.sampleRate > 0 else { return nil }
        let frames = fragment.pcm.count / 2
        guard frames > 0,
              let format = AVAudioFormat(
                  commonFormat: .pcmFormatFloat32,
                  sampleRate: Double(fragment.sampleRate),
                  channels: 1,
                  interleaved: false
              ),
              let buffer = AVAudioPCMBuffer(
                  pcmFormat: format,
                  frameCapacity: AVAudioFrameCount(frames)
              )
        else { return nil }

        buffer.frameLength = AVAudioFrameCount(frames)
        guard let target = buffer.floatChannelData?[0] else { return nil }
        fragment.pcm.withUnsafeBytes { raw in
            for index in 0 ..< frames {
                let low = UInt16(raw[index * 2])
                let high = UInt16(raw[index * 2 + 1])
                let sample = Int16(bitPattern: low | (high << 8))
                target[index] = Float(sample) / Float(Int16.max)
            }
        }
        return buffer
    }

    func play(fragment: FfiAudioFragment) {
        guard let buffer = Self.buffer(from: fragment) else { return }
        stop()
        if !isAttached {
            engine.attach(node)
            isAttached = true
        }
        engine.connect(node, to: engine.mainMixerNode, format: buffer.format)
        do {
            try engine.start()
        } catch {
            return
        }
        isPlaying = true
        node.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { [weak self] _ in
            Task { @MainActor in self?.stop() }
        }
        node.play()
    }

    func stop() {
        if node.isPlaying { node.stop() }
        if engine.isRunning { engine.stop() }
        isPlaying = false
    }
}
```

- [ ] **Step 4: Прогнать тесты**

Run на Mac: `xcodebuild … -only-testing:MeetingRaftTests/SegmentAudioPlayerTests test CODE_SIGNING_ALLOWED=NO`
Expected: PASS

- [x] **Step 5: Подключить к строке**

В `FinalSegmentsView` завести проигрыватель и передать фрагмент в строку:

```swift
    @State private var player = SegmentAudioPlayer()
```

В вызов `FinalSegmentRow` добавить:

```swift
                fragment: viewModel.audioFragment(for: segment),
                isPlaying: player.isPlaying,
                onPlay: { player.play(fragment: viewModel.audioFragment(for: segment)) },
                onStopPlayback: { player.stop() },
```

В `FinalSegmentRow` — соответствующие свойства и кнопка первой в `editingBar`:

```swift
            // Записи за диапазон может не быть — тогда кнопки нет вовсе.
            // Показать нерабочую значило бы поставить в интерфейс
            // заглушку.
            if SegmentAudioPlayer.buffer(from: fragment) != nil {
                Button(isPlaying ? "Стоп" : "▶ Прослушать") {
                    isPlaying ? onStopPlayback() : onPlay()
                }
                .buttonStyle(.themedSecondary)
            }
```

- [x] **Step 6: Подключить к баннеру**

У неприменившейся правки сегмента нет, но место (канал и границы) есть — значит звук ей доступен тем же путём. В `SpeakerAttributionViewModel` рядом с `audioFragment(for:)`:

```swift
    /// Звук неприменившейся правки: сегмента у неё нет, а место есть.
    func audioFragment(channelCode: String, startMs: UInt64, endMs: UInt64) -> FfiAudioFragment {
        core.segmentAudio(
            meetingId: meetingId,
            channelCode: channelCode,
            startMs: startMs,
            endMs: endMs
        )
    }
```

В `UnappliedEditsBanner` добавить свойство `let onPlay: (FfiSegmentEdit) -> Void` и кнопку первой в ряду карточки:

```swift
                Button("▶ Прослушать") { onPlay(edit) }
                    .buttonStyle(.themedSecondary)
```

В `FinalSegmentsView` передать в баннер:

```swift
                    onPlay: { edit in
                        player.play(fragment: viewModel.audioFragment(
                            channelCode: edit.channel,
                            startMs: edit.startMs,
                            endMs: edit.endMs
                        ))
                    },
```

- [x] **Step 7: Полная проверка и коммит**

```bash
scripts/verify-mac.sh
git add apps/macos/Sources/Meetings/SegmentAudioPlayer.swift apps/macos/Tests/SegmentAudioPlayerTests.swift apps/macos/Sources/Meetings/FinalSegmentsView.swift apps/macos/Sources/Meetings/UnappliedEditsBanner.swift
git commit -m "feat: play the audio behind a segment"
```

---

## После всех задач

- [x] Обновить `docs/backlog.md`, Epic 19: закрыть пункт «Проигрывание фрагмента и **весь интерфейс** — плана нет», оставив открытыми сжатие словаря и перечисленные в спеке исключения.
- [ ] `scripts/verify-mac.sh` целиком на Mac.
- [ ] Pull request.
