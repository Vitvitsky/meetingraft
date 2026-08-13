#!/usr/bin/env bash
# Разбор замера подъёма захвата по шагам (Epic 25, задача 3).
#
# Зачем: системный канал начинается позже микрофонного — на встрече
# `6CE19EC5` на 1150 мс, — и эта секунда не только смещение, а потерянная
# запись: первых слов созвона в `system` нет вовсе. Какой шаг её съедает,
# из кода не видно: `AudioHardwareCreateProcessTap`, сборка aggregate и
# `AudioDeviceStart` уходят в `coreaudiod`.
#
# Приложение пишет цену каждого шага в журнал диагностики; этот скрипт
# складывает её в таблицу за последний запуск записи.
#
# Пустой вывод здесь невозможен: если строк нет, скрипт говорит об этом и
# возвращает ошибку. Прибор, молчащий на пустых данных, читается как
# «дорогих шагов нет» — ровно та ошибка, которой стоит правило про
# заведомо положительный случай (`CLAUDE.md`).
#
# Запуск:
#   scripts/capture-start-breakdown.sh                       # журнал по умолчанию
#   scripts/capture-start-breakdown.sh путь/к/diagnostics.jsonl
set -euo pipefail

DEFAULT_LOG="$HOME/Library/Application Support/meetingraft/diagnostics.jsonl"
LOG="${1:-$DEFAULT_LOG}"

if [[ ! -f "$LOG" ]]; then
    echo "журнала нет: $LOG" >&2
    echo "он появляется после первой записи; диагностика включена по умолчанию." >&2
    exit 1
fi

python3 - "$LOG" <<'PY'
import json, sys

path = sys.argv[1]
rows = []
with open(path, encoding="utf-8") as handle:
    for line in handle:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("kind") in {
            "capture_start_step",
            "capture_channel_start",
            "capture_channel_skew",
        }:
            rows.append(record)

if not rows:
    sys.exit(
        "в журнале нет ни одной строки про подъём захвата.\n"
        "Это отказ, а не «дорогих шагов нет»: запись либо не начиналась\n"
        "после установки этой сборки, либо диагностика выключена в настройках."
    )

# Последний запуск: всё от последнего `session_open` и дальше. Он же
# первый шаг цепочки, поэтому годится за её границу.
starts = [i for i, r in enumerate(rows) if r.get("text") == "session_open"]
run = rows[starts[-1]:] if starts else rows
if not starts:
    print("! строки `session_open` нет — показано всё, что нашлось в журнале\n")

steps = {
    r["text"]: r["buffer_ms"] for r in run if r["kind"] == "capture_start_step"
}
if not steps:
    sys.exit("строки про шаги подъёма есть только про часы каналов — мерить нечего")

# Шаги вложены: `mic_start` содержит все `mic:*`, кроме ожидания буфера.
# Складывать их в один список нельзя — цена посчиталась бы дважды.
OUTER = ["session_open", "system_prepare", "mic_start", "system_start"]
inner = {
    "mic_start": [k for k in steps if k.startswith("mic:") and k != "mic:first_buffer"],
    "system_start": [
        k for k in steps if k.startswith("system:") and k != "system:first_buffer"
    ],
}

print("Путь от нажатия до первого звука, мс\n")
print("  общее для обоих каналов")
shared = 0
for name in ["session_open", "system_prepare", "mic_start"]:
    if name in steps:
        shared += steps[name]
        print(f"    {steps[name]:>5}  {name}")
mic_wait = steps.get("mic:first_buffer", 0)
print(f"    -> микрофон запел через {shared + mic_wait} мс")

print("\n  только системный канал")
system_only = 0
for name in ["system_start", "system:first_buffer"]:
    if name in steps:
        system_only += steps[name]
        print(f"    {steps[name]:>5}  {name}")
print(f"    -> система запела через {shared + system_only} мс")

# Главная строка отчёта. Она отвечает на вопрос задачи 3 не «что дорого»,
# а «объясняют ли наши отметки разницу вообще».
by_steps = system_only - mic_wait
print(f"\n  Разница стартов по шагам: {by_steps} мс")
skew = next(
    (r["buffer_ms"] for r in run if r["kind"] == "capture_channel_skew"),
    None,
)
if skew is None:
    print("  Разница стартов по часам: строки нет — записывался один канал?")
else:
    print(f"  Разница стартов по часам:  {skew} мс")
    gap = abs(skew - by_steps)
    if gap > max(50, skew // 10):
        print(
            f"  ! не сходится на {gap} мс. Значит время уходит там, где отметки нет\n"
            f"    вовсе, и следующий замер надо ставить туда, а не спорить о причинах."
        )
    else:
        print("  сходится: разница стартов разложена по шагам целиком")

for parent, children in inner.items():
    if parent not in steps or not children:
        continue
    print(f"\n  Внутри {parent} ({steps[parent]} мс)")
    for name in children:
        share = steps[name] * 4 >= steps[parent] and steps[parent] > 0
        print(f"    {steps[name]:>5}  {name}{'  <-' if share else ''}")
    inside = sum(steps[name] for name in children)
    if abs(inside - steps[parent]) > max(5, steps[parent] // 10):
        print(f"    ! части дают {inside} мс, а шаг целиком {steps[parent]} мс")

print("\nЧитать так:")
print("  «<-» — шаг съедает четверть своего родителя и больше.")
print("  0 мс — «меньше миллисекунды», то есть шаг ни при чём.")
print("  Переставлять порядок подъёма источников стоит только после того,")
print("  как разница стартов сошлась с шагами: иначе переставим не то.")
PY
