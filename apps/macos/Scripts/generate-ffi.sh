#!/usr/bin/env bash
# Собирает meetingraft-ffi, генерирует Swift-биндинги UniFFI и Xcode-проект.
# Запуск из корня репо: apps/macos/Scripts/generate-ffi.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
MACOS_DIR="$ROOT/apps/macos"
RUST_DIR="$ROOT/rust"
OUT_DIR="$MACOS_DIR/Generated"
# Всегда пишем dylib туда, куда смотрит XcodeGen (не sandbox CARGO_TARGET_DIR).
TARGET_DIR="$RUST_DIR/target"

mkdir -p "$OUT_DIR"
cd "$RUST_DIR"
export CARGO_TARGET_DIR="$TARGET_DIR"

# Whisper Metal и движок голосов по умолчанию; CI: MEETINGRAFT_FFI_FEATURES= ./generate-ffi.sh
#
# `diarize` здесь не роскошь: без него подпись по слепкам в приложении не
# работает вовсе, и вкладка Speakers прячет её целиком. Цена известна и
# велика — sherpa качает готовый тулкит и линкует его весь, порядка 34 МБ
# в бинаре; ужать это до одной модели эмбеддинга — задача 5.1 плана.
#
# `gigaam` — русский движок post-call. Тулкит тот же самый, что у
# `diarize`, и распознаватель в нём уже линкуется: фича добавляет к
# бинарю почти ничего, а без неё выбор движка в настройках был бы
# заглушкой — переключатель, который не переключает.
FEATURES="${MEETINGRAFT_FFI_FEATURES-whisper,diarize,gigaam}"
if [[ -n "$FEATURES" ]]; then
  cargo build -p meetingraft-ffi --features "$FEATURES"
else
  cargo build -p meetingraft-ffi
fi
LIB="$TARGET_DIR/debug/libmeetingraft_ffi.dylib"
test -f "$LIB"

cargo run -p uniffi-bindgen -- generate --library "$LIB" --language swift --out-dir "$OUT_DIR"
echo "Generated Swift bindings in $OUT_DIR"
echo "dylib: $LIB"

cd "$MACOS_DIR"
xcodegen generate
echo "Xcode project: $MACOS_DIR/MeetingRaft.xcodeproj"
