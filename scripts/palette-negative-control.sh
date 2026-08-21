#!/usr/bin/env bash
# Отрицательный контроль палитры (Epic 23).
#
# Запуск из корня репо, только на Mac: scripts/palette-negative-control.sh
#
# ЗАЧЕМ. `ThemeContrastTests` зелёный. Сам по себе это не значит ничего:
# зелёным он был бы и если бы порог стоял на нуле, и если бы список
# проверяемых цветов оказался пуст. Довод даёт только показанное падение
# на заведомо плохой палитре — и с теми числами, которые посчитаны
# заранее, а не подогнаны под вывод.
#
# ЧТО ДЕЛАЕТ. Временно подменяет пять светлых значений на статусы тёмной
# темы, перенесённые как есть, гоняет тесты контраста и требует:
#
#   * светлая палитра падает ровно на пяти цветах;
#   * тёмная при этом остаётся зелёной — иначе тест ловит не тему;
#   * в выводе есть посчитанные заранее числа.
#
# Палитра возвращается на место всегда, в том числе по Ctrl-C и по любой
# ошибке: подмена, забытая в рабочем дереве, хуже непроверенного теста.
#
# ЧЕГО НЕ ДЕЛАЕТ. Не судит красоту и не проверяет остальные тесты. Это
# один вопрос: умеет ли прибор краснеть.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

THEME="apps/macos/Sources/DesignSystem/Theme.swift"
TESTS="MeetingRaftTests/ThemeContrastTests"

# Ожидаемые числа. Посчитаны до написания теста и лежат в спеке
# `docs/superpowers/specs/2026-08-20-light-theme-and-honest-ui-design.md`.
EXPECTED_TOKENS=(textTertiary success warning error info)
EXPECTED_RATIOS=(2.99 2.02 1.41 3.41 1.72)

fail() {
    printf '\n\033[1;31m%s\033[0m\n' "$1"
    exit 1
}

ok() {
    printf '  \033[32m✓\033[0m %s\n' "$1"
}

# Подмена в рабочем дереве. Если файл уже изменён, отличить своё от чужого
# нельзя, и откат затёр бы чужую правку.
if ! git diff --quiet -- "$THEME" || ! git diff --cached --quiet -- "$THEME"; then
    fail "В $THEME есть незакоммиченные правки. Отложите их: скрипт правит этот файл и откатывает его целиком."
fi

restore() {
    git checkout -- "$THEME"
    printf '\n  палитра возвращена на место\n'
}
trap restore EXIT

printf '\033[1m==> Подмена: статусы тёмной темы переносятся в светлую как есть\033[0m\n'

python3 - "$THEME" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
source = path.read_text(encoding="utf-8")

# Ровно та палитра, которая вышла бы при переносе тёмных статусов в
# светлую тему без счёта. Пять значений, пять ожидаемых провалов.
naive = {
    "textTertiary": ("0x7C7C82", "0x8E8E93"),
    "success": ("0x1D7A32", "0x30D158"),
    "warning": ("0xB25000", "0xFFD60A"),
    "error": ("0xD70015", "0xFF453A"),
    "info": ("0x0071A4", "0x64D2FF"),
}

for token, (good, bad) in naive.items():
    needle = f"static let {token} = dynamic(light: {good},"
    if needle not in source:
        sys.exit(f"не найдено светлое значение токена {token} ({good}) — палитра изменилась, скрипт устарел")
    source = source.replace(needle, f"static let {token} = dynamic(light: {bad},", 1)

path.write_text(source, encoding="utf-8")
print("  подменено токенов: 5")
PY

printf '\n\033[1m==> Прогон тестов контраста\033[0m\n'

OUTPUT="$(mktemp)"
trap 'rm -f "$OUTPUT"; restore' EXIT

set +e
(cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
    -only-testing:"$TESTS" test CODE_SIGNING_ALLOWED=NO) >"$OUTPUT" 2>&1
set -e

printf '\n\033[1m==> Разбор\033[0m\n'

# Тёмная палитра не менялась и обязана остаться зелёной. Её падение
# означало бы, что тест реагирует не на тему, а на что-то ещё.
if grep -q "testDarkPaletteMeetsContrastFloors.*failed" "$OUTPUT"; then
    grep -E "testDarkPaletteMeetsContrastFloors|NSAppearanceNameDarkAqua" "$OUTPUT" | head -20
    fail "Тёмная палитра тоже покраснела. Тест ловит не тему — разбираться надо с ним, а не с палитрой."
fi
ok "тёмная палитра осталась зелёной"

if ! grep -q "testLightPaletteMeetsContrastFloors.*failed" "$OUTPUT"; then
    fail "Светлая палитра НЕ покраснела на заведомо плохих значениях. Тест контраста не умеет падать — верить его зелёному нельзя."
fi
ok "светлая палитра покраснела"

FAILED_TOKENS="$(grep -oE "NSAppearanceNameAqua: [a-zA-Z]+ на" "$OUTPUT" | awk '{print $2}' | sort -u)"
COUNT="$(printf '%s\n' "$FAILED_TOKENS" | grep -c . || true)"

printf '\n  покрасневшие токены (%s):\n' "$COUNT"
printf '%s\n' "$FAILED_TOKENS" | sed 's/^/    /'

MISSING=""
for token in "${EXPECTED_TOKENS[@]}"; do
    printf '%s\n' "$FAILED_TOKENS" | grep -qx "$token" || MISSING="$MISSING $token"
done
[ -z "$MISSING" ] || fail "Ожидались провалы у:${MISSING} — их нет. Либо палитра изменилась, либо тест проверяет не то."
ok "покраснели все пять ожидаемых токенов"

[ "$COUNT" -eq 5 ] || fail "Покрасневших токенов $COUNT, а ожидалось 5. Лишний провал — такой же сигнал, как недостающий."
ok "лишних провалов нет"

printf '\n  посчитанные заранее числа:\n'
MISSING_RATIOS=""
for ratio in "${EXPECTED_RATIOS[@]}"; do
    if grep -q "даёт $ratio:1" "$OUTPUT"; then
        printf '    \033[32m✓\033[0m %s\n' "$ratio"
    else
        printf '    \033[31m✗\033[0m %s — в выводе нет\n' "$ratio"
        MISSING_RATIOS="$MISSING_RATIOS $ratio"
    fi
done
[ -z "$MISSING_RATIOS" ] || fail "Числа разошлись с посчитанными:${MISSING_RATIOS}. Расчёт спеки и поведение кода не совпадают — это отдельный разбор."
ok "все пять чисел совпали с посчитанными до написания теста"

printf '\n\033[1;32mПрибор умеет краснеть. Зелёному ThemeContrastTests теперь есть основание верить.\033[0m\n'
