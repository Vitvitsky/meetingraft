# ТЗ: редизайн macOS UI MeetingRaft (по `mac_design.pen`)

**Источник:** `mac_design.pen` (Pencil / design system + экраны)
**Дата выжимки:** 2026-08-03
**Цель документа:** текстовое ТЗ «как переделать дизайн» текущего SwiftUI
shell под макет, без привязки к конкретной реализации пиксель-в-пиксель.

> **Оговорка про источник.** `mac_design.pen` читается только запущенным
> pen.dev: ни в диффах, ни агентами, ни на CI он не открывается
> (`product-ux-review-2026-08-03.md` §3.7). Пока рядом не лежит экспорт
> (PNG или HTML), **этот документ — рабочий источник правды по визуалу**,
> а не сам `.pen`.

## 1. Цель редизайна

Перевести MeetingRaft с утилитарного Form/List-shell на **тёмную
native-first оболочку** с единым визуальным языком:

- тёмный «graphite» фон, спокойный синий акцент;
- чёткая иерархия: toolbar → контент → status;
- отдельные поверхности для Live, Meetings, Settings и будущих режимов
  (overlay, menu bar, calendar).

Редизайн **не отменяет** архитектурные границы (`AGENTS.md`): SwiftUI
только presentation; бизнес-логика / STT / sync — через UniFFI / Rust /
backend.

## 2. Design system (обязательный фундамент)

Перед экранами внедрить токены (сейчас в pen как variables; в Swift —
`Color`/`Font`/`CGFloat` constants или Asset Catalog).

### 2.1 Цвета

| Токен | Тёмная | Светлая | Назначение |
|-------|--------|---------|------------|
| `surface-root` | `#0D0D0F` | `#FFFFFF` | Фон окна |
| `surface` | `#141416` | `#F5F5F7` | Панели / toolbar |
| `surface-elevated` | `#1C1C1F` | `#FFFFFF` | Карточки, elevated rows |
| `surface-overlay` | `#242428` | `#FFFFFF` | Всплывающие панели |
| `surface-glass` | чёрный @ 19% | белый @ 72% | Overlay / glass |
| `text-primary` | `#FFFFFF` | `#1D1D1F` | Основной текст |
| `text-secondary` | `#A1A1A6` | `#6E6E73` | Вторичный |
| `text-tertiary` | `#6E6E73` | `#7C7C82` | Третичный |
| `text-disabled` | `#48484D` | `#C7C7CC` | Disabled |
| `accent-primary` | `#0A84FF` | `#0069D9` | CTA / selected |
| `accent-bg` | акцент @ 9% | акцент @ 10% | Selected chip bg |
| `success` | `#30D158` | `#1D7A32` | Статус |
| `warning` | `#FFD60A` | `#B25000` | Статус |
| `error` | `#FF453A` | `#D70015` | Статус |
| `info` | `#64D2FF` | `#0071A4` | Статус |
| `border-subtle` / `default` / `strong` | белый @ 6–14% | чёрный @ 6–14% | Разделители |

Тема: **Auto по умолчанию**, в Settings → General переключатель
System / Light / Dark. Реализовано 2026-08-20.

Три значения здесь не те, что стояли в первой редакции, и каждое —
следствие счёта, а не вкуса (спека
`docs/superpowers/specs/2026-08-20-light-theme-and-honest-ui-design.md`):

- **Акцент тёмной темы `#0A84FF`**, а не `#4A9FD8`: системный синий
  Apple, о котором просил Epic 23.
- **Акцент светлой `#0069D9`**, а не системный `#007AFF`: тот даёт на
  белом 4.02:1, ниже порога 4.5:1 для обычного текста, а этим цветом
  красятся подписи в 10–13 пунктов.
- **Предупреждение светлой оранжевое.** Жёлтое `#FFD60A` на белом даёт
  1.41:1 — надпись пропадает. Перенести статусные цвета тёмной темы как
  есть нельзя: так падают пять из восьми, и это отрицательный контроль
  теста `ThemeContrastTests`.

### 2.2 Типографика

- Sans: **SF Pro** (системный). Решение 2026-08-20: предписание брать
  Inter отменено — пользователю понравился ровно системный шрифт, а
  Inter увёл бы от него и добавил файл в бандл.
