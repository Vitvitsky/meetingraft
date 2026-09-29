# AGENTS.md — scripts

Проверки и приборы. Шаги `verify-mac.sh` и общая дисциплина приборов —
в корневом `AGENTS.md`; здесь разделение по платформам и конвенция
загрузки моделей.

## Только на Маке

- `verify-mac.sh` — полная проверка репозитория, 7 шагов: тесты Rust →
  clippy/fmt → clippy `meetingraft-stt --features whisper` →
  `generate-ffi.sh` → swiftformat → xcodebuild test → pre-commit.
- `measure-power.sh` — замер энергии через powermetrics; на не-Darwin
  падает жёстко.
- `palette-negative-control.sh` — отрицательный контроль
  `ThemeContrastTests` через xcodebuild.
- `ax-probe.swift` — дерево Accessibility: виден ли активный говорящий из
  другого приложения.
- `capture-start-breakdown.sh` — разбирает диагностический лог приложения
  (стоимость шагов старта захвата, Epic 25). Лог **читает**, а не меряет:
  его числа — не живой тайминг.

## Кросс-платформенные

- `check-localization.py` — полнота каталога переводов; подключён в
  `pre-commit` (хук `check-localization`). В CI не вызывается: автозапуск CI
  выключен. Гоняется и на Linux.
- `fetch-diarize-models.sh`, `fetch-gigaam-models.sh`,
  `fetch-parakeet-models.sh`, `fetch-tone-model.sh`,
  `fetch-vad-model.sh`.

## Загрузка моделей и известный ответ

Загрузчики кладут модели в `<data-dir>/models/<engine>/` и дополнительно
тянут образец с известным ответом в `check/`. Правило: **прибор не выносит
вердикта без образца с известным ответом**, и каждый прибор начинается с
заведомо положительного и заведомо отрицательного случая раньше настоящих
данных. `diarize-probe` без фичи `model` честно проваливает самопроверку —
это не поломка.

## Зачем отрицательные контроли

`palette-negative-control.sh` существует, чтобы доказать: тест контраста
**может** упасть на заведомо плохой палитре. Зелёный тест, который не
может упасть, не доказывает ничего.
