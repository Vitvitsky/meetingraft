# Markdown export (Obsidian) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export Final + optional Brief/Follow-up as separate `.md` files into a Settings (or picker) folder for Obsidian, with overwrite.

**Architecture:** Pure Swift writer + naming helpers; ViewModel orchestrates UniFFI reads and write; Settings holds `exportFolderPath`. No Rust filesystem I/O.

**Tech Stack:** SwiftUI, Foundation `FileManager`, XCTest, existing `MeetingsCoreProviding`.

**Spec:** `docs/superpowers/specs/2026-08-03-markdown-export-obsidian-design.md`

## Global Constraints

- Up to 3 files: `final` / `brief` / `follow-up`; skip missing artifacts; fail without Final.
- Names: `{yyyy-MM-dd}-{shortId}-{kind}.md`; overwrite existing.
- Body = raw `body_markdown`; no frontmatter.
- File I/O only in Swift (AGENTS.md platform layer).
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- TDD: failing test → implement → green → commit per task.

---

### Task 1: `MarkdownExport` naming + write (TDD)

**Files:**
- Create: `apps/macos/Sources/Meetings/MarkdownExport.swift`
- Create: `apps/macos/Tests/MarkdownExportTests.swift`
- Modify: `apps/macos/project.yml` only if new sources need explicit listing (Xcodegen usually globs `Sources/**`)

**Interfaces:**
- Produces:
  ```swift
  enum MarkdownExportKind: String {
      case final = "final"
      case brief = "brief"
      case followUp = "follow-up"
  }

  enum MarkdownExport {
      static func shortId(meetingId: String) -> String
      static func fileName(startedAtMs: UInt64, meetingId: String, kind: MarkdownExportKind, calendar: Calendar = .current, timeZone: TimeZone = .current) -> String
      /// Пишет UTF-8 markdown; создаёт directory; перезаписывает файл.
      static func write(folderURL: URL, fileName: String, body: String) throws -> URL
  }
  ```

- [ ] **Step 1: Failing tests**

```swift
final class MarkdownExportTests: XCTestCase {
    func testShortIdTakesFirst8SafeChars() {
        XCTAssertEqual(MarkdownExport.shortId(meetingId: "abcdef12-zzzz"), "abcdef12")
        XCTAssertEqual(MarkdownExport.shortId(meetingId: "ab/cd"), "ab_cd")
    }

    func testFileNameUsesDateShortIdAndKind() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: 0)!
        // 2026-08-03T00:00:00Z
        let ms: UInt64 = 1_754_179_200_000
        let name = MarkdownExport.fileName(
            startedAtMs: ms,
            meetingId: "a1b2c3d4xxxx",
            kind: .brief,
            calendar: calendar,
            timeZone: TimeZone(secondsFromGMT: 0)!
        )
        XCTAssertEqual(name, "2026-08-03-a1b2c3d4-brief.md")
    }

    func testWriteCreatesAndOverwrites() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mr-export-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let url1 = try MarkdownExport.write(folderURL: dir, fileName: "x-final.md", body: "v1")
        XCTAssertEqual(try String(contentsOf: url1, encoding: .utf8), "v1")
        _ = try MarkdownExport.write(folderURL: dir, fileName: "x-final.md", body: "v2")
        XCTAssertEqual(try String(contentsOf: url1, encoding: .utf8), "v2")
    }
}
```

Fix expected `shortId`/`fileName` assertions to match the chosen rule: first 8 chars of sanitized id.

- [ ] **Step 2: Run — expect FAIL**

```bash
cd apps/macos && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO \
  -only-testing:MeetingRaftTests/MarkdownExportTests
```

- [ ] **Step 3: Implement `MarkdownExport.swift`**

- [ ] **Step 4: Tests PASS + swiftformat**

```bash
swiftformat Sources Tests --lint
```

- [ ] **Step 5: Commit**

```bash
git add apps/macos/Sources/Meetings/MarkdownExport.swift apps/macos/Tests/MarkdownExportTests.swift
git commit -m "$(cat <<'EOF'
feat: MarkdownExport имена файлов и запись на диск

EOF
)"
```

---

