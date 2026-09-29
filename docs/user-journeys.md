# MeetingRaft — Клиентские пути (User Journeys) и UX-анализ

Версия 1.0 · К макетам `mac_design.pen`

---

## Journey 1: Первый запуск (First Launch)

```
Пользователь впервые открывает MeetingRaft
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Welcome / Onboarding       │ ← NNi1S
    │  ① Выбор языка              │
    │  ② Скачать модель Whisper   │
    │  ③ Начать первую встречу    │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Settings (General)         │ ← Pz05o (нужен переход)
    │  Выбор языка (RU/EN/ES)     │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Settings (STT Engine)      │ ← Pz05o (нужен переход)
    │  Скачать ggml модель        │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Audio & Devices            │ ← kdTkA (нужен переход)
    │  Проверить микрофон         │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Live Captions              │ ← Qjy4j
    │  Первая запись              │
    └─────────────────────────────┘
```

**Оценка UX**: 🟡 Хорошо, но нет связанных переходов между экранами
- ✅ Welcome с 3 шагами
- ⚠️ Нет wizard-флоу с авто-переходом Settings → Audio → Live
- ❌ Нет индикатора прогресса загрузки модели Whisper

**Что добавить**:
1. Кнопку «Continue» на каждом шаге Welcome
2. Прогресс-бар загрузки модели (эпик 6)
3. После настройки — авто-переход на Live Captions с подсказкой «Click Start Live»


## Journey 2: Живая встреча с записью (Live Meeting)

```
Пользователь начинает встречу в Zoom/Teams
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Menu Bar — Meeting Detected│ ← EMlyN
    │  Popup: «Zoom meeting»      │
    │  [Start Recording]  [✕]     │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Live Captions              │ ← Qjy4j
    │  • Субтитры в реальном      │
    │    времени                  │
    │  • Audio level meter        │
    │  • Глоссарий активен        │
    │  • Пауза / Стоп             │
    └─────────────┬───────────────┘
         │                │
         ▼                ▼
    ┌─────────┐    ┌──────────────────┐
    │Pause    │    │ Transparent      │ ← XG9JU
    │         │    │ Overlay (⌘⇧O)   │
    └─────────┘    │ • Compact/Medium │
                   │ • Full с футером │
                   │ • Drag anywhere  │
                   └──────────────────┘
         │                │
         └───────┬────────┘
                 ▼
    ┌─────────────────────────────┐
    │  Stop Recording             │
    │  → Meetings (авто-переход)  │
    └─────────────────────────────┘
```

**Оценка UX**: 🟢 Отлично
- ✅ Авто-детект встречи через Menu Bar
- ✅ Два режима просмотра (основной + overlay)
- ✅ Audio meter, статус STT, глоссарий
- ⚠️ Нет индикатора состояния сети/Whisper (только текст в status bar)
- ⚠️ Нет visual feedback при нажатии Pause (элемент не меняется)

**Что добавить**:
1. Breathing-анимацию красной точки REC
2. Hover-эффекты на кнопках Pause/Stop
3. Анимированный переход из основного окна в overlay и обратно


## Journey 3: Календарный сценарий (Calendar-Driven)

```
Пользователь импортирует календарь
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Calendar & Today           │ ← bliH7
    │  • Подключить источники     │
    │    (macOS/Google/Outlook)   │
    │  • Timeline встреч на       │
    │    сегодня                  │
    │  • Правила авто-записи      │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Up Next → Join & Record    │ ← bliH7
    │  или авто-запись по правилам│
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Live Captions              │ ← Qjy4j
    └─────────────────────────────┘
```

**Оценка UX**: 🟢 Отлично
- ✅ Три источника календаря
- ✅ Timeline с цветовой маркировкой
- ✅ Правила авто-записи
- ✅ Up Next с кнопкой Join & Record
- ⚠️ Календарь не встроен в основной sidebar — нужно переключаться

**Что добавить**:
1. Calendar как пункт в sidebar (сейчас его нет в основном shell)
2. Badge в tray иконке: «следующая встреча через N мин»


## Journey 4: Пост-обработка встречи (Post-call)

