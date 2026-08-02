# Providers map & Meetings provenance — Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Показать provenance Brief/Follow-up на вкладках Meetings и карту Providers (STT / translation / LLM / URL) в Settings.

**Architecture:** Только Swift presentation: banners + Form. Новые pickers (post-call STT, LLM) живут в `ProviderSettingsStore` до UniFFI; translation/STT path — существующие stores/MeetingCore. Disabled engines видны с подписью «скоро».

**Tech Stack:** SwiftUI, Observation, XCTest, MeetingCore UniFFI (read-only paths).

**Spec:** `docs/superpowers/specs/2026-08-02-providers-and-provenance-design.md`

## Global Constraints

- Brief/Follow-up вход = только Final.
- URL-поля только когда engine требует URL.
- Нерабочие engines: видны, выбор откатывается / disabled + «скоро».
- Без HTTP health-check и без вызова Ollama/NLLB/WhisperX.

---

## File map

| File | Role |
|------|------|
| `apps/macos/Sources/App/ProviderSettingsStore.swift` | Post-call STT + LLM pickers, apiBaseUrl/llm fields |
| `apps/macos/Sources/Settings/SettingsView.swift` | Providers Form |
| `apps/macos/Sources/Meetings/MeetingDetailView.swift` | Provenance banners |
| `apps/macos/Sources/MeetingRaftApp.swift` | `.environment(providerStore)` |
| `apps/macos/Tests/ProviderSettingsStoreTests.swift` | Defaults + availability |
| `docs/architecture.md` | One-line Providers note |

---

### Task 1: ProviderSettingsStore + tests

- [x] Add store enums: `PostCallSttEngine`, `LlmEngine` with `isAvailable`
- [x] Defaults: `localFinal`, `builtinTemplates`
- [x] XCTest defaults + availability
- [x] Wire into `MeetingRaftApp`

### Task 2: Settings Providers UI

- [x] Restructure `SettingsView`: language → Live STT → Post-call STT → Translation → LLM → Data roots
- [x] Live STT status from MeetingCore paths
- [x] Translation URL caption `POST /v1/translate`
- [x] Larger settings window

### Task 3: Meetings provenance banners

- [x] Banner on Live / Final / Artifacts (copy from spec)
- [x] Artifacts caption uses LLM engine label when builtin
- [x] Tooltip on disabled Generate buttons

### Task 4: Docs + verify

- [x] architecture.md note
- [x] `xcodebuild test` (25 passed)
- [x] Commit on feature branch (`dd30027`)

---

## Done criteria

- Meetings alone clarifies Brief ← Final, not Live.
- Settings lists live STT, post-call STT, translation, LLM + conditional URLs.
- Unavailable engines clearly marked.