### Task 2: ViewModel `exportMarkdown` (TDD)

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift`

**Interfaces:**
- Consumes: `getFinalTranscript`, `listArtifacts`, `MarkdownExport`
- Produces:
  ```swift
  struct MarkdownExportResult: Equatable {
      var writtenFileNames: [String]
      var folderPath: String
  }

  // MeetingsViewModel
  private(set) var exportStatusMessage: String = ""

  func exportMarkdown(
      meetingId: String,
      startedAtMs: UInt64,
      folderURL: URL
  ) -> Result<MarkdownExportResult, String>
  ```
  Logic:
  1. `getFinalTranscript` — if `meetingId` empty / no body → `Err("Нужен Final transcript")` (match existing empty-final convention used in app).
  2. Write `…-final.md`.
  3. From `listArtifacts`, pick latest Brief and latest FollowUp by `createdAtMs`; write if present.
  4. Set `exportStatusMessage` on success/failure.
  5. Do not touch backend refine markdown.

- [ ] **Step 1: Failing tests**

```swift
func testExportMarkdownWritesFinalAndLatestArtifacts() throws {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("mr-vm-export-\(UUID().uuidString)", isDirectory: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let transcript = FfiFinalTranscript(
        meetingId: "abcd1234-rest",
        version: 1,
        bodyMarkdown: "# Final body",
        createdAtMs: 1
    )
    let briefOld = makeArtifact(id: "b0", meetingId: "abcd1234-rest", kind: .brief, body: "old", createdAtMs: 10)
    let briefNew = makeArtifact(id: "b1", meetingId: "abcd1234-rest", kind: .brief, body: "new brief", createdAtMs: 20)
    let follow = makeArtifact(id: "f1", meetingId: "abcd1234-rest", kind: .followUp, body: "fu", createdAtMs: 15)
    // extend makeArtifact helper with kind/body/createdAtMs as needed
    let core = MeetingsCoreSpy(finalTranscript: transcript, artifacts: [briefOld, briefNew, follow])
    let vm = MeetingsViewModel(core: core)

    let result = vm.exportMarkdown(
        meetingId: "abcd1234-rest",
        startedAtMs: 1_785_715_200_000,
        folderURL: dir
    )
    guard case let .success(ok) = result else { return XCTFail("\(result)") }
    XCTAssertEqual(ok.writtenFileNames.count, 3)
    let briefURL = dir.appendingPathComponent(ok.writtenFileNames.first { $0.contains("brief") }!)
    XCTAssertEqual(try String(contentsOf: briefURL, encoding: .utf8), "new brief")
}

func testExportMarkdownFailsWithoutFinal() {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("mr-vm-export-empty-\(UUID().uuidString)", isDirectory: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let core = MeetingsCoreSpy(
        finalTranscript: FfiFinalTranscript(meetingId: "", version: 0, bodyMarkdown: "", createdAtMs: 0)
    )
    let vm = MeetingsViewModel(core: core)
    let result = vm.exportMarkdown(meetingId: "m1", startedAtMs: 1, folderURL: dir)
    guard case .failure = result else { return XCTFail("expected failure") }
}
```

Adapt to how `MeetingsViewModel` detects empty final today (`meetingId.isEmpty`).

- [ ] **Step 2: FAIL → implement → GREEN**

```bash
xcodebuild ... -only-testing:MeetingRaftTests/MeetingsViewModelTests
```

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: exportMarkdown Final/Brief/Follow-up в папку

EOF
)"
```

---

### Task 3: Settings export folder + MeetingDetail UI

**Files:**
- Modify: `ProviderSettingsStore.swift` — `exportFolderPath`
- Modify: `SettingsView.swift` — Export section
- Modify: `MeetingDetailView.swift` — Export button + folder picker
- Test: `ProviderSettingsStoreTests.swift` (default path non-empty)

**Interfaces:**
- `var exportFolderPath: String` default `"~/Documents/MeetingRaft"`
- Settings: TextField + Button «Choose…» (`NSOpenPanel` canChooseDirectories)
- MeetingDetail: Button «Export to Markdown»; uses `exportFolderPath` expanded; optional «Choose folder…» then export; show `viewModel.exportStatusMessage` / `errorMessage`

Expand tilde:

```swift
NSString(string: path).expandingTildeInPath
```

- [x] **Step 1: Test default export path**

```swift
func testExportFolderPathDefault() {
    let store = ProviderSettingsStore()
    XCTAssertTrue(store.exportFolderPath.contains("MeetingRaft"))
}
```

- [x] **Step 2: Wire UI (manual smoke OK for panel)**

- [x] **Step 3: macOS tests + swiftformat**

- [x] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: Settings export folder и кнопка Export to Markdown

EOF
)"
```

---

### Task 4: Docs + verify

**Files:**
- `docs/backlog.md` — export `.md` done/partial; **API + Obsidian plugin deferred**
- `docs/roadmap.md` — Remaining note
- `docs/architecture-and-install.md` — short Export / Obsidian tip

- [ ] **Step 1: Docs**

- [ ] **Step 2: Full verify**

```bash
cd apps/macos && swiftformat Sources Tests --lint && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: Markdown export / Obsidian и backlog API plugin

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| Naming + write/overwrite | 1 |
| Final required; skip missing artifacts; latest per kind | 2 |
| Settings path + Choose folder + Export UI | 3 |
| Docs + Obsidian plugin backlog | 4 |
| No Rust FS / no frontmatter / no API | (non-goals) |
