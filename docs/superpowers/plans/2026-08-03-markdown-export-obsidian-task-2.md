# Markdown export — Task 2: ViewModel `exportMarkdown`

> **Parent plan:** `docs/superpowers/plans/2026-08-03-markdown-export-obsidian.md`
> **Spec:** `docs/superpowers/specs/2026-08-03-markdown-export-obsidian-design.md`

**Goal:** Оркестрация экспорта Final + latest Brief/Follow-up через `MeetingsViewModel`.

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift`

## Interfaces

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

## Logic

1. `getFinalTranscript(meetingId)` — если `transcript.meetingId.isEmpty` → `.failure("Нужен Final transcript")` (как `reload(meetingId:)`).
2. Записать `…-final.md` через `MarkdownExport.write`.
3. `listArtifacts(meetingId)` — latest Brief и latest FollowUp по `createdAtMs`; пропустить отсутствующие.
4. `exportStatusMessage` на success/failure.
5. Backend refine markdown не трогать.

## TDD tests

- `testExportMarkdownWritesFinalAndLatestArtifacts` — 3 файла; brief = latest по `createdAtMs`; `startedAtMs = 1_785_715_200_000`.
- `testExportMarkdownFailsWithoutFinal` — empty `meetingId` в DTO; failure + status message.

## Verify

```bash
cd apps/macos && xcodegen generate
swiftformat Sources Tests --lint
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO \
  -only-testing:MeetingRaftTests/MeetingsViewModelTests
```

## Commit

```
feat: exportMarkdown Final/Brief/Follow-up в папку
```
