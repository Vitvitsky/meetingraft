# MeetingRaft — Дизайн-спецификация (ТЗ)

Версия 1.0 · macOS Dark Mode · Apple HIG

Файл макетов: `mac_design.pen`

---

## 1. Общие принципы

| Принцип | Реализация |
|----------|-----------|
| **Нативность** | macOS-native UI: SF Pro, `.ultraThinMaterial`, правильные отступы HIG |
| **Тёмный режим** | Все экраны в dark mode, поверхность `#0D0D0F` |
| **Единая дизайн-система** | 47 токенов (цвета, типографика, отступы, скругления) + 5 компонентов |
| **Glass-эффекты** | `NSVisualEffectView` для боковых панелей и overlay |
| **Анимации** | Spring-анимации, `.matchedGeometryEffect`, hover-эффекты |

---

## 2. Design Tokens

### 2.1 Цвета

```yaml
# Surface
surface-root:      "#0D0D0F"   # Корневой фон экранов
surface:           "#141416"   # Боковые панели, тулбары
surface-elevated:  "#1C1C1F"   # Карточки, elevated-элементы
surface-overlay:   "#242428"   # Модальные overlay

# Text
text-primary:      "#FFFFFF"   # Основной текст
text-secondary:    "#A1A1A6"   # Второстепенный текст
text-tertiary:     "#6E6E73"   # Подсказки, мета-информация
text-disabled:     "#48484D"   # Неактивный текст
text-placeholder:  "#636368"   # Плейсхолдеры в полях

# Accent
accent-primary:    "#4A9FD8"   # Основной акцент (синий)
accent-secondary:  "#3A8BC0"   # Тёмный акцент
accent-bg:         "#4A9FD818" # Фон под акцентом
accent-bg-hover:   "#4A9FD828" # Фон при наведении

# Semantic
success:           "#30D158"   # Успех (зелёный)
warning:           "#FFD60A"   # Предупреждение (жёлтый)
error:             "#FF453A"   # Ошибка / Live (красный)
info:              "#64D2FF"   # Информация (голубой)

# Border
border-subtle:     "#FFFFFF0F" # Едва заметный
border-default:    "#FFFFFF18" # Стандартный
border-strong:     "#FFFFFF24" # Выраженный
```

### 2.2 Типографика

| Токен | Размер | Вес | Применение |
|-------|--------|-----|-----------|
| `text-caption` | 10px | Medium | Бейджи, метки, footnote |
| `text-body-sm` | 12px | Regular | Списки, sidebar, чипсы |
| `text-body` | 13px | Regular | Основной текст, кнопки |
| `text-body-lg` | 15px | Regular | Описания, подзаголовки |
| `text-title` | 17px | Semibold | Заголовки секций |
| `text-headline` | 20px | Bold | Заголовки экранов |
| `text-large-title` | 28px | Bold | Заголовки страниц |
| `text-xlarge-title` | 34px | Bold | Hero-заголовки |

- **Sans**: Inter (основной)
- **Mono**: IBM Plex Mono (ID, код, таймкоды)

### 2.3 Отступы и скругления

```yaml
spacing:  xs(4) → sm(8) → md(12) → lg(16) → xl(24) → 2xl(32) → 3xl(48)
radius:   xs(3) → sm(6) → md(8) → lg(12) → xl(16) → full(999)
```

---

## 3. Reusable Components

| Компонент | Описание | Стили |
|-----------|----------|-------|
| **Button / Primary** | Основная кнопка | `fill: accent-primary`, `text: white`, `radius: md` |
| **Button / Secondary** | Второстепенная | `fill: border-subtle`, `text: text-secondary` |
| **Button / Tertiary** | Текстовая кнопка | Без заливки, `text: text-secondary` |
| **Button / Destructive** | Опасное действие | `fill: error`, `text: white` |
| **Button / Icon** | Иконка-кнопка | `fill: border-subtle`, иконка 16px |

Все кнопки доступны как `ref` с переопределением текста через descendants:
```js
Insert(parent, {type:"ref", ref:"LbSLG", name:"Save", descendants:{TUSSI:{content:"Save"}}})
```

---

## 4. Экраны

