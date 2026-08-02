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

cargo build -p meetingraft-ffi
LIB="$TARGET_DIR/debug/libmeetingraft_ffi.dylib"
test -f "$LIB"

cargo run -p uniffi-bindgen -- generate --library "$LIB" --language swift --out-dir "$OUT_DIR"
echo "Generated Swift bindings in $OUT_DIR"
echo "dylib: $LIB"

cd "$MACOS_DIR"
xcodegen generate
echo "Xcode project: $MACOS_DIR/MeetingRaft.xcodeproj"