```
Встреча завершена, запись остановлена
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Meetings → встреча         │ ← Ut1lG
    │  • Live транскрипт          │
    │  • Final транскрипт         │
    │  • Вкладка Speakers         │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Speaker Identification     │ ← zCHsU  🆕
    │  • 8 голосовых клипов       │
    │  • Подтверждение/новый      │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Brief Generator            │ ← zdfmk
    │  • Выбор шаблона            │
    │  • LLM engine               │
    │  • ⚡ Generate Brief         │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Preview: Brief / Follow-up │ ← zdfmk
    │  • Attendees                │
    │  • Key Decisions            │
    │  • Action Items             │
    │  [Copy] [Export] [Share]    │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Export                     │ ← j1TMO
    │  • .md / .srt / .vtt / .txt │
    │  • Опции форматирования     │
    └─────────────────────────────┘
```

**Оценка UX**: 🟢 Отлично
- ✅ Полный путь: транскрипт → спикеры → бриаф → экспорт
- ✅ Предпросмотр брифа перед экспортом
- ✅ Разные форматы экспорта
- ⚠️ Speaker Identification пока «Coming soon» (ждёт diarization engine)
- ⚠️ Нет кнопки «Copy to clipboard» на экране Meetings (есть только в Brief Generator)

**Что добавить**:
1. Прогресс-бар обработки diarization
2. Кнопку Copy на экране Final Transcript
3. Быстрый экспорт из Meetings без перехода в Export


## Journey 5: Заметка без записи (Quick Note)

```
Пользователь хочет записать мысли без звонка
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Quick Note                 │ ← EvJf5 🆕
    │  • Title, Date, Attendees   │
    │  • Markdown редактор        │
    │  • Agenda → Notes → Actions │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Quick Actions:             │
    │  🎤 Voice Memo              │
    │  ⚡ Generate Brief ←───────→│ zdfmk
    │  ✅ Extract Actions         │
    │  🌐 Translate               │
    │  📤 Share as PDF            │
    └─────────────────────────────┘
```

**Оценка UX**: 🟡 Хорошо, но требует доработок
- ✅ Markdown редактор с курсором
- ✅ Quick Actions палитра
- ✅ Note History
- ⚠️ Нет авто-сохранения (черновик)
- ⚠️ Voice Memo — отдельная фича, не проработана в дизайне
- ⚠️ Share as PDF — нет экрана предпросмотра PDF

**Что добавить**:
1. Авто-сохранение черновика
2. Экран записи Voice Memo (мини-рекордер)
3. Preview PDF перед Share


## Journey 6: Глоссарий (Glossary Management)

```
Пользователь настраивает термины
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Glossary                   │ ← H9TlhC
    │  • Поиск по терминам        │
    │  • Фильтр по языку          │
    │  • CRUD терминов            │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Импорт CSV                 │
    │  • surface, canonical,      │
    │    language, scope          │
    └─────────────────────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Live Captions              │
    │  • Термины применяются      │
    │    в реальном времени       │
    │  • Status bar: «42 terms    │
    │    active»                  │
    └─────────────────────────────┘
```

**Оценка UX**: 🟢 Хорошо
- ✅ Поиск, фильтры, CRUD
- ✅ Индикатор активных терминов в Live Captions
- ✅ Разные scope (Global/Meeting)
- ⚠️ Нет Drag & Drop CSV
- ⚠️ Нет импорта в xlsx/Google Sheets формате
- ⚠️ Нет подсказки формата CSV файла

**Что добавить**:
1. Drop zone для CSV
2. Кнопку «Download template CSV»


## Journey 7: Настройка шаблонов брифа (Template Customization)

```
Пользователь хочет свой формат брифа
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Brief Templates            │ ← Mg10U 🆕
    │  • Список шаблонов          │
    │  • [+ New Template]         │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Редактор шаблона           │
    │  • Markdown + {{variables}} │
    │  • Палитра placeholders     │
    │  • [Test with meeting]      │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Brief Generator            │
    │  • Выбор кастомного шаблона │
    │  • Generate с новым форматом│
    └─────────────────────────────┘
```

**Оценка UX**: 🟢 Отлично
- ✅ Встроенные + кастомные шаблоны
- ✅ Markdown редактор с подсветкой переменных
- ✅ Палитра placeholders с описанием
- ✅ Test with meeting


## Journey 8: Настройки (Configuration)

```
                    ┌──────────────────┐
                    │   Settings       │ ← Pz05o
                    │   Sidebar:       │
                    │   • General      │
                    │   • Audio        │
                    │   • STT Engine   │
                    │   • Translation  │
                    │   • LLM Providers│
                    │   • Backend API  │
                    │   • Data/Storage │
                    └──────────────────┘
```

**Оценка UX**: 🟡 Хорошо, но экран только один
- ✅ 7 категорий в sidebar
- ✅ Чёткая организация по секциям
- ⚠️ Нет экранов для Translation, LLM Providers, Backend API, Data/Storage
- ⚠️ Текущий Settings показывает только General