- Mono: **IBM Plex Mono** (latency, ids, timestamps).
- Шкала: caption 10 · body-sm 12 · body 13 · body-lg 15 · title 17 ·
  headline 20 · large 28 · xlarge 34.
- Веса: 400 / 500 / 600 / 700.

### 2.3 Spacing & radius

- Space: 4 / 8 / 12 / 16 / 24 / 32 / 48.
- Radius: xs 3 · sm 6 · md 8 · lg 12 · xl 16 · full pill.

### 2.4 Компоненты (из pen)

Реализовать как переиспользуемые SwiftUI views:

- **Button:** Primary / Secondary / Tertiary / Destructive / Icon.
- **Chips:** language (`RU`), status (`REC`, `Active`), engine badges.
- **List rows:** meeting row (id · time · duration · lang · artifact count).
- **Settings row:** label + control + caption.
- **Provenance / status badges** внизу экрана.

## 3. Информационная архитектура

### 3.1 Навигация верхнего уровня (целевая)

| Область | Роль |
|---------|------|
| **Live Captions** | Основной recording / субтитры |
| **Meetings** | История + detail (Live / Final / Speakers / Artifacts; Compare — поверх Final) |
| **Glossary** | Отдельное окно/сцена (как сейчас sidebar, но с новым layout) |
| **Settings** | Sidebar-разделы (не один длинный Form) |
| **Menu bar** | Tray + shortcuts + detection |
| **Overlay** | Отдельное всегда-on-top окно |

### 3.2 Settings — разделы sidebar (обязательная структура)

1. General (язык сессии, appearance, about)
2. Audio
3. STT Engine
4. Translation
5. LLM Providers
6. Backend API
7. Data & Storage

Текущий монолитный `SettingsView` Form разбить по разделам.

## 4. Экраны: требования «как в макете»

Ниже — **функциональная** спецификация UI. Копирайт с макета можно
русифицировать, смысл сохранить.

### 4.1 Live Captions (главный redesign)

**Layout:** вертикальный — Toolbar → Captions (центр) → Controls →
Audio meter → Status bar.

**Toolbar**

- Traffic lights (native).
- Badge **REC** (error-red + белая точка) при записи.
- Chips: язык (`RU`), таймер сессии (`12:34`).

**Captions**

- Крупный блок по центру (~740pt), 1–3 строки: текущий partial +
  недавние finals.
- Читаемость > плотность списка (не как длинный `List` всех событий по
  умолчанию; полный лог — вторично или в Meetings → Live).

**Controls**

- Primary: **Stop Recording**.
- Secondary: **Pause**.

**Audio meter**

- Подпись уровня (Microphone Level) + визуальный meter.

**Status bar**

- Слева: `Whisper {model} · on-device · {latency} ms`.
- Справа: `N terms active` (glossary).

**Отличие от текущего:** меньше «инженерных» панелей в первом экране;
фокус на субтитрах и REC-состоянии.

### 4.2 Meetings

**3 колонки:**

1. **Sidebar filters:** All / Today / This Week / Older (+ counts).
2. **List:** строки встреч — короткий id, relative date, duration, lang
   chip, счётчик артефактов.
3. **Detail:** tab bar **Live | Final | Speakers | Artifacts**
   (+ Compare уже в коде — вписать в tab bar макета).

**Final tab (макет):**

- Заголовок «Final Transcript» + meta (дата · длительность · язык ·
  N speakers).
- Сегменты с **именем спикера + timestamp + текст** (целевой UX; сейчас
  текст без speaker attribution до diarization — показывать спикеров
  когда есть, иначе plain paragraphs).

**Actions** внизу detail — Generate / Export / Refine по текущей логике,
но в стиле primary/secondary кнопок DS.

### 4.3 Settings — General

- Session: Default Language; Allowed Languages (RU/EN/ES chips).
- Appearance: Theme Dark/Light/Auto; Accent Color.
- About: Version, Stack, Whisper summary.

### 4.4 Audio & Devices

- Input device card (name · built-in · sample rate · channels).
- Sources: **Microphone** / **System Audio** / **Both (Mixed)** с
  статусами Active / Available / Pro.
