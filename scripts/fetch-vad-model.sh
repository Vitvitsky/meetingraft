#!/usr/bin/env bash
# Скачать модель Silero VAD — детектор речи, обещанный ADR-005.
#
# Кладёт в <каталог-данных>/models/vad/silero_vad.onnx (~0.6 МБ).
#
# В живом пути этой модели пока нет вовсе: она нужна прибору
# `gate-probe`, который ставит VAD рядом с нынешним гейтом по энергии и
# печатает цену каждого. До этих чисел живой путь не трогается.
#
# Сеть трогает только этот скрипт.

set -euo pipefail

ROOT="${1:-}"
if [ -z "$ROOT" ]; then
    cat <<'USAGE'
Использование: scripts/fetch-vad-model.sh <каталог-данных>

Каталог данных — тот, где лежит meetingraft.sqlite3. На Маке это
  ~/Library/Application Support/meetingraft

Скачивается около 0.6 МБ. Кладётся в <каталог-данных>/models/vad/ — рядом с
моделями Whisper, GigaAM и разделения голосов.
USAGE
    exit 1
fi

DIR="$ROOT/models/vad"
mkdir -p "$DIR"

URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"
DEST="$DIR/silero_vad.onnx"
MARK="$DEST.source"
# Метка рядом с файлом — по тому же правилу, что у моделей разделения
# голосов: «файл уже есть» и «файл тот, который нужен» — разные
# утверждения, и первое молча выдаёт себя за второе (обожглись
# 2026-08-11).
WANT="$(basename "$URL")"

if [ -f "$DEST" ] && [ "$(cat "$MARK" 2>/dev/null)" = "$WANT" ]; then
    echo "  уже на месте: $(basename "$DEST") ($WANT)"
else
    echo "  качаю: $(basename "$DEST") ($WANT)"
    curl -fL --progress-bar -o "$DEST.part" "$URL"
    mv "$DEST.part" "$DEST"
    printf '%s' "$WANT" > "$MARK"
fi

echo
echo "Готово. Дальше — в каталоге rust:"
echo
echo "  cargo run --release -p meetingraft-gate-probe --features vad -- \"$ROOT\" <сессия>"
echo
echo "Прибор печатает обе колонки — гейт по фону и VAD — на одном"
echo "материале. Одна колонка без другой не значит ничего."
