#!/usr/bin/env bash
# Скачивает ggml Whisper-модель с Hugging Face в Application Support MeetingRaft.
# Источник: huggingface.co/ggerganov/whisper.cpp
# По умолчанию: ggml-base (dev). Для prod: MODEL=large-v3-turbo ./download-stt-model.sh
#
# Предпочитает `hf download` (меньше обрывов), иначе curl.
set -euo pipefail

MODEL="${MODEL:-base}"
SUPPORT="${HOME}/Library/Application Support/meetingraft"
MODELS_DIR="${SUPPORT}/models"
HF_REPO="ggerganov/whisper.cpp"
mkdir -p "$MODELS_DIR"

case "$MODEL" in
  base)
    FILE="ggml-base.bin"
    ;;
  small)
    FILE="ggml-small.bin"
    ;;
  large-v3-turbo|turbo)
    FILE="ggml-large-v3-turbo.bin"
    ;;
  *)
    echo "Unknown MODEL=$MODEL (base|small|large-v3-turbo)" >&2
    exit 1
    ;;
esac

DEST="$MODELS_DIR/$FILE"
if [[ -f "$DEST" ]]; then
  echo "Already present: $DEST"
  exit 0
fi

echo "Downloading $FILE → $DEST (HF: $HF_REPO)"
if command -v hf >/dev/null 2>&1; then
  hf download "$HF_REPO" "$FILE" --local-dir "$MODELS_DIR"
elif command -v huggingface-cli >/dev/null 2>&1; then
  huggingface-cli download "$HF_REPO" "$FILE" --local-dir "$MODELS_DIR"
else
  URL="https://huggingface.co/${HF_REPO}/resolve/main/${FILE}"
  curl -fL --progress-bar -o "$DEST.partial" "$URL"
  mv "$DEST.partial" "$DEST"
fi

test -f "$DEST"
echo "OK. Rebuild FFI with --features whisper to use it:"
echo "  apps/macos/Scripts/generate-ffi.sh"
echo ""
echo "Для sync-перевода (отдельная функция) позже: NLLB / small LLM тоже с HF"
echo "  в каталог models/translate/ — не путать с ggml Whisper."
