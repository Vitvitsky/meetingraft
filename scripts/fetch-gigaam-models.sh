#!/usr/bin/env bash
# Скачать русский распознаватель GigaAM v3 и контрольную запись к нему.
#
# Кладёт всё в <каталог-данных>/models/gigaam/:
#   encoder.int8.onnx      — энкодер Conformer (225 МБ, int8)
#   decoder.onnx           — предсказатель RNNT
#   joiner.onnx            — объединяющая сеть RNNT
#   tokens.txt             — алфавит (посимвольный, не subword)
#   check/example.wav      — запись с известным текстом
#   check/example.txt      — сам текст
#
# Контрольная запись не украшение. Прибор `stt-probe` без неё не судит
# движок вовсе: прогон, у которого нет заведомо верного ответа, отличить
# от сломанного нельзя (`CLAUDE.md`, раздел про способы проверить пустоту
# и не заметить).
#
# Сеть трогает только этот скрипт. Движок живёт за фичей `gigaam`,
# выключенной по умолчанию, и обычная сборка не качает ничего.

set -euo pipefail

ROOT="${1:-}"
if [ -z "$ROOT" ]; then
    cat <<'USAGE'
Использование: scripts/fetch-gigaam-models.sh <каталог-данных>

Каталог данных — тот, где лежит meetingraft.sqlite3. На Маке это
  ~/Library/Application Support/meetingraft

Скачивается около 230 МБ модели и 0.4 МБ контрольной записи.

Кладётся всё в <каталог-данных>/models/gigaam/ — рядом с базой, туда же,
куда приложение кладёт модели Whisper и куда fetch-diarize-models.sh
кладёт модели разделения голосов. Ничего вне этого каталога скрипт не
трогает.
USAGE
    exit 1
fi

DIR="$ROOT/models/gigaam"
CHECK="$DIR/check"
mkdir -p "$CHECK"

# Версия экспорта зашита явно, а не берётся «последняя». Эти файлы лежат
# на Hugging Face, но **в релиз sherpa-onnx и в документацию не попали**
# (issue k2-fsa/sherpa-onnx#3619), то есть «последняя версия» здесь
# означает «неизвестно что». Меняется версия — меняется и строка ниже,
# руками и осознанно.
EXPORT="sherpa-onnx-nemo-transducer-giga-am-v3-russian-2025-12-16"
BASE="https://huggingface.co/csukuangfj/$EXPORT/resolve/main"

# Рядом с каждым скачанным файлом лежит `.source` с именем того, что в
# него скачано. Без него смена модели не доезжает ни до кого: файл на
# месте, скрипт его пропускает, и человек продолжает мерить старой
# моделью, ничего об этом не зная. На этом обожглись 2026-08-11 с
# моделью голосов.
fetch() {
    local url="$1" dest="$2"
    # Объявление отдельно от присваивания: `local x="$(...)"` прячет код
    # возврата подстановки, и `set -e` на ней не сработает.
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
# Имена файлов фиксированы, а не «любой .onnx в каталоге». В соседнем
# экспорте того же семейства (CTC) лежит `model.int8.onnx` того же
# размера, и подстановка чужого файла не сломала бы ничего видимого —
# движок просто распознавал бы хуже, а объяснить расхождение чисел
# замера было бы нечем.
fetch "$BASE/encoder.int8.onnx" "$DIR/encoder.int8.onnx"
fetch "$BASE/decoder.onnx" "$DIR/decoder.onnx"
fetch "$BASE/joiner.onnx" "$DIR/joiner.onnx"
fetch "$BASE/tokens.txt" "$DIR/tokens.txt"

echo "Контрольная запись -> $CHECK"
fetch "$BASE/test_wavs/example.wav" "$CHECK/example.wav"

# Эталон взят **из стихотворения**, а не из чужой расшифровки. Разница
# принципиальная: расшифровка, опубликованная рядом с моделью, — это
# выход модели того же семейства, и сверять с ней значит сверять догадку
# сама с собой. Здесь же читается «Евгений Онегин» (посвящение) и первые
# строки «Руслана и Людмилы» — текст, известный независимо от любого
# распознавателя.
#
# Регистр и знаки препинания сняты: движок-transducer их не ставит, а
# считалка WER всё равно нормализует обе стороны.
cat > "$CHECK/example.txt" <<'REFERENCE'
ничьих не требуя похвал счастлив уж я надеждой сладкой что дева с трепетом любви посмотрит может быть украдкой на песни грешные мои у лукоморья дуб зеленый
REFERENCE
echo "  записан: example.txt (Пушкин, известен независимо от модели)"

echo
echo "Готово. Дальше — в каталоге rust:"
echo
echo "  cargo run --release -p meetingraft-stt-probe --features gigaam -- \"$ROOT\""
echo
echo "Прибор начинает с самопроверки: контрольная запись против эталона"
echo "и тот же эталон против шума. Не разошлись — до настоящего звука"
echo "дело не доходит."
