# Providers map & Meetings provenance — Design

**Date:** 2026-08-02
**Status:** Implemented; LLM `backend`, `ollama` and `openai_compat` wired
**Maps to:** ADR-003, ADR-005, ADR-007, ADR-008; Phase 6 local post-call
**Approved approach:** B — full provider map in Settings «на вырост» + provenance on Meetings tabs

## Goal

Сделать прозрачным:

1. **из чего** собираются Brief и Follow-up (какие вкладки / артефакты);
2. **какие провайдеры** используются для Live STT, post-call STT, translation, LLM;
3. **какие URL / пути** нужны и когда они показываются.

## Non-goals

- Реальный HTTP health-check / вызов Ollama, NLLB, WhisperX в этом UI-PR.
- Смена входа Brief/Follow-up на что-либо кроме Final transcript.
- Отдельный экран Pipeline внутри Meetings (отклонён в пользу баннеров + Settings Providers).

## Decisions

| Topic | Choice |
|-------|--------|
| Settings layout | Один Form «Providers» (подход 1) |
| Meetings provenance | Короткие баннеры на Live / Final / Artifacts |
| Brief/Follow-up input | Только **Final** transcript |
| Unavailable engines | Видны в picker, disabled + caption «скоро» |
| LLM default | `builtin_templates` |
| Post-call STT default | `local_final` (stitch live finals) |

## Meetings tabs provenance

| Tab | Content | Banner copy (RU UI ok) |
|-----|---------|------------------------|
| Live | `caption_events` from live STT | «Источник: Live STT · не используется для Brief / Follow-up» |
| Final | `final_transcripts` (live finals + glossary) | «Источник: Live finals + glossary · **вход для Brief / Follow-up**» |
| Artifacts | generated brief / follow-up | Над Generate: «Генерация из **Final** · builtin templates (без LLM)» либо выбранный backend engine/model |

Rules:

- Generate Brief / Follow-up остаются `disabled` если Final отсутствует.
- Tooltip на disabled: «Нужен Final transcript».
- Когда LLM engine ≠ `builtin_templates`: баннер Artifacts показывает имя engine/model вместо «без LLM».

## Settings → Providers

Session language (ADR-003) остаётся **над** Providers, не смешивается с engine pickers.

### Provider rows

| Section | Engine options | Model / path | Base URL (when shown) | MVP behavior |
|---------|----------------|--------------|-----------------------|--------------|
| **Live STT** | `mock` \| `whisper` (resolved by model file) | `{Application Support}/meetingraft/models/ggml-*.bin` | — | Status only; download via script |
| **Post-call STT** | `local_final` \| `backend_whisperx` | — | `POST {apiBase}/v1/jobs` | Only `local_final` enabled; WhisperX disabled «скоро» |
| **Translation** | `auto` \| `apple` \| `backend` \| `local_llm` \| `stub` \| `off` | — | `{translateBase}/v1/translate` for `backend` / `auto→backend` | Existing ADR-008 wiring |
| **LLM (Brief/Follow-up)** | `builtin_templates` \| `backend` \| `ollama` \| `openai_compat` | model id e.g. `gemma2` | Backend `{apiBase}`; Ollama/OpenAI-compat e.g. `http://127.0.0.1:11434` | All four engines enabled; local engines call `/api/chat` or `/v1/chat/completions` |
| **Data roots** | read-only | App Support root, models dir, DB path | — | Copyable paths |

### URL field rules

- Показывать TextField **только** если выбранный engine требует URL.
- Caption под полем: HTTP method + path (например `POST /v1/translate`).
- Статус: `configured` \| `missing model` \| `unreachable` — в UI-PR только `configured` / `missing model`; `unreachable` позже.

### Suggested API base fields (future shared)

Чтобы не плодить три разных «backend» URL без смысла:

- `apiBaseUrl` — общий backend (jobs, optional translate/LLM proxies) — ADR-007.
- `translateBaseUrl` — override для translation, если отдельный сервис; иначе = `apiBaseUrl`.
- `llmBaseUrl` — Ollama / OpenAI-compat local; не путать с `apiBaseUrl`.

В v1 UI: Translation имеет `backendBaseUrl`; для enabled `ollama` и
`openai_compat` показываются shared `llmBaseUrl` и model id.

## Architecture boundaries

- SwiftUI: presentation only — banners, Form, disabled state.
- Provider selection persisted via Observable stores → UniFFI where logic already exists (`set_translation_backend`, STT path queries).
- Settings использует локальный `MeetingCore` только для health-check; Generate перед вызовом применяет сохранённые настройки к shell core через `MeetingsViewModel.applyProviderConfig`.
- LLM / post-call STT pickers хранят выбор в Swift; доступные LLM-конфиги передаются через существующие UniFFI setters без параллельных бизнес-правил во views.
- Cocoa Translation stays in `HostTranslationBridge` (ADR-008).

## UI sketch (Settings)

```text
Session language: [ Русский ▾ ]

Providers
┌ Live STT ─────────────────────────────┐
│ Engine: Whisper (model found)         │
│ Model: …/models/ggml-base.bin         │
│ Models dir: …/meetingraft/models      │
└───────────────────────────────────────┘
┌ Post-call STT ────────────────────────┐
│ Engine: [ local_final ▾ ]             │
│ backend_whisperx — скоро              │
└───────────────────────────────────────┘
┌ Translation ──────────────────────────┐
│ Enable / Target / Backend (ADR-008)   │
│ Base URL (if backend): …              │
│ → POST {base}/v1/translate            │
└───────────────────────────────────────┘
┌ LLM (Brief / Follow-up) ──────────────┐
│ Engine: [ builtin / backend / ollama / openai_compat ▾ ] │
│ Local: Base URL + model id             │
└───────────────────────────────────────┘
┌ Data roots ───────────────────────────┐
│ App Support: …/meetingraft            │
└───────────────────────────────────────┘
```

## Implementation slices (for plan)

1. Meetings provenance banners + Artifacts caption + tooltip.
2. Settings restructure: Providers sections; Live STT labeling; Data roots.
3. Post-call STT + LLM pickers (disabled non-MVP options).
4. Docs: link from architecture / Settings help captions; optional tiny ADR note if ProviderConfig becomes Rust DTO later.

## Done criteria

- User can tell from Meetings alone that Brief/Follow-up come from Final, not Live.
- Settings lists transciber (live + post-call), translation, LLM, and shows URL fields only when relevant.
- No fake “working” engines: disabled options clearly marked.