### 4.1 Welcome / Onboarding
**Назначение**: Первый запуск приложения.
- Крупный логотип MeetingRaft (иконка `waves`, акцентный фон)
- Приветственный заголовок + описание
- 3 шага onboarding: язык → модель Whisper → первая запись
- Кнопка "Get Started" (Primary, с иконкой стрелки)

### 4.2 Menu Bar & Meeting Detection
**Назначение**: Системный трей + авто-детект встреч.
- **Menu Bar**: иконка в трее macOS, выпадающее меню (Start Live, Meetings, Glossary, Overlay, Settings, Quit)
- **Meeting Detection Popup**: при запуске Zoom/Teams/Meet — всплывает окно с предложением начать запись
- **Состояния иконки**: Idle (waves), Meeting detected (video, жёлтый), Recording (circle, красный), Post-call (file-text, синий)
- **Настройки детекта**: тогглы auto-detect, notification popup, auto-record, launch at login

### 4.3 Calendar & Today's Meetings
**Назначение**: Импорт календаря + список встреч на сегодня.
- **Источники**: macOS Calendar (✅), Google Calendar (✅), Outlook (Connect)
- **Timeline**: вертикальная линия событий с цветовой маркировкой
- Каждая встреча: время, цветовая полоса, название, приложение (Zoom/Teams), количество участников, бейдж REC
- **Summary-карточки**: Meetings (5), Recording (2), Free time (3h 15m)
- **Auto-Record Rules**: ≥3 участников, повторяющиеся, важные, 1:1
- **Up Next**: ближайшая встреча с кнопкой Join & Record

### 4.4 Live Captions
**Назначение**: Главный экран записи встречи с субтитрами.
- **Toolbar**: Window Controls + Live Badge (REC, красный) + Lang Chip (RU) + Duration (моно) + Settings (иконка)
- **Captions Box**: elevated-фон, 4 строки субтитров
  - Partial (tertiary, 16px) → Active (large-title, 28px) → Next (secondary) → Old (tertiary, 50% opacity)
- **Controls**: Stop Recording (error, красный) + Pause (border-default)
- **Audio Meter**: 24 полосы accent-primary, анимированные
- **Status Bar**: STT dot (success) + "Whisper large-v3 · on-device · 156 ms" + Glossary badge (42 terms)

### 4.5 Transparent Overlay Mode
**Назначение**: Floating окно субтитров поверх всех приложений.
- **NSWindow**: `.floating` level, `.borderless` styleMask, `NSVisualEffectView(.hudWindow)`
- **Compact**: одна строка + красная точка, `#00000040` glass
- **Medium**: две строки + время/язык в top bar
- **Full**: control bar (LIVE, время, drag handle, opacity %, close) + 3 строки + footer (Whisper статус + glossary)
- **Взаимодействия**: Hover → controls, Drag → reposition, Double-click → cycle mode, ⌘⇧T → cycle opacity (20→40→60→80%)
- **Поведение**: `canJoinAllSpaces`, `fullScreenAuxiliary`, movable by background

### 4.6 Meetings
**Назначение**: История встреч с просмотром транскрипта.
- **Трёхколоночный**: Sidebar (группы All/Today/Week/Older) → Meeting List → Detail Panel
- **Meeting List**: карточки с ID (моно), датой, длительностью, языком, статусом (✓/○), счётчиком артефактов
- **Detail Panel**: Tab Bar (Live/Final/Speakers/Artifacts) + контент
- **Final Transcript**: meta-pills (календарь, микрофон) + строки с speaker badge (accent-bg) + таймкоды (моно, disabled)
- **Actions**: Copy All (Secondary), Export .md (Secondary), Generate Brief (Primary)

### 4.7 Brief Generator
**Назначение**: AI-генерация саммари встречи.
- **Шаблоны**: Brief, Follow-up, Minutes, Action Items — карточки с иконками и описанием
- **LLM Settings**: выбор engine (Ollama · gemma2) + язык (RU)
- **Кнопки**: ⚡ Generate Brief (Primary), Regenerate (Secondary)
- **Preview**: Brief Title + Date (моно) + секции Attendees, Key Decisions, Action Items
- **Actions**: Copy, Export, Share (иконки)

