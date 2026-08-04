#!/usr/bin/env bash
# Замер энергопотребления MeetingRaft через powermetrics (Epic 18).
#
# Зачем: жалоба на нагрев есть, чисел нет. Пока их нет, любые разговоры
# про оптимизацию — догадки, включая мои.
#
# Сырой вывод powermetrics сохраняется всегда. Разбор может ошибиться —
# формат меняется от версии macOS, — и терять из-за этого сам замер
# нельзя: переразобрать файл можно, переснять сценарий вживую нельзя.
#
# Запуск:
#   scripts/measure-power.sh baseline-no-app
#   scripts/measure-power.sh idle-app
#   scripts/measure-power.sh recording-silence
#   scripts/measure-power.sh recording-monologue
#   scripts/measure-power.sh recording-dialogue
#
# Протокол — `--help`.
set -euo pipefail

DURATION=60
INTERVAL_MS=1000
PROCESS_NAME="MeetingRaft"
OUT_DIR="${MEETINGRAFT_POWER_DIR:-$HOME/meetingraft-power}"

usage() {
    cat <<'USAGE'
Замер энергопотребления MeetingRaft.

  scripts/measure-power.sh [-d СЕКУНДЫ] [-o КАТАЛОГ] <метка-сценария>

Опции:
  -d  длительность замера, секунд (по умолчанию 60)
  -o  каталог для результатов (по умолчанию ~/meetingraft-power)
  -h  эта справка

ПРОТОКОЛ

Порядок важен: без базы остальные числа не значат ничего.

  1. baseline-no-app       приложение закрыто, Zoom закрыт, ничего не
                           делаем 60 секунд
  2. baseline-zoom         Zoom в созвоне, MeetingRaft закрыт
  3. idle-app              MeetingRaft открыт, запись НЕ идёт
  4. recording-silence     запись идёт, все молчат
  5. recording-monologue   запись идёт, говорит один человек без пауз
  6. recording-dialogue    запись идёт, разговор с паузами
  7. rebuild-final         идёт пересбор Final (post-call, large-модель)

Между сценариями дайте машине минуту остыть: горячая машина показывает
другие числа при той же нагрузке.

Каждый сценарий снимайте дважды. Совпали в пределах 10% — верим.

ЧТО ВАЖНО ЗАФИКСИРОВАТЬ РЯДОМ

  - модель Мака и объём памяти
  - подключён ли он к питанию (на батарее система душит частоты)
  - какая STT-модель выбрана в Settings
  - включён ли MEETINGRAFT_STT_TIMING (тогда рядом будут и латентности)

ЧТЕНИЕ РЕЗУЛЬТАТА

  combined_mW      главное число: CPU + GPU + ANE
  ane_mW           ноль означает, что Neural Engine не используется —
                   ожидаемо для whisper.cpp на Metal, и это довод в
                   пользу ветки CoreML/WhisperKit, если энергия окажется
                   проблемой
  app_cpu_ms_per_s сколько миллисекунд процессорного времени в секунду
                   съедает само приложение
  thermal          Nominal / Fair / Serious — Serious означает, что
                   система уже сбрасывает частоты
USAGE
}

while getopts "d:o:h" option; do
    case "$option" in
        d) DURATION="$OPTARG" ;;
        o) OUT_DIR="$OPTARG" ;;
        h)
            usage
            exit 0
            ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
done
shift $((OPTIND - 1))

