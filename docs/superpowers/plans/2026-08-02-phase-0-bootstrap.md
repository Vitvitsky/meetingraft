# Phase 0 — Decisions and Tooling Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Рабочий скелет монорепо: cargo workspace с первым крейтом, Xcode-проект MeetingRaft через XcodeGen и CI, который собирает оба мира и гоняет тесты.

**Architecture:** Rust workspace в `rust/` (крейты в `rust/crates/`), macOS-приложение в `apps/macos/` (проект генерируется XcodeGen из `project.yml`, сам `.xcodeproj` не трекается), GitHub Actions собирает обе части. ADR-004..007 уже приняты — код в этой фазе не реализует продуктовую логику, только каркас.

**Tech Stack:** Rust stable (edition 2024), cargo workspace; SwiftUI, XcodeGen, xcodebuild; GitHub Actions (macos-15 runner).

## Global Constraints

- Минимальная macOS: **15.0** (ADR-004) — проставляется в deployment target.
- Идентификаторы английские, комментарии/docstrings русские (AGENTS.md).
- Коммиты: Conventional Commits с русской темой.
- SwiftUI-слой не содержит бизнес-логики; доменная логика — только в Rust (AGENTS.md). В этой фазе — заглушки без логики.
- Bundle id: `com.vitvitsky.meetingraft`.
- Сгенерированный `MeetingRaft.xcodeproj` не коммитится (генерируется из `project.yml`).

---

### Task 1: Cargo workspace и крейт domain

**Files:**
- Create: `rust/Cargo.toml`
- Create: `rust/crates/domain/Cargo.toml`
- Create: `rust/crates/domain/src/lib.rs`
- Delete: `rust/crates/.gitkeep` (каталог перестаёт быть пустым)

**Interfaces:**
- Consumes: ничего (первая задача).
- Produces: workspace `rust/` с членом `meetingraft-domain`; команда проверки для CI — `cargo test` из каталога `rust/`. Package name: `meetingraft-domain`, путь `rust/crates/domain`.

- [ ] **Step 1: Написать падающий тест (крейта ещё нет)**

Создать `rust/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/domain"]

[workspace.package]
version = "0.1.0"
edition = "2024"
```

Создать `rust/crates/domain/Cargo.toml`:

```toml
[package]
name = "meetingraft-domain"
version.workspace = true
edition.workspace = true

[lib]
name = "domain"
```

Создать `rust/crates/domain/src/lib.rs` только с тестом (без константы — тест должен упасть на компиляции):

```rust
//! Доменные модели MeetingRaft. Наполняется в Phase 2 (см. docs/roadmap.md).

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-тест сборки workspace: версия крейта совпадает с манифестом.
    #[test]
    fn crate_version_matches_manifest() {
        assert_eq!(CRATE_VERSION, "0.1.0");
    }
}
```

- [ ] **Step 2: Убедиться, что тест падает**

Run: `cd rust && cargo test`
Expected: FAIL — ошибка компиляции `cannot find value CRATE_VERSION`.

- [ ] **Step 3: Минимальная реализация**

В `rust/crates/domain/src/lib.rs` добавить над блоком тестов:

```rust
/// Версия доменного крейта; используется smoke-тестом сборки.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
```

- [ ] **Step 4: Убедиться, что тесты проходят**

Run: `cd rust && cargo test`
Expected: PASS — `1 passed; 0 failed`.

- [ ] **Step 5: Прогнать линтеры (те же команды, что будут в CI)**

Run: `cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: обе команды завершаются без ошибок.

- [ ] **Step 6: Удалить `.gitkeep` и закоммитить**

```bash
git rm rust/crates/.gitkeep
git add rust/
git commit -m "feat: cargo workspace и крейт meetingraft-domain"
```

---

### Task 2: Xcode-проект MeetingRaft через XcodeGen

**Files:**
- Create: `apps/macos/project.yml`
- Create: `apps/macos/Sources/MeetingRaftApp.swift`
- Create: `apps/macos/Sources/ContentView.swift`
- Modify: `.gitignore` (игнорировать сгенерированный `.xcodeproj`)
- Delete: `apps/macos/.gitkeep`

**Interfaces:**
- Consumes: ничего (не зависит от Task 1).
- Produces: команда сборки для CI —
  `xcodegen generate` в `apps/macos/`, затем
  `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO`.
  Scheme: `MeetingRaft`. Bundle id: `com.vitvitsky.meetingraft`.

- [ ] **Step 1: Установить XcodeGen и SwiftFormat (если не установлены)**

Run: `command -v xcodegen || brew install xcodegen; command -v swiftformat || brew install swiftformat`
Expected: `xcodegen` и `swiftformat` доступны в PATH.

- [ ] **Step 2: Описать проект**

Создать `apps/macos/project.yml`:

```yaml
name: MeetingRaft
options:
  bundleIdPrefix: com.vitvitsky
  deploymentTarget:
    macOS: "15.0"