### 4.8 Brief Templates
**Назначение**: Редактор шаблонов для генерации брифа.
- **Список шаблонов**: встроенные (Brief, Follow-up) + кастомные (Technical Spec, Sprint Review, Client Summary)
- **Редактор**: markdown с подсветкой переменных `{{placeholder}}`
- **Палитра переменных** (12 шт): meeting_title, date, duration, speakers, transcript, brief_summary, decisions, action_items, next_date, next_agenda, glossary_terms, key_quotes
- **Test with meeting**: прогон шаблона на реальной встрече

### 4.9 Speakers & Diarization
**Назначение**: Управление спикерами встречи.
- **Diarization Bar**: статус "Coming soon" (warning)
- **Список спикеров**: аватар (цветной круг с инициалом), имя, роль, speaking time, segments count, кнопка Edit
- **Статистика**: Speaking Time с цветными точками
- **Unknown Speaker**: неразмеченные сегменты

### 4.10 Speaker Identification
**Назначение**: Обучение системы распознаванию спикеров по голосу.
- **Прогресс**: "3 of 8 clips" (accent badge)
- **Вопрос**: "Who is speaking?" + контекст (clip duration, confidence)
- **Waveform Player**: визуализация аудио, progress bar, кнопки Play/Replay
- **Best Match**: карточка с аватаром, именем, процентом совпадения (accent-bg)
- **New Speaker**: поле ввода для нового имени
- **Confirm/Skip**: кнопки действия
- **Sidebar**: Known Speakers (с количеством голосовых клипов) + How it works (4 шага) + When Done

### 4.11 Audio & Devices
**Назначение**: Настройка аудио-устройств.
- **Input Device**: MacBook Pro Microphone, 48 kHz, 2 channels, level meter (24 полосы), кнопка Change
- **Audio Sources**: Microphone (✅ Active), System Audio (✅ Available), Both Mixed (🔒 Pro feature)
- **Quick Test**: "Speak to test" + большой level meter (40 полос) + Start Test

### 4.12 Glossary
**Назначение**: Управление терминами глоссария.
- **Toolbar**: поиск + Add Term (Primary)
- **Sidebar**: фильтры All (42), RU (28), EN (10), ES (4), Meeting Scope (15)
- **Список терминов**: surface (моно) → canonical + язык (чип) + scope (чип) + Edit (иконка)
- **Scope**: Global (default) / Meeting (warning)

### 4.13 Export
**Назначение**: Экспорт транскрипта в разные форматы.
- **Форматы**: Markdown (.md), SubRip (.srt), WebVTT (.vtt), Plain Text (.txt)
- **Опции**: Include timestamps, Include speakers, Merge segments, Remove filler words
- **Preview**: рендер markdown с подсветкой в monospace
- **Export Btn**: Primary кнопка экспорта

### 4.14 Quick Note
**Назначение**: Быстрая заметка без записи звонка.
- **Meta**: поле Title, Date Picker, Attendees (чипсы с крестиком + Add)
- **Editor**: markdown с курсором, структура: Agenda → Notes → Action Items
- **Quick Actions**: Record Voice Memo, Generate Brief, Extract Actions, Translate Note, Share as PDF
- **Note History**: 3 последние заметки

### 4.15 Settings
**Назначение**: Настройки приложения.
- **Sidebar**: General, Audio, STT Engine, Translation, LLM Providers, Backend API, Data & Storage
- **General**: Session Language (Picker) + Allowed (чипсы RU/EN/ES) + Theme (Dark/Light/Auto) + Accent Color (6 цветов) + About (Version, Stack, Whisper)

### 4.16 Design System
**Назначение**: Библиотека компонентов и токенов.
- Color Palette: все 5 групп цветов с hex-кодами
- Typography Scale: 8 уровней + Mono
- Spacing & Radius: визуальные шкалы
- Elevation: None / Subtle / Moderate
- Buttons: 5 вариантов + 3 размера
- Input, Badge, Toggle, Card, Tab Bar, Sidebar Item, Empty State, Alert, Avatar, Chip, Progress

---

## 5. SwiftUI Implementation Notes

