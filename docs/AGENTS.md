# AGENTS.md — docs

Как устроено дерево документации и как в нём работать. Таблица «где что
искать» и порядок «спека → план → выполнение» — в корневом `AGENTS.md`;
здесь структура и правила.

## Дерево

- `adr/` — Architecture Decision Records, имена `ADR-NNN-<slug>.md`,
  001–014. **Неизменяемы**: заменяются новым ADR, не правкой. Индекса и
  README нет.
- `superpowers/specs/` — спеки-проектирования, имя
  `YYYY-MM-DD-<name>-design.md`. Пишутся **до** планов.
- `superpowers/plans/` — планы задач по TDD, имя `YYYY-MM-DD-<name>.md`.
  Суффикс `-design` отличает спеку от плана — не путать.
- `windows/` — онбординг Windows-клиента: `README.md` (начало),
  `core-interaction.md` (шелл↔ядро), `platform-contract.md` (контракт
  платформенного слоя).

Ключевые файлы корня: `backlog.md` (единственный источник открытых работ
и состояния «ждёт проверки на Маке»), `roadmap.md` (фазы и критерии
выхода), `architecture.md` (архитектура, двухстадийность, бюджеты
задержки), `architecture-and-install.md` (онбординг; настройка backend —
§2.5 / `#backend-setup`), `mac-verification.md` (ручные сценарии за Маком;
разделы дописываются в конец — сейчас последний про Silero VAD против
гейта), `design-spec.md` (спека UI v1.0,
ссылается на `mac_design.pen`), `naming.md` (продукт MeetingRaft против
репозитория meetingraft), `user-journeys.md`. Плюс `analysis-pre-meeting-2026-08-03.md`,
`product-ux-review-2026-08-03.md`, `ui-redesign-macos-2026-08-03.md`,
`design-tz-mac-redesign.md`, `windows-client-brief.md`.

## Правила ADR

ADR не редактируется: новое решение — новый файл, старое помечается
заменённым. Язык заголовков расщеплён: ADR-001–008 английские,
ADR-009–014 русские.

## Язык

Язык задаётся **файлом**, а не каталогом: `roadmap.md`, `architecture.md`,
`naming.md` — английские; `backlog.md`, `mac-verification.md`,
`architecture-and-install.md` — русские. Общее правило репозитория —
проза в `docs/` по-русски, перечисленные файлы — исключения.

## Как читать

`backlog.md` и `mac-verification.md` — больше 100 КБ каждый: их грепают,
а не читают целиком. Бюджеты задержки — в `architecture.md`, раздел про
live caption latency.
