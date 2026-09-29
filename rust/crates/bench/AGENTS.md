# AGENTS.md — rust/crates/bench

Стенд сравнения распознавателей. В приложение не входит — прибор, как
`stt-probe` и `diarize-probe`, и с той же дисциплиной: заведомо
положительный и заведомо отрицательный случай раньше настоящих данных.
Спека — `docs/superpowers/specs/2026-08-28-asr-bench-design.md`, план —
`docs/superpowers/plans/2026-08-28-asr-bench.md`.

## Устройство

- Пакет `meetingraft-bench`, `publish = false`. `[lib] name =
  "meetingraft_bench"` + `[[bin]] meetingraft-bench`.
- **Библиотека, а не один бинарь**: метрики нужны и соседям — `stt-probe`
  подключает `bench/src/wer.rs` файлом через `#[path]`. Своя реализация WER
  в приборе разошлась бы со стендом.
- Зависимости: `domain`; `storage`, `stt`, `postcall`, `diarize` — optional
  за фичами; `glossary` — **только dev**, чтобы пары CSV сверялись тем самым
  разбором, что в приложении (своя сборка CSV молча импортирует 0 строк).
- Модули: case, cpwer, dataset, engines, export, history, hotwords, judge,
  labels, metrics, run, segmentation, wav, wer.

## Фичи

`default = []`. `export` (тянет `storage`, гоняется только на Маке — там
лежат встречи), `gigaam`/`parakeet` (тянут `stt` и включают `biasing`),
`tone` (без `biasing`), `whisper`, `vad`, `diarize`, `judge`, `biasing`.

- `biasing` **вручную не задаётся**: его включают transducer'ы (`gigaam`,
  `parakeet`). Потоковый CTC `tone` смещение не поддерживает вовсе.
- В отличие от прибора, `gigaam` и `parakeet` здесь живут **рядом**: стенд
  для того и есть, чтобы гнать их на одном материале.

## Платформенный запрет

`compile_error!` в `bench/src/lib.rs:46-61` валит сборку при
`target_os = "linux"` вместе с `whisper` и любой sherpa-фичей
(`gigaam`/`parakeet`/`tone`/`vad`/`diarize`). Замерено `free(): invalid
pointer`; на macOS Metal и CPU-библиотеки уживаются. Запрет узкий
намеренно — расширять его по догадке нельзя.

## Эталон

`bench/src/labels.rs:5-13` — непроверенный текст в эталон не идёт никогда:
авторазметка «верно» запрещена, потому что согласные движки ошибаются
одинаково. Сверять расшифровку с расшифровкой без эталона — сверка догадки
с самой собой.

## Отношение к продукту

Стенд — не продукт. Общая логика живёт в core-крейтах; копия в стенде
меряла бы выдумку стенда, а не продукт. `judge` переиспользует LLM-клиент
`postcall`, а не заводит второй HTTP-клиент к Ollama.