### 5.1 Transparent Overlay Window
```swift
let window = NSWindow(
    contentRect: NSRect(x: 0, y: 0, width: 900, height: 120),
    styleMask: [.borderless, .resizable],
    backing: .buffered, defer: false
)
window.isOpaque = false
window.backgroundColor = .clear
window.level = .floating
window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
window.isMovableByWindowBackground = true
window.contentView = NSHostingView(rootView: SubtitlesOverlayView())
```

### 5.2 Menu Bar App
```swift
// Info.plist: LSUIElement = YES
// AppDelegate: NSStatusBar.system.statusItem
// Meeting detection: NSWorkspace.shared.notificationCenter
//   + runningApplications + bundle IDs for Zoom/Teams/Meet/Webex/Discord
```

### 5.3 Window Materials
- Sidebar: `.ultraThinMaterial`
- Panels: `.regularMaterial`
- Overlay: `.hudWindow` via `NSVisualEffectView`

### 5.4 Трёхколоночный Meetings
```swift
NavigationSplitView {
    SidebarGroups()
} content: {
    MeetingList()
} detail: {
    MeetingDetail()
}
```

---

## 6. Карта экранов

```
                    ┌─ Welcome / Onboarding ─┐
                    │    (первый запуск)      │
                    └──────────┬──────────────┘
                               ▼
              ┌────────────────────────────────┐
              │     Menu Bar (tray icon)       │
              │  Idle → Detected → Recording   │
              └────────────┬───────────────────┘
                           ▼
              ┌────────────────────────┐
              │  Meeting Detected Popup │
              │   [Start Recording]     │
              └────────────┬───────────┘
                           ▼
         ┌─────────────────┴─────────────────┐
         ▼                                    ▼
  ┌──────────────┐                  ┌────────────────────┐
  │ Live Captions│◄──── перекл. ───│ Transparent Overlay │
  │  (основной)  │                  │   (поверх всех)     │
  └──────┬───────┘                  └────────────────────┘
         │ Stop
         ▼
  ┌──────────────────────────────────────────────────────┐
  │                    Meetings                          │
  │  Live ─── Final ─── Speakers ─── Artifacts          │
  └────┬─────────────┬─────────────┬────────────────────┘
       ▼             ▼             ▼
  ┌──────────┐ ┌────────────┐ ┌──────────┐
  │  Brief   │ │  Speakers  │ │  Export  │
  │ Generator│ │Identification│ │ .md/.srt │
  └────┬─────┘ └────────────┘ └──────────┘
       ▼
  ┌────────────┐
  │  Templates │
  │  (editor)  │
  └────────────┘

  Вспомогательные:
  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
  │ Calendar │ │ Glossary │ │  Audio   │ │ Quick    │
  │ & Today  │ │          │ │ & Devices│ │  Note    │
  └──────────┘ └──────────┘ └──────────┘ └──────────┘
  ┌──────────┐
  │ Settings │
  └──────────┘
```

---

## 7. Чеклист реализации

### Phase A: Shell + Design System
- [ ] App shell с дизайн-системой (цвета, шрифты, отступы)
- [ ] 5 кнопок как переиспользуемые компоненты
- [ ] `.ultraThinMaterial` sidebar во всех экранах

### Phase B: Основные экраны
- [ ] Welcome / Onboarding (3 шага)
- [ ] Live Captions (субтитры + audio meter + контролы)
- [ ] Transparent Overlay (NSWindow floating)
- [ ] Meetings (трёхколоночный)

### Phase C: Menu Bar + Календарь
- [ ] Menu Bar tray icon (4 состояния)
- [ ] Meeting Detection Popup
- [ ] Calendar & Today's Meetings (timeline)

### Phase D: Post-call
- [ ] Brief Generator (шаблоны + LLM)
- [ ] Brief Templates (редактор)
- [ ] Speakers & Diarization
- [ ] Speaker Identification (voice clips)
- [ ] Export (.md/.srt/.vtt/.txt)

### Phase E: Вспомогательные
- [ ] Glossary (термины + поиск)
- [ ] Audio & Devices
- [ ] Quick Note
- [ ] Settings (все вкладки)
