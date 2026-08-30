#!/usr/bin/env bash
# Скачать многоязычный распознаватель parakeet-tdt-0.6b-v3.
#
# Кладёт всё в <каталог-данных>/models/parakeet/:
#   encoder.int8.onnx      — энкодер FastConformer (622 МБ, int8)
#   decoder.int8.onnx      — предсказатель TDT
#   joiner.int8.onnx       — объединяющая сеть
#   tokens.txt             — BPE-словарь на 8193 позиции
#   check/                 — контрольные записи экспорта (en, es, de, fr)
#
# Зачем он рядом с GigaAM: тот понимает только русский, а ADR-003 требует
# ru/en/es. Этот заявляет 25 языков, включая все три наших, и отдаёт
# тайм-коды слов с пунктуацией. Заменит ли он оба движка — решает замер
# (`docs/superpowers/plans/2026-08-28-asr-bench.md`, задача 5), а не эта
# строка.
#
# Лицензия модели — CC-BY-4.0, у NVIDIA. Проверяется у **этого** файла, а
# не у семейства: у соседнего экспорта GigaAM в каталоге sherpa стоит
# non-commercial, хотя у v3 в репозитории заявлен MIT.
#
# Сеть трогает только этот скрипт. Движок живёт за фичей `parakeet`,
# выключенной по умолчанию.

set -euo pipefail

ROOT="${1:-}"
if [ -z "$ROOT" ]; then
    cat <<'USAGE'
Использование: scripts/fetch-parakeet-models.sh <каталог-данных>

Каталог данных — тот, где лежит meetingraft.sqlite3. На Маке это
  ~/Library/Application Support/meetingraft

Скачивается архив на 465 МБ, в распакованном виде — 640 МБ.

Кладётся всё в <каталог-данных>/models/parakeet/ — рядом с моделями
Whisper, GigaAM и разделения голосов. Ничего вне этого каталога скрипт не
трогает.
USAGE
    exit 1
fi

DIR="$ROOT/models/parakeet"
CHECK="$DIR/check"
mkdir -p "$CHECK"

# Версия экспорта зашита явно, а не берётся «последняя»: смена экспорта —
# это смена того, что мерится, и происходить она должна руками.
EXPORT="sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"
URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/$EXPORT.tar.bz2"

# Метка рядом с файлами — та же дисциплина, что у соседних скриптов: без
# неё смена модели не доезжает ни до кого. Файлы на месте, скрипт их
# пропускает, и человек продолжает мерить старой моделью.
MARK="$DIR/.source"
if [ -f "$DIR/encoder.int8.onnx" ] && [ -f "$MARK" ] && [ "$(cat "$MARK")" = "$EXPORT" ]; then
    echo "уже на месте: $EXPORT"
    exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Качаю $EXPORT (465 МБ) -> $DIR"
curl -fL --progress-bar -o "$TMP/model.tar.bz2" "$URL"

echo "Распаковываю"
tar xjf "$TMP/model.tar.bz2" -C "$TMP"

# Имена файлов фиксированы, а не «любой .onnx». У parakeet они **другие**,
# чем у GigaAM: `decoder.int8.onnx` против `decoder.onnx`. Подстановка
# чужого файла не сломала бы ничего видимого — движок просто распознавал
# бы хуже.
for file in encoder.int8.onnx decoder.int8.onnx joiner.int8.onnx tokens.txt; do
    if [ ! -f "$TMP/$EXPORT/$file" ]; then
        echo "в архиве нет $file — экспорт сменился, скрипт править руками" >&2
        exit 1
    fi
    mv "$TMP/$EXPORT/$file" "$DIR/$file"
    echo "  $file"
done

# Контрольные записи экспорта: русской среди них нет (en, es, de, fr).
# Русский контроль у нас свой — запись GigaAM, у которой эталон взят из
# стихотворения, а не из выхода какой-либо модели.
if [ -d "$TMP/$EXPORT/test_wavs" ]; then
    mv "$TMP/$EXPORT/test_wavs/"*.wav "$CHECK/" 2>/dev/null || true
    echo "  контрольные записи -> $CHECK"
fi

printf '%s' "$EXPORT" > "$MARK"

cat <<EOF

Готово. Дальше — в каталоге rust:

  cargo run --release -p meetingraft-stt-probe --features parakeet -- "$ROOT"

Прибор начинает с самопроверки: контрольная запись против эталона и тот
же эталон против шума. Не разошлись — до настоящего звука дело не
доходит.
EOF
