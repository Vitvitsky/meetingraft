#!/usr/bin/env bash
# Скачать модели разделения голосов и контрольные записи к ним.
#
# Кладёт всё в <каталог-данных>/models/diarize/:
#   segmentation.onnx      — кто когда говорит (pyannote 3.0)
#   embedding.onnx         — насколько голоса похожи (CAM++ zh+en)
#   check/2-*.wav          — записи с известным числом людей
#   check/4-*.wav
#
# Контрольные записи не украшение. Прибор `diarize-probe` без них не
# судит движок вовсе — и это правило написано по случаю: первая версия
# проверяла движок синтетическими тонами и объявила работающий движок
# слепым, потому что модель голосов на тонах не работает.
#
# Сеть трогает только этот скрипт. Обычная сборка не качает ничего:
# движок живёт за фичей `model`, выключенной по умолчанию.

set -euo pipefail

ROOT="${1:-}"
if [ -z "$ROOT" ]; then
    cat <<'USAGE'
Использование: scripts/fetch-diarize-models.sh <каталог-данных>

Каталог данных — тот, где лежит meetingraft.sqlite3. На Маке это
  ~/Library/Application Support/MeetingRaft

Скачивается около 42 МБ моделей и 2.3 МБ контрольных записей.
USAGE
    exit 1
fi

DIR="$ROOT/models/diarize"
CHECK="$DIR/check"
mkdir -p "$CHECK"

SEG_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models"
EMB_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models"

fetch() {
    local url="$1" dest="$2"
    if [ -f "$dest" ]; then
        echo "  уже на месте: $(basename "$dest")"
        return
    fi
    echo "  качаю: $(basename "$dest")"
    curl -fL --progress-bar -o "$dest.part" "$url"
    mv "$dest.part" "$dest"
}

echo "Модели -> $DIR"

# Сегментация приезжает архивом; нужен из него один файл.
if [ ! -f "$DIR/segmentation.onnx" ]; then
    echo "  качаю: segmentation.onnx"
    TMP="$(mktemp -d)"
    curl -fL --progress-bar -o "$TMP/seg.tar.bz2" \
        "$SEG_URL/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2"
    tar xjf "$TMP/seg.tar.bz2" -C "$TMP"
    # Именно model.onnx, а не model.int8.onnx: квантованная модель
    # подменилась бы молча и разделяла бы хуже, а объяснить расхождение
    # чисел замера было бы нечем.
    mv "$TMP/sherpa-onnx-pyannote-segmentation-3-0/model.onnx" "$DIR/segmentation.onnx"
    rm -rf "$TMP"
else
    echo "  уже на месте: segmentation.onnx"
fi

# CAM++, обученный на китайском и английском. Английский VoxCeleb стоял
# здесь до 2026-08-11 и был заменён по замеру: на трудной записи он давал
# 4 голоса в исходном виде, 5 при переставленных половинах и 6 при
# удвоении, а CAM++ даёт 4 при любом расположении. Неустойчивость
# оказалась свойством соответствия модели материалу, а не кластеризации.
#
# По-русски не обучена ни та, ни другая: на наших встречах это проверяет
# задача 3.
fetch "$EMB_URL/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx" "$DIR/embedding.onnx"

echo "Контрольные записи -> $CHECK"
# Число людей в имени файла — прибор читает ожидаемый ответ оттуда.
fetch "$SEG_URL/1-two-speakers-en.wav" "$CHECK/2-two-speakers-en.wav"
fetch "$SEG_URL/0-four-speakers-zh.wav" "$CHECK/4-four-speakers-zh.wav"

echo
echo "Готово. Прогон:"
echo "  cd rust && cargo run --release -p meetingraft-diarize-probe --features model -- \"$ROOT\""
