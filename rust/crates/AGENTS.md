# AGENTS.md — rust/crates

Правила Rust-воркспейса целиком. Частное живёт рядом с кодом:
граница UniFFI — `ffi/AGENTS.md`, распознавание — `stt/AGENTS.md`,
SQLite — `storage/AGENTS.md`, финал — `postcall/AGENTS.md`.

## Воркспейс

- `rust/Cargo.toml`: resolver 2, edition 2024, 20 members.
- `[profile.dev.package."*"] opt-level = 2` — зависимости оптимизируются
  даже в debug. Причина замерена: приложение линкует debug-dylib, а
  кодирование FLAC занимало 145 мс на пакет при бюджете 50 мс под
  мьютексом ядра. Свои крейты остаются неоптимизированными намеренно.
- **Ни у одного крейта нет своего `build.rs`.** Фразы «build.rs ходит в
  сеть» — про build.rs зависимости `sherpa-onnx`, а не про наш код.

## Имена

- Пакет — `meetingraft-<имя>`, но `[lib] name = <короткое>`: в Rust
  импортируется `domain`, `stt`, `session`, а не длинное имя пакета.
- Приборы и `bench` переопределяют `[[bin]]` в голый kebab-case
  (`echo-probe`, `stt-probe`); у `bench` бинарь — `meetingraft-bench`.
- `uniffi-bindgen` — единственный пакет без префикса.
- Каталог и имя пакета не совпадают: `-p storage` молча не найдёт ничего,
  нужно `-p meetingraft-storage`.

## Карта крейтов

| Крейт | Пакет | Зависит от | Ключевые модули |
|---|---|---|---|
| `domain` | `meetingraft-domain` | — (лист графа) | audio, caption, diagnostics, glossary, language, postcall, session, speaker |
| `session` | `meetingraft-session` | domain, uuid | см. `session/AGENTS.md` |
| `stt` | `meetingraft-stt` | domain | см. `stt/AGENTS.md` |
| `glossary` | `meetingraft-glossary` | domain, uuid | см. `glossary/AGENTS.md` |
| `postcall` | `meetingraft-postcall` | domain, glossary, reqwest | см. `postcall/AGENTS.md` |
| `storage` | `meetingraft-storage` | domain, rusqlite (bundled), flac-codec | см. `storage/AGENTS.md` |
| `sync` | `meetingraft-sync` | **ни одного `meetingraft-*`** | client (blocking), dto, error, job_poll |
| `translate` | `meetingraft-translate` | domain, uuid | policy, stub, host/http/local_llm |
| `diarize` | `meetingraft-diarize` | domain | см. `diarize/AGENTS.md` |
| `ffi` | `meetingraft-ffi` | все core-крейты | см. `ffi/AGENTS.md` |
| `bench` | `meetingraft-bench` | domain; storage/stt/postcall/diarize — опц.; glossary — dev | см. `bench/AGENTS.md` |

- `sync` изолирован намеренно: его `dto.rs` зеркалит `shared/openapi.yaml`
  (ADR-007), и зависимость на `domain` привязала бы контракт backend к
  внутренним типам.
- `translate` **не имеет HTTP-зависимости**, хотя модуль `http` есть:
  `host`, `http`, `local_llm` — скелеты. `auto` откатывается на `stub`.
- `diarize` никогда не выдаёт имена — только метки кластеров. Имена
  ставит человек в UI.
- `ChannelMixer` смешивает каналы **только для живого STT**; на диске
  дорожки остаются раздельными (ADR-009).

## Фичи

Дисциплина единая: каждый тяжёлый или сетевой движок — за opt-in фичей,
`default = []`. `ffi` пробрасывает только `whisper`, `diarize`, `gigaam`;
`parakeet` и `tone` наружу не выходят. Сеть на сборке тянут только
sherpa-фичи (`gigaam`, `parakeet`, `tone`, `vad`, `diarize/model`) — через
build.rs зависимости `sherpa-onnx`. `whisper` (`dep:whisper-rs`) собирается
локально и требует `cmake`, а не сети.

## Приборы и стенд

Приборы в приложение не входят: `echo-probe`, `gate-probe`,
`diarize-probe`, `dup-probe`, `term-probe`, `brief-probe`, `fix-probe`,
`stt-probe`. Пакеты с префиксом, бинарь — голый kebab-case, `src/main.rs`.
`stt-probe` держит `gigaam` и `parakeet` **взаимоисключающими намеренно**:
движок выбирается явно, иначе число приедет без подписи.

`bench` — стенд сравнения распознавателей; отдельный файл `bench/AGENTS.md`
(платформенный `compile_error!`, фичи, эталон).

## Ловушки

- Не добавляй крейту свой `build.rs` без нужды: сеть на сборке — это
  `sherpa-onnx`, и она уже за фичей.
- `bench` и приборы — не продукт. Общая логика живёт в core-крейтах, а не
  копируется в прибор: копия меряет выдумку прибора, а не продукт.
- Полный `cargo test` по воркспейсу не влезает в память Linux-машины —
  там гонять по крейтам (`-p meetingraft-storage -p meetingraft-postcall`).