**Что добавить**:
1. Экран Translation — выбор backend, target language
2. Экран LLM Providers — Ollama/OpenAI/Backend, model id, base URL
3. Экран Backend API — URL, token, Test API
4. Экран Data — размер базы, очистка, экспорт всех данных


## Journey 9: Menu Bar (Background Agent)

```
Пользователь не открывает основное окно
                    │
                    ▼
    ┌─────────────────────────────┐
    │  Menu Bar Icon (tray)       │ ← EMlyN
    │  Состояния:                 │
    │  🌊 Idle (нет встреч)       │
    │  📹 Meeting detected        │
    │  🔴 Recording               │
    │  📄 Post-call               │
    └─────────────┬───────────────┘
                  ▼
    ┌─────────────────────────────┐
    │  Dropdown Menu              │
    │  • Start Live Captions ⌘R   │
    │  • Open Meetings     ⌘M     │
    │  • Glossary          ⌘G     │
    │  • Transparent Overlay ⌘⇧O │
    │  • Preferences...    ⌘,     │
    │  • Quit              ⌘Q     │
    └─────────────────────────────┘
```

**Оценка UX**: 🟢 Отлично
- ✅ LSUIElement (background app)
- ✅ 4 состояния иконки
- ✅ Полное меню с shortcuts
- ✅ Popup при детекте встречи


## Journey 10: Экспорт (Export Flow)

```
                    ┌──────────────────┐
                    │   Export         │ ← j1TMO
                    │   • Markdown .md │
                    │   • SubRip  .srt │
                    │   • WebVTT  .vtt │
                    │   • Plain    .txt│
                    │   • Опции        │
                    │   • Preview      │
                    │   [Export .md]   │
                    └──────────────────┘
```

**Оценка UX**: 🟢 Хорошо
- ✅ 4 формата
- ✅ Preview рендеринг
- ✅ Опции форматирования
- ⚠️ Нет выбора пути сохранения в дизайне
- ⚠️ Нет PDF экспорта


---

## Сводная таблица покрытия

| Journey | Экраны | Полнота | Качество UX |
|---------|--------|---------|-------------|
| 1. First Launch | Welcome, Settings, Audio, Live | 🟡 80% | Нет wizard flow |
| 2. Live Meeting | Menu Bar, Live, Overlay | 🟢 95% | Отлично |
| 3. Calendar | Calendar, Live | 🟢 90% | Нет в sidebar |
| 4. Post-call | Meetings, Speakers, Brief, Export | 🟢 95% | Отлично |
| 5. Quick Note | Quick Note, Brief Generator | 🟡 75% | Нет авто-сохранения |
| 6. Glossary | Glossary | 🟢 85% | Нет drag-drop CSV |
| 7. Templates | Templates, Brief Generator | 🟢 90% | Хорошо |
| 8. Settings | Settings (General) | 🟡 60% | Только General экран |
| 9. Menu Bar | Menu Bar & Detection | 🟢 95% | Отлично |
| 10. Export | Export | 🟢 85% | Нет PDF |

**Итого**: 85% покрытия, 4 области для улучшения.


## 🔴 Критические gaps

| Gap | Проблема | Решение |
|-----|----------|---------|
| **Нет wizard-флоу** | Welcome не ведёт пользователя по шагам | Экран «Setup Wizard» с авто-переходами Settings → Audio → Live |
| **Settings incomplete** | Только General; нет Translation, LLM, API, Data | Дорисовать 4 недостающих экрана Settings |
| **Quick Note bare** | Нет авто-сохранения, Voice Memo | Доработать Quick Note: draft saving, mini-recorder |
| **Нет PDF** | Экспорт только в текст | Добавить PDF в Export и Share as PDF в Quick Note |
| **Calendar в sidebar** | Нет в основном shell | Добавить Calendar в AppDestination + SidebarView |


## 🟡 Желательные улучшения

| Улучшение | Эффект |
|-----------|--------|
| Breathing анимация REC dot | Статус записи очевиден |
| Hover-эффекты на кнопках | Тактильный отклик |
| Анимированный переход Live ↔ Overlay | Плавный UX |
| Badge в tray: «встреча через N мин» | Проактивность |
| Прогресс-бар diarization | Прозрачность процесса |
| Copy to clipboard в транскрипте | Быстрый доступ |
| Drop zone для CSV в Glossary | Удобство импорта |
| Auto-save черновика в Quick Note | Защита от потери |
