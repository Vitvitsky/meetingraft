#!/usr/bin/env bash
# Собирает meetingraft-ffi и генерирует Swift-биндинги UniFFI.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
RUST_DIR="$ROOT/rust"
OUT_DIR="$ROOT/apps/macos/Generated"
TARGET_DIR="${CARGO_TARGET_DIR:-$RUST_DIR/target}"

mkdir -p "$OUT_DIR"
cd "$RUST_DIR"
export CARGO_TARGET_DIR="$TARGET_DIR"

cargo build -p meetingraft-ffi
LIB="$TARGET_DIR/debug/libmeetingraft_ffi.dylib"
test -f "$LIB"

cargo run -p uniffi-bindgen -- generate --library "$LIB" --language swift --out-dir "$OUT_DIR"
echo "Generated Swift bindings in $OUT_DIR"
echo "dylib: $LIB"
