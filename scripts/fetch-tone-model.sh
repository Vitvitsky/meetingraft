#!/usr/bin/env bash
# Скачать потоковый русский распознаватель T-one.
#
# Кладёт в <каталог-данных>/models/tone/:
#   model.onnx   — потоковый Conformer-CTC
#   tokens.txt   — русский алфавит
#   check/0.wav  — контрольная запись экспорта
#
# Чем он отличается от двух соседних движков: он **потоковый**. GigaAM и
# parakeet офлайновые — им подают кусок и ждут ответа целиком; этот
# принимает звук чанками, держит состояние между ними и сам говорит, где
# кончилась реплика. То есть у него нет оси нарезки: границы у него свои.
#
# **Модель телефонная.** T-one специализирован под 8 кГц телефонии, а у
# нас широкополосный звук встречи. Сколько он на этом теряет — ровно тот
# вопрос, ради которого он здесь; ответ даёт стенд, а не эта строка.
#
# Лицензия — Apache 2.0, T-Software DC. Проверена у самого файла.
#
# Сеть трогает только этот скрипт. Движок живёт за фичей `tone`,
# выключенной по умолчанию.

set -euo pipefail

ROOT="${1:-}"
if [ -z "$ROOT" ]; then
    cat <<'USAGE'
Использование: scripts/fetch-tone-model.sh <каталог-данных>

Каталог данных — тот, где лежит meetingraft.sqlite3. На Маке это
  ~/Library/Application Support/meetingraft

Кладётся всё в <каталог-данных>/models/tone/ — рядом с остальными
моделями. Ничего вне этого каталога скрипт не трогает.
USAGE
    exit 1
fi

DIR="$ROOT/models/tone"
CHECK="$DIR/check"
mkdir -p "$CHECK"

# Версия экспорта зашита явно: смена экспорта — это смена того, что
# мерится, и происходить она должна руками.
EXPORT="sherpa-onnx-streaming-t-one-russian-2025-09-08"
BASE="https://huggingface.co/csukuangfj/$EXPORT/resolve/main"

# Метка рядом с каждым файлом — та же дисциплина, что у соседних
# скриптов: без неё смена модели не доезжает ни до кого. Файл на месте,
# скрипт его пропускает, и человек продолжает мерить старой моделью.
fetch() {
    local url="$1" dest="$2"
    local want
    want="$EXPORT/$(basename "$url")"
    local mark="$dest.source"

    if [ -f "$dest" ]; then
        local have=""
        [ -f "$mark" ] && have="$(cat "$mark")"
        if [ "$have" = "$want" ]; then
            echo "  уже на месте: $(basename "$dest") ($want)"
            return
        fi
        if [ -z "$have" ]; then
            echo "  заменяю: $(basename "$dest") — неизвестно, что в нём, метки нет"
        else
            echo "  заменяю: $(basename "$dest") — было $have, нужно $want"
        fi
    else
        echo "  качаю: $(basename "$dest") ($want)"
    fi

    curl -fL --progress-bar -o "$dest.part" "$url"
    mv "$dest.part" "$dest"
    printf '%s' "$want" > "$mark"
}

echo "Модель -> $DIR"
fetch "$BASE/model.onnx" "$DIR/model.onnx"
fetch "$BASE/tokens.txt" "$DIR/tokens.txt"

echo "Контрольная запись -> $CHECK"
fetch "$BASE/0.wav" "$CHECK/0.wav"

echo "Лицензия -> $DIR"
fetch "$BASE/LICENSE" "$DIR/LICENSE"

cat <<EOF

Готово. Эталона к контрольной записи экспорта у нас нет — её текст
известен только из выхода самой модели, а сверять расшифровку с
расшифровкой значит сверять догадку саму с собой. Поэтому судится этот
движок на нашей русской контрольной записи, у которой эталон взят из
стихотворения:

  cd rust && cargo run --release -p meetingraft-bench --features tone -- \\
    run <каталог-случая> "$ROOT" tone stream
EOF