- Quick Test: speak-to-test mic.

Связать с `feat/phase-8-system-audio` (mic/system/mix) — UI должен
отражать реальные режимы захвата.

### 4.5 Glossary

- Toolbar + search.
- Sidebar scopes: All / RU / EN / ES / Meeting (+ counts).
- Term rows: surface → canonical, lang chip, scope (Global / Meeting).

### 4.6 Transparent Overlay Mode (новый продуктовый режим)

Floating always-on-top окно субтитров:

- Режимы: **Compact → Medium → Full** (double-click cycle).
- Opacity cycle: 20→40→60→80% (⌘⇧T).
- Hover → controls (mode, opacity, close).
- Drag anywhere; join all Spaces.

**Статус:** нет в текущем shell — отдельный slice после базового DS +
Live redesign.

### 4.7 Menu Bar & Meeting Detection (новый)

- Tray icon states: Idle / Meeting detected / Recording / Post-call.
- Dropdown: Start Live, Meetings, Glossary, Overlay, Preferences, Quit
  (+ shortcuts).
- Popup «Zoom meeting detected» → Start Recording.
- Detection: Zoom/Teams/Meet/Webex/Discord/Skype; NSWorkspace + bundle IDs.
- Settings: auto-detect, popup, auto-start, launch at login.

### 4.8 Calendar & Today's Meetings (новый / later)

- Timeline дня + Up Next.
- Sources: macOS Calendar / Google / Outlook.
- Auto-record rules (≥3 participants, recurring, important, 1:1).

### 4.9 Brief & Follow-up Generator

Двухпанельный UX вместо кнопок-only:

- Слева: template cards — Brief / Follow-up / Minutes / Action Items;
  engine + language; Generate.
- Справа: Preview (Attendees / Decisions / Actions) + copy/export.

Minutes / Action Items — из backlog (сейчас только Brief/Follow-up).

### 4.10 Speakers & Diarization / Speaker Identification

- Список спикеров: avatar initial, роль, speaking time, segment count.
- Unknown Speaker + assign flow.
- Отдельный wizard «Who is speaking?» с clips + confidence match.
- Banner «Auto-diarization · Coming soon» пока нет ML.

### 4.11 Export

Форматы: `.md` / `.srt` / `.vtt` / `.txt`.
Опции: timestamps, speaker names, merge consecutive, remove fillers.
Preview справа.

Сейчас в продукте — в основном markdown folder export; остальные форматы —
follow-up.

### 4.12 Brief Templates / Quick Note

- Editor шаблонов с `{{placeholders}}` (transcript, speakers, glossary, …).
- Quick Note: agenda/notes/actions + quick actions (voice memo, generate
  brief, extract actions, translate, PDF).

Оба — backlog / later; в ТЗ зафиксированы как целевые экраны.

### 4.13 Settings — Translation / LLM / Backend / Data

Соответствуют уже существующим возможностям, но в **sidebar Settings**:

- Translation: enable, target, engine (Apple/backend), glossary apply.
- LLM: engine cards (Builtin / Ollama / OpenAI-compat / Backend) +
  URL/model + Test.
- Backend API: base URL, token, Test, jobs/endpoints help.
- Data & Storage: usage breakdown (transcripts / audio / models /
  glossary), paths, clear/open actions.

## 5. Что есть сейчас vs что менять (gap)

| Область | Сейчас | По макету |
|---------|--------|-----------|
| Visual | System Form, светлая/нейтральная | Dark DS + токены |
| Live | Список captions + controls | Center stage captions + REC + status bar |
| Meetings | List + detail tabs | 3-column + richer Final with speakers |
| Settings | Один Form | Sidebar 7 разделов |
| Audio | Частично / phase-8 | Карточки sources Mic/System/Both |
| Overlay / Tray / Calendar | Нет или минимально | Отдельные продуктовые поверхности |
| Artifacts | Кнопки Generate | Template picker + preview pane |
| Export | Markdown folder | Multi-format + options + preview |

## 6. Поэтапная реализация (рекомендуемый порядок)

### Phase D1 — Design kit (обязательный)

1. Токены цвета/типо/spacing/radius в `apps/macos`.
2. Button / Chip / SettingsRow / StatusBadge.
3. Применить kit к оболочке окна (фон `surface-root`).

