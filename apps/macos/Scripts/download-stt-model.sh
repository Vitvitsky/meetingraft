#!/usr/bin/env bash
# Скачивает ggml Whisper-модель в Application Support MeetingRaft.
# По умолчанию: ggml-base (dev). Для prod: MODEL=large-v3-turbo ./download-stt-model.sh
set -euo pipefail

MODEL="${MODEL:-base}"
SUPPORT="${HOME}/Library/Application Support/meetingraft"
MODELS_DIR="${SUPPORT}/models"
mkdir -p "$MODELS_DIR"

case "$MODEL" in
  base)
    FILE="ggml-base.bin"
    URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
    ;;
  small)
    FILE="ggml-small.bin"
    URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
    ;;
  large-v3-turbo|turbo)
    FILE="ggml-large-v3-turbo.bin"
    URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
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

echo "Downloading $FILE → $DEST"
curl -fL --progress-bar -o "$DEST.partial" "$URL"
mv "$DEST.partial" "$DEST"
echo "OK. Rebuild FFI with --features whisper to use it:"
echo "  cd rust && cargo build -p meetingraft-ffi --features whisper"
echo "  apps/macos/Scripts/generate-ffi.sh  # (скрипт включает whisper)"