targets:
  MeetingRaft:
    type: application
    platform: macOS
    sources:
      - Sources
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: com.vitvitsky.meetingraft
        MACOSX_DEPLOYMENT_TARGET: "15.0"
        SWIFT_VERSION: "6.0"
    info:
      path: Sources/Info.plist
      properties:
        CFBundleDisplayName: MeetingRaft
```

- [ ] **Step 3: Минимальное приложение**

Создать `apps/macos/Sources/MeetingRaftApp.swift`:

```swift
import SwiftUI

/// Точка входа приложения MeetingRaft.
@main
struct MeetingRaftApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
```

Создать `apps/macos/Sources/ContentView.swift`:

```swift
import SwiftUI

/// Заглушка главного экрана; заменяется реальным shell в Phase 1.
struct ContentView: View {
    var body: some View {
        Text("MeetingRaft")
            .padding()
    }
}
```

- [ ] **Step 4: Конфиг SwiftFormat и проверка**

Создать `apps/macos/.swiftformat`:

```
--swiftversion 6.0
```

Run: `cd apps/macos && swiftformat --lint Sources`
Expected: завершается без ошибок (0 files require formatting).

- [ ] **Step 5: Игнорировать генерируемые артефакты**

В `.gitignore`, в секцию `# Xcode / Swift`, добавить строки:

```gitignore
apps/macos/MeetingRaft.xcodeproj/
apps/macos/Sources/Info.plist
```

- [ ] **Step 6: Сгенерировать и собрать**

Run:
```bash
cd apps/macos && xcodegen generate && \
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug build CODE_SIGNING_ALLOWED=NO
```
Expected: `** BUILD SUCCEEDED **`.

- [ ] **Step 7: Проверить чистоту git-статуса и закоммитить**

Run: `git status --short` — в выводе не должно быть `MeetingRaft.xcodeproj` и `Info.plist`.

```bash
git rm apps/macos/.gitkeep
git add apps/macos/ .gitignore
git commit -m "feat: скелет macOS-приложения MeetingRaft через XcodeGen"
```

---

### Task 3: CI на GitHub Actions

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: из Task 1 — `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` в каталоге `rust/`; из Task 2 — `xcodegen generate` + `xcodebuild ... CODE_SIGNING_ALLOWED=NO` в каталоге `apps/macos/`, scheme `MeetingRaft`.
- Produces: workflow `CI` с job'ами `rust` и `macos`, запускается на push в `main` и на pull request.

- [ ] **Step 1: Написать workflow**

Создать `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    runs-on: macos-15
    defaults:
      run:
        working-directory: rust
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Формат
        run: cargo fmt --check
      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings
      - name: Тесты
        run: cargo test

  macos:
    runs-on: macos-15
    defaults:
      run:
        working-directory: apps/macos
    steps:
      - uses: actions/checkout@v4
      - name: Установить XcodeGen и SwiftFormat
        run: brew install xcodegen swiftformat
      - name: Линт Swift
        run: swiftformat --lint Sources
      - name: Сгенерировать проект
        run: xcodegen generate
      - name: Собрать приложение
        run: >
          xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft
          -configuration Debug build CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 2: Локальная проверка эквивалентов CI-команд**

Run (из корня репо):
```bash
(cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test) && \
(cd apps/macos && xcodegen generate && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO)
```
Expected: всё зелёное.

- [ ] **Step 3: Закоммитить и запушить, проверить CI**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: сборка Rust workspace и macOS-приложения в GitHub Actions"
git push
gh run watch --exit-status
```
Expected: оба job'а зелёные. Если runner `macos-15` недоступен или падает не из-за кода — зафиксировать лог и разбираться, не менять код вслепую.

---

### Task 4: Актуализировать Setup в AGENTS.md

**Files:**
- Modify: `AGENTS.md` (раздел `## Setup`)

**Interfaces:**
- Consumes: команды из Task 1–3.
- Produces: документированные команды разработки для всех будущих агентов.

- [ ] **Step 1: Заменить содержимое раздела Setup**

В `AGENTS.md` заменить тело раздела `## Setup` (строки от заголовка до `## Stack & conventions`) на:

```markdown
## Setup

- Rust core: `cd rust && cargo test` (workspace; крейты в `rust/crates/`)
- Lint Rust: `cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- macOS shell: `cd apps/macos && xcodegen generate`, затем открыть
  `MeetingRaft.xcodeproj` в Xcode или
  `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO`
  (`.xcodeproj` генерируется, в git не трекается — источник `project.yml`)
- CI: `.github/workflows/ci.yml` — fmt, clippy, cargo test, xcodebuild
- UniFFI bindings: появятся в Phase 2 (см. `docs/roadmap.md`)
- Backend: появится в Phase 6 (ADR-007); контракт — `shared/openapi.yaml`
- Docs: architecture и ADR — в `docs/`
```

- [ ] **Step 2: Закоммитить**

```bash
git add AGENTS.md
git commit -m "docs: актуализировать команды Setup после bootstrap"
```

---

## Exit criteria Phase 0 (из roadmap)

- [x] ADR-004..007 приняты (сделано до этого плана).
- [ ] Пустое приложение и workspace собираются зелёным в CI (Task 1–3).
- [ ] Команды разработки задокументированы (Task 4).