if [ $# -ne 1 ]; then
    usage >&2
    exit 2
fi
SCENARIO="$1"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "powermetrics есть только на macOS" >&2
    exit 1
fi
if ! command -v powermetrics >/dev/null 2>&1; then
    echo "powermetrics не найден" >&2
    exit 1
fi

# powermetrics читает счётчики ядра и требует root. Переподнимаемся сами,
# чтобы не заставлять человека вспоминать это посреди сценария.
if [ "$(id -u)" -ne 0 ]; then
    echo "Нужен root для powermetrics — запрашиваю sudo."
    exec sudo --preserve-env=MEETINGRAFT_POWER_DIR "$0" -d "$DURATION" -o "$OUT_DIR" "$SCENARIO"
fi

mkdir -p "$OUT_DIR"
STAMP="$(date +%Y%m%d-%H%M%S)"
RAW="$OUT_DIR/${STAMP}-${SCENARIO}.txt"
CSV="$OUT_DIR/measurements.csv"
SAMPLES=$((DURATION * 1000 / INTERVAL_MS))

echo "Сценарий: $SCENARIO"
echo "Длительность: ${DURATION}s (${SAMPLES} проб по ${INTERVAL_MS}ms)"
if pgrep -x "$PROCESS_NAME" >/dev/null 2>&1; then
    echo "Приложение: запущено (pid $(pgrep -x "$PROCESS_NAME" | head -1))"
else
    echo "Приложение: не запущено"
fi
echo "Пишу сырой вывод в $RAW"
echo

powermetrics \
    --samplers cpu_power,gpu_power,thermal,tasks \
    --show-process-energy \
    -i "$INTERVAL_MS" \
    -n "$SAMPLES" \
    >"$RAW" 2>/dev/null

# Среднее по пробам. Пустое значение вместо нуля, если поля нет:
# отсутствующий замер и нулевой замер — разные вещи.
average_field() {
    local pattern="$1"
    awk -v pattern="$pattern" '
        $0 ~ pattern {
            for (i = 1; i <= NF; i++) {
                if ($i ~ /^[0-9]+(\.[0-9]+)?$/) {
                    total += $i
                    count++
                    break
                }
            }
        }
        END { if (count > 0) printf "%.1f", total / count }
    ' "$RAW"
}

# Строка процесса в таблице энергии: имя, затем числовые колонки.
average_process_field() {
    local column="$1"
    awk -v name="$PROCESS_NAME" -v column="$column" '
        $1 == name {
            value = $column
            if (value ~ /^[0-9]+(\.[0-9]+)?$/) {
                total += value
                count++
            }
        }
        END { if (count > 0) printf "%.2f", total / count }
    ' "$RAW"
}

CPU_MW="$(average_field '^CPU Power')"
GPU_MW="$(average_field '^GPU Power')"
ANE_MW="$(average_field '^ANE Power')"
COMBINED_MW="$(average_field '^Combined Power')"
# Колонки 3 и 4 — CPU ms/s и user%; порядок менялся между версиями macOS,
# поэтому сверяйтесь с шапкой таблицы в сыром файле.
APP_CPU="$(average_process_field 3)"
THERMAL="$(grep -m1 -o 'pressure level: [A-Za-z]*' "$RAW" | awk '{print $3}' || true)"

if [ ! -f "$CSV" ]; then
    echo "timestamp,scenario,duration_s,samples,cpu_mW,gpu_mW,ane_mW,combined_mW,app_cpu_ms_per_s,thermal,raw_file" >"$CSV"
fi
printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$STAMP" "$SCENARIO" "$DURATION" "$SAMPLES" \
    "$CPU_MW" "$GPU_MW" "$ANE_MW" "$COMBINED_MW" \
    "$APP_CPU" "${THERMAL:-}" "$(basename "$RAW")" >>"$CSV"

# Файлы созданы из-под root — иначе они останутся недоступными на запись.
if [ -n "${SUDO_USER:-}" ]; then
    chown -R "$SUDO_USER" "$OUT_DIR" 2>/dev/null || true
fi

echo "Результат:"
printf '  CPU:      %s mW\n' "${CPU_MW:-нет данных}"
printf '  GPU:      %s mW\n' "${GPU_MW:-нет данных}"
printf '  ANE:      %s mW\n' "${ANE_MW:-нет данных}"
printf '  Всего:    %s mW\n' "${COMBINED_MW:-нет данных}"
printf '  %s: %s ms/s CPU\n' "$PROCESS_NAME" "${APP_CPU:-нет данных}"
printf '  Тепло:    %s\n' "${THERMAL:-нет данных}"
echo
echo "Сводка: $CSV"

if [ "${THERMAL:-}" = "Serious" ] || [ "${THERMAL:-}" = "Critical" ]; then
    echo
    echo "ВНИМАНИЕ: система сбрасывает частоты. Замер снят под троттлингом" >&2
    echo "и сравнивать его с холодными прогонами нельзя." >&2
fi
