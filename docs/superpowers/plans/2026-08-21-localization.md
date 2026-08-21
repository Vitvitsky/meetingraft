# Локализация — план работ

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Интерфейс целиком на английском на английской системе и целиком
на русском на русской.

**Architecture:** Каталог `Localizable.xcstrings` с английским базовым и
русским переводом. Литералы во вьюхах остаются литералами — SwiftUI сам
берёт их как `LocalizedStringKey`; меняется лишь язык самого литерала.
Вне вьюх строки оборачиваются в `String(localized:)`. Полноту каталога
проверяет `scripts/check-localization.py`, который гоняется на Linux.

**Tech Stack:** Swift, String Catalog (`.xcstrings`, JSON), xcodegen,
Python 3 для прибора.

Спека: `docs/superpowers/specs/2026-08-21-localization-design.md`.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; коммиты — **по-английски**.
- **Swift здесь не собирается.** Прибор проверяет полноту каталога и
  отсутствие русского в интерфейсе — и только это. Синтаксис Swift
  остаётся непроверенным до Мака.
- Порядок жёсткий: **прибор первым**, до единой правки строк. Его
  положительный контроль — текущее состояние репозитория, где каталога
  нет вовсе: прибор обязан на нём краснеть.
- Ключи английские; `snake_case` не вводить — ключом служит сама
  английская фраза, как в 118 существующих `String(localized:)`.
- Ветка: `feat/localization`.

---

### Task 1: Прибор, проверяющий полноту

**Files:**
- Create: `scripts/check-localization.py`

- [ ] **Step 1: Написать прибор** — четыре проверки из спеки, белый
      список из решения 3, вывод с именем файла и строкой.
- [ ] **Step 2: Убедиться, что он краснеет на текущем репозитории.**
      Каталога нет, русские строки на месте — обязаны провалиться
      проверки 1, 2 и 4. Это его заведомо положительный случай, и он
      бесплатный: состояние уже такое.
- [ ] **Step 3: Проверить, что белый список работает** — демо-речь и
      эндонимы не должны попадать в отчёт.
- [ ] **Step 4: Коммит.**

---

### Task 2: Каталог и ресурсы

**Files:**
- Create: `apps/macos/Sources/Resources/Localizable.xcstrings`
- Modify: `apps/macos/project.yml` (ресурсы, `developmentRegion`)

- [ ] **Step 1: Завести каталог** с 118 существующими ключами и русским
      переводом к каждому.
- [ ] **Step 2: Подключить ресурсы в `project.yml`** и объявить базовым
      языком английский.
- [ ] **Step 3: Прогнать прибор** — проверки 2 и 4 обязаны позеленеть,
      проверка 1 остаться красной (русские литералы ещё на месте).
- [ ] **Step 4: Коммит.**

---

### Task 3: Русские литералы во вьюхах

**Files:** `MeetingDetailView`, `SpeakersPanelView`, `FinalSegmentsView`,
`UnappliedEditsBanner`, `GlossaryView`, `AppShellView`,
`SettingsProviderSections`.

- [ ] **Step 1: Заменить литерал на английский**, добавив ключ и русский
      перевод в каталог. По файлу за раз.
- [ ] **Step 2: Прогнать прибор** после каждого файла.
- [ ] **Step 3: Коммит по файлу или по паре близких файлов.**

---

### Task 4: Строки вне вьюх

**Files:** `SpeakerAttributionViewModel`, `ProviderSettingsStore`,
`MeetingsViewModel`, `SettingsModel`, `GlossaryViewModel`,
`AudioCaptureCoordinator`, `MicrophoneCapture`, `SystemAudioCapture`.

- [ ] **Step 1: Обернуть в `String(localized:)`** и сменить на
      английский, добавив ключ и перевод.
- [ ] **Step 2: Прогнать прибор.**
- [ ] **Step 3: Коммит.**

---

### Task 5: Множественное число

- [ ] **Step 1: Найти строки с числом** — прибор печатает их отдельным
      списком: интерполяция числа в локализуемой строке.
- [ ] **Step 2: Завести вариации** `one/few/many/other` для русского,
      `one/other` для английского.
- [ ] **Step 3: Коммит.**

---

### Task 6: Разрешения на языке системы

**Files:**
- Create: `apps/macos/Sources/Resources/InfoPlist.xcstrings`
- Modify: `apps/macos/project.yml`

- [ ] **Step 1: Перенести** `NSMicrophoneUsageDescription` и
      `NSAudioCaptureUsageDescription` на английский в `project.yml`,
      русский — в `InfoPlist.xcstrings`.
- [ ] **Step 2: Коммит.**

---

### Task 7: Прибор в pre-commit и документы

- [ ] **Step 1: Добавить хук** рядом с `swiftformat-lint`.
- [ ] **Step 2: Обновить** Phase 15 в роадмапе и раздел проверки за
      Маком: две локали, первый запуск на чистой машине.
- [ ] **Step 3: Прогнать прибор целиком, все четыре проверки зелёные.**
- [ ] **Step 4: Коммит.**

---

## Чего этот план не содержит

- Локализации ошибок из Rust-ядра: они рождаются там и переводятся там.
- Испанского интерфейса.
- Перекомпоновки экранов под длину английских строк — работа глазами.
