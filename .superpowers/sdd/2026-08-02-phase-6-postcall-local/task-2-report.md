# Task 2 Report: postcall assemble, templates and LLM stub

**Status:** done
**Branch:** `feat/phase-6-postcall-local`

## Commits

- `b860460` — `fix: выравнивает postcall-счётчики с design spec`
- `b0d0b71` — `feat: добавляет postcall assemble и builtin-шаблоны`

## What was done

1. Исправлены доменные типы: timestamps и `artifact_count` теперь соответствуют design spec (`u64`).
2. Добавлен workspace crate `meetingraft-postcall`.
3. Через RED/GREEN реализован `assemble_final`: берёт только final captions, применяет нормализацию и собирает markdown.
4. Реализованы детерминированные builtin-шаблоны Brief и Follow-up, включая summary до 280 символов, key points и эвристику next steps.
5. `make_artifact` назначает `builtin.brief` или `builtin.follow_up`.
6. Добавлены `LlmClient`, `LlmError::NotConfigured` и `NullLlmClient` как явная граница для будущих Ollama/LM Studio/Gemma.

## Verification

- `cargo test -p meetingraft-postcall`: 8 passed, 0 failed.
- `cargo test -p meetingraft-domain`: 4 passed, 0 failed.
- `cargo clippy -p meetingraft-postcall --all-targets -- -D warnings`: passed.
- `cargo fmt -p meetingraft-postcall --check`: passed.

## Concerns

- ID артефакта пока детерминирован из `meeting_id`, `template_id` и времени; persistence-слой может заменить это на UUID при появлении требований к повторной генерации в одну миллисекунду.
- `NullLlmClient` намеренно не выполняет fallback: подключение конкретного локального провайдера остаётся отдельной задачей.