### Phase D2 — Core screens redesign

1. **Live Captions** по §4.1.
2. **Meetings** 3-column + tabs (включая Compare).
3. **Settings** sidebar + General / Audio / STT / LLM / Backend / Data /
   Translation.
4. **Glossary** layout §4.5.

### Phase D3 — Product surfaces (после D2)

1. Menu bar tray + detection popup.
2. Transparent overlay window.
3. Audio sources UI ↔ system audio pipeline.

### Phase D4 — Intelligence UX

1. Brief generator dual-pane + templates Minutes/Actions.
2. Export multi-format.
3. Speakers identification wizard (когда diarization).
4. Calendar / Quick Note / Template editor.

## 6.1 Связь с продуктовым roadmap

D-фазы визуальные, фазы 8–16 (`docs/roadmap.md`) продуктовые; они
пересекаются, и порядок склейки такой:

| D-фаза | Зависит от | Комментарий |
|--------|-----------|-------------|
| D1 | — | Можно делать в любой момент, ничего не блокирует |
| D2 Live | Phase 8 | Status bar и REC осмысленны, когда пишутся оба канала; атрибуция говорящего уже есть в `FfiCaptionEvent.channel` |
| D2 Meetings | Phase 9 | 3 колонки с фильтрами и счётчиками требуют названий, длительности и поиска |
| D2 Final с сегментами | Phase 10, 11 | До re-ASR и диаризации показывать plain paragraphs |
| D2 Settings sidebar | — | Независимо; заодно закрывает пункт «убрать номера ADR из UI» (Phase 15) |
| D3 Menu bar, Overlay | Phase 12 | Это одна и та же работа: Phase 12 даёт поведение, D3 — визуал |
| D3 Audio sources | Phase 8 | Карточки Mic / System / Both отражают реальные режимы |
| D4 | Phase 10, 11, 14 | Артефакты и экспорт следуют за содержательной частью |

Практический вывод: **D1 и Settings sidebar можно брать параллельно с
Phase 8**, остальное подключается по мере готовности продуктовых фаз.
Phase 15 («чистка UI и локализация») из roadmap фактически растворяется в
D1–D2 — при переходе на design kit инженерные артефакты уходят сами.

## 7. Acceptance criteria (D1+D2)

- [ ] Все цвета экранов D2 берутся из токенов, не hardcode ad-hoc.
- [ ] Live: REC badge, centered captions, Stop/Pause, status
      (model · latency · glossary count).
- [ ] Meetings: filters + list + detail tabs в одном окне без «голого» Form.
- [ ] Settings: sidebar ≥7 разделов; содержимое не в одном бесконечном Form.
- [ ] Glossary: search + scope sidebar + term rows.
- [ ] Границы архитектуры не нарушены (нет networking/business rules во
      views).
- [ ] Существующие потоки (Start/Stop Live, Generate Brief/Follow-up,
      Export md, Backend test) работают.

## 8. Non-goals этого ТЗ

- Пиксель-perfect parity с каждым frame pen.
- Реализация diarization / calendar sync / overlay в том же PR, что D1.
- Смена стека (SwiftUI остаётся).
- Light theme full polish (достаточно Dark + переключатель).

## 9. Риски и решения

| Риск | Решение |
|------|---------|
| Inter / IBM Plex не в системе | Бандл шрифтов или SF / SF Mono fallback с токеном |
| Overlay + Spaces / always-on-top | Отдельный `WindowGroup` / `NSPanel`; ADR на permissions |
| Meeting detection privacy | Явный opt-in; список bundle IDs в Settings |
| Speakers в Final UI без diarization | Показывать ручные метки / placeholder до Epic 9 ML |
| `.pen` нечитаем вне pen.dev | Держать рядом экспорт PNG/HTML; при расхождении верить этому ТЗ |

## 10. Источник правды

- Визуал: `mac_design.pen` (+ этот документ как читаемая выжимка)
- Поведение продукта: `docs/architecture.md`, `docs/backlog.md`, ADR
- При конфликте: **поведение ADR/backlog > декоративные детали pen**;
  **IA и иерархия pen > текущий Form layout**.
