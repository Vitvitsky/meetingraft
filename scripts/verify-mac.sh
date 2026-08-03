#!/usr/bin/env bash
# Полная проверка репозитория на macOS: Rust + Swift + линтеры.
# Запуск из корня репо: scripts/verify-mac.sh
#
# На Linux-машине разработки Swift-часть недоступна (нет Xcode), поэтому
# шаги 4-6 выполняются только здесь. Шаг 3 (генерация биндингов) обязателен
# перед сборкой приложения: Swift-код собирается против Generated/.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

step() {
    printf '\n\033[1m==> %s\033[0m\n' "$1"
}

step "1/6 Rust: тесты"
(cd rust && cargo test)

step "2/6 Rust: clippy и формат"
(cd rust && cargo clippy --all-targets -- -D warnings && cargo fmt --check)

step "3/6 UniFFI: dylib, биндинги, Xcode-проект"
apps/macos/Scripts/generate-ffi.sh

step "4/6 Swift: формат"
(cd apps/macos && swiftformat Sources Tests --lint)

step "5/6 Swift: сборка и тесты"
(cd apps/macos && xcodebuild \
    -project MeetingRaft.xcodeproj \
    -scheme MeetingRaft \
    -configuration Debug \
    CODE_SIGNING_ALLOWED=NO \
    test)

step "6/6 pre-commit"
if command -v pre-commit >/dev/null 2>&1; then
    pre-commit run --all-files
else
    echo "pre-commit не установлен — пропущено (brew install pre-commit)"
fi

printf '\n\033[1;32mВсё зелёное.\033[0m\n'
