# AGENTS.md — rust/crates/session

Машина состояний встречи и `ChannelMixer` (ADR-004/009). Смешивание
каналов существует **только для живого STT**; на диске дорожки остаются
раздельными.

## Устройство

Пакет `meetingraft-session`, lib `session`. Зависимости: `domain`, `uuid`.
Модули: `engine`, `mixer`, `fake_captions`.

## Машина состояний (`engine.rs`)

Только `Idle → Live → Ended`. Любой другой переход —
`SessionError::InvalidTransition`: `start` из не-Idle, `stop` из не-Live.
`push_tick` вне `Live` молча отдаёт пусто — не ошибка, но и не событие.

## ChannelMixer (`mixer.rs`)

Пороги замерены; менять их без нового замера нельзя.

- Усиливается **только тихий системный канал**: `QUIET_CEILING_RMS = 700`
  не трогает нормальную речь, `GAIN_NOISE_FLOOR_RMS = 150` не поднимает
  тишину линии — иначе Whisper выдаёт титры на пустоте. `MAX_GAIN = 5.0`,
  `GAIN_SMOOTHING = 0.25`, целевой уровень `TARGET_RMS = 1200`.
- Доминирование с гистерезисом `HYSTERESIS_RATIO = 1.5`: перевес меньше
  полутора раз доминанта не меняет — защита от мигания канала.
- Допуск `DEFAULT_TOLERANCE_SLOTS = 2` при `DEFAULT_SLOT_MS = 100`: канал,
  опоздавший на два слота, отдаёт тишину, а не тянет чужой звук.
