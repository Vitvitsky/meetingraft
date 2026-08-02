# Meetings ↔ backend refine stub UI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** На вкладке Artifacts закрыть e2e петлю Submit refine (stub) → poll → markdown рядом с local Brief, без новых Rust/API контрактов.

**Architecture:** Session-only state в `MeetingsViewModel`; poll на MainActor через UniFFI `submitBackendJob` / `getBackendJob` / `getBackendArtifact`; local `FfiArtifact` list не смешивается со stub body; networking остаётся в Rust sync.

**Tech Stack:** SwiftUI, Observation, UniFFI `MeetingCore`, XCTest, SwiftFormat.

**Spec:** `docs/superpowers/specs/2026-08-02-meetings-backend-refine-stub-design.md`

## Global Constraints

- Не менять OpenAPI / `meetingraft-sync` / FFI signatures (только Swift protocol + UI + ViewModel).
- Local Brief/Follow-up generation не трогать.
- Не persist `job_id`.
- Poll: max **20** attempts, delay **250 ms** (в тестах инжектить меньшие значения).
- Comments на русском; identifiers English; Conventional Commits с русским subject.
- `swiftformat Sources Tests --lint` должен быть чистым.

---

## File map

| File | Role |
|------|------|
| `apps/macos/Sources/Meetings/MeetingsViewModel.swift` | Protocol + backend refine state/poll |
| `apps/macos/Sources/Meetings/MeetingDetailView.swift` | Button + Backend refine panel + onDisappear reset |
| `apps/macos/Tests/MeetingsViewModelTests.swift` | Spy + 4 сценария |
| `docs/backlog.md` | Чекбокс e2e Meetings↔stub |
| `docs/roadmap.md` | Phase 6 follow-up note |
| `docs/architecture-and-install.md` | Кнопка + poll в install/flow |

---

### Task 1: Protocol + ViewModel poll (TDD)

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift`

**Interfaces:**
- Consumes: `FfiBackendJob`, `FfiBackendArtifact` (Generated UniFFI)
- Produces:
  - `enum BackendRefineStatus: String, Equatable { case idle, submitting, polling, succeeded, failed }`
  - `MeetingsCoreProviding.submitBackendJob(meetingId:kindCode:) -> FfiBackendJob`
  - `MeetingsCoreProviding.getBackendJob(jobId:) -> FfiBackendJob`
  - `MeetingsCoreProviding.getBackendArtifact(artifactId:) -> FfiBackendArtifact`
  - `MeetingsViewModel.backendJobStatus: BackendRefineStatus`
  - `MeetingsViewModel.backendJobId: String`
  - `MeetingsViewModel.backendArtifactMarkdown: String`
  - `MeetingsViewModel.performBackendRefine(meetingId: String) async`
  - `MeetingsViewModel.submitBackendRefine(meetingId: String)` — starts cancellable Task
  - `MeetingsViewModel.resetBackendRefineSession()`
  - Init: `init(core:maxPollAttempts:pollDelayNanoseconds:)` defaults `20` / `250_000_000`

- [ ] **Step 1: Extend protocol + spy stubs (compile)**

В `MeetingsViewModel.swift` добавить в protocol:

```swift
func submitBackendJob(meetingId: String, kindCode: String) -> FfiBackendJob
func getBackendJob(jobId: String) -> FfiBackendJob
func getBackendArtifact(artifactId: String) -> FfiBackendArtifact
```

`MeetingCore` уже имеет эти методы — `extension MeetingCore: MeetingsCoreProviding {}` продолжит компилироваться.

В `MeetingsCoreSpy` добавить свойства и реализации:

```swift
var submitJobResult: FfiBackendJob = FfiBackendJob(
    id: "", meetingId: "", kind: "", status: "", error: "", artifactIds: []
)
var getJobResults: [FfiBackendJob] = []
var getArtifactResult: FfiBackendArtifact = FfiBackendArtifact(
    id: "", kind: "", bodyMarkdown: "", createdAt: "", error: ""
)
private(set) var submitBackendJobCallCount = 0
private(set) var getBackendJobCallCount = 0
private(set) var getBackendArtifactCallCount = 0
private var getJobIndex = 0

func submitBackendJob(meetingId _: String, kindCode _: String) -> FfiBackendJob {
    submitBackendJobCallCount += 1
    return submitJobResult
}

func getBackendJob(jobId _: String) -> FfiBackendJob {
    getBackendJobCallCount += 1
    guard getJobIndex < getJobResults.count else {
        return getJobResults.last ?? submitJobResult
    }
    defer { getJobIndex += 1 }
    return getJobResults[getJobIndex]
}

func getBackendArtifact(artifactId _: String) -> FfiBackendArtifact {
    getBackendArtifactCallCount += 1
    return getArtifactResult
}
```

- [ ] **Step 2: Write failing tests**

Добавить в `MeetingsViewModelTests` (все `@MainActor`):

```swift
func testSubmitBackendRefineHappyPathImmediateSuccess() async {
    let transcript = makeTranscript(meetingId: "meeting-1")
    let local = makeArtifact(id: "local-1", meetingId: "meeting-1")
    let core = MeetingsCoreSpy(finalTranscript: transcript, artifacts: [local])
    core.submitJobResult = FfiBackendJob(
        id: "job-1",
        meetingId: "meeting-1",
        kind: "refine",
        status: "succeeded",
        error: "",
        artifactIds: ["art-b1"]
    )
    core.getArtifactResult = FfiBackendArtifact(
        id: "art-b1",
        kind: "refine",
        bodyMarkdown: "# Stub refine",
        createdAt: "2026-08-02T00:00:00Z",
        error: ""
    )
    let viewModel = MeetingsViewModel(core: core, maxPollAttempts: 20, pollDelayNanoseconds: 0)
    viewModel.reload(meetingId: "meeting-1")

    await viewModel.performBackendRefine(meetingId: "meeting-1")

    XCTAssertEqual(viewModel.backendJobStatus, .succeeded)
    XCTAssertEqual(viewModel.backendJobId, "job-1")
    XCTAssertEqual(viewModel.backendArtifactMarkdown, "# Stub refine")
    XCTAssertEqual(viewModel.artifacts, [local])
    XCTAssertEqual(core.submitBackendJobCallCount, 1)
    XCTAssertEqual(core.getBackendArtifactCallCount, 1)
    XCTAssertEqual(core.getBackendJobCallCount, 0)
    XCTAssertNil(viewModel.errorMessage)
}

func testSubmitBackendRefineSurfacesSubmitError() async {
    let transcript = makeTranscript(meetingId: "meeting-1")
    let core = MeetingsCoreSpy(finalTranscript: transcript)
    core.submitJobResult = FfiBackendJob(
        id: "", meetingId: "", kind: "", status: "", error: "connection refused", artifactIds: []
    )
    let viewModel = MeetingsViewModel(core: core)
    viewModel.reload(meetingId: "meeting-1")

    await viewModel.performBackendRefine(meetingId: "meeting-1")

    XCTAssertEqual(viewModel.backendJobStatus, .failed)
    XCTAssertEqual(viewModel.errorMessage, "connection refused")
    XCTAssertEqual(core.getBackendArtifactCallCount, 0)
}

func testSubmitBackendRefineRequiresFinalTranscript() async {
    let core = MeetingsCoreSpy()
    let viewModel = MeetingsViewModel(core: core)
    viewModel.reload(meetingId: "meeting-1")

    await viewModel.performBackendRefine(meetingId: "meeting-1")

    XCTAssertEqual(viewModel.backendJobStatus, .failed)
    XCTAssertEqual(viewModel.errorMessage, "Нужен Final transcript")
    XCTAssertEqual(core.submitBackendJobCallCount, 0)
}

func testSubmitBackendRefineTimesOutWhileQueued() async {
    let transcript = makeTranscript(meetingId: "meeting-1")
    let core = MeetingsCoreSpy(finalTranscript: transcript)
    core.submitJobResult = FfiBackendJob(
        id: "job-1",
        meetingId: "meeting-1",
        kind: "refine",
        status: "queued",
        error: "",
        artifactIds: []
    )
    core.getJobResults = [
        FfiBackendJob(
            id: "job-1", meetingId: "meeting-1", kind: "refine",
            status: "queued", error: "", artifactIds: []
        ),
        FfiBackendJob(
            id: "job-1", meetingId: "meeting-1", kind: "refine",
            status: "running", error: "", artifactIds: []
        ),
    ]
    let viewModel = MeetingsViewModel(core: core, maxPollAttempts: 2, pollDelayNanoseconds: 0)
    viewModel.reload(meetingId: "meeting-1")

    await viewModel.performBackendRefine(meetingId: "meeting-1")

    XCTAssertEqual(viewModel.backendJobStatus, .failed)
    XCTAssertEqual(viewModel.errorMessage, "Backend job timeout")
    XCTAssertEqual(core.getBackendArtifactCallCount, 0)
}
```

- [ ] **Step 3: Run tests — expect FAIL**

```bash
cd apps/macos && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug \
  test CODE_SIGNING_ALLOWED=NO -only-testing:MeetingRaftTests/MeetingsViewModelTests
```

Expected: compile errors / missing `performBackendRefine` / status enum.

- [ ] **Step 4: Implement ViewModel**

Добавить enum и поля/методы (эскиз логики):

```swift
enum BackendRefineStatus: String, Equatable {
    case idle
    case submitting
    case polling
    case succeeded
    case failed
}

// properties:
private(set) var backendJobStatus: BackendRefineStatus = .idle
private(set) var backendJobId = ""
private(set) var backendArtifactMarkdown = ""
private var backendRefineTask: Task<Void, Never>?
private let maxPollAttempts: Int
private let pollDelayNanoseconds: UInt64

init(
    core: any MeetingsCoreProviding,
    maxPollAttempts: Int = 20,
    pollDelayNanoseconds: UInt64 = 250_000_000
) {
    self.core = core
    self.maxPollAttempts = maxPollAttempts
    self.pollDelayNanoseconds = pollDelayNanoseconds
}

func submitBackendRefine(meetingId: String) {
    backendRefineTask?.cancel()
    backendRefineTask = Task { @MainActor [weak self] in
        await self?.performBackendRefine(meetingId: meetingId)
    }
}

func resetBackendRefineSession() {
    backendRefineTask?.cancel()
    backendRefineTask = nil
    backendJobStatus = .idle
    backendJobId = ""
    backendArtifactMarkdown = ""
}

func performBackendRefine(meetingId: String) async {
    guard finalTranscript != nil else {
        backendJobStatus = .failed
        errorMessage = "Нужен Final transcript"
        return
    }

    backendJobStatus = .submitting
    backendArtifactMarkdown = ""
    errorMessage = nil

    let job = core.submitBackendJob(meetingId: meetingId, kindCode: "refine")
    if !job.error.isEmpty {
        backendJobStatus = .failed
        backendJobId = job.id
        errorMessage = job.error
        return
    }

    backendJobId = job.id
    var current = job

    if current.status != "succeeded" {
        backendJobStatus = .polling
        var attempts = 0
        while attempts < maxPollAttempts {
            if Task.isCancelled { return }
            if pollDelayNanoseconds > 0 {
                try? await Task.sleep(nanoseconds: pollDelayNanoseconds)
            }
            if Task.isCancelled { return }
            current = core.getBackendJob(jobId: current.id)
            if !current.error.isEmpty {
                backendJobStatus = .failed
                errorMessage = current.error
                return
            }
            if current.status == "failed" {
                backendJobStatus = .failed
                errorMessage = current.error.isEmpty ? "Backend job failed" : current.error
                return
            }
            if current.status == "succeeded" { break }
            attempts += 1
        }
        if current.status != "succeeded" {
            backendJobStatus = .failed
            errorMessage = "Backend job timeout"
            return
        }
    }

    guard let artifactId = current.artifactIds.first else {
        backendJobStatus = .failed
        errorMessage = "Backend job has no artifacts"
        return
    }

    let artifact = core.getBackendArtifact(artifactId: artifactId)
    if !artifact.error.isEmpty {
        backendJobStatus = .failed
        errorMessage = artifact.error
        return
    }

    backendArtifactMarkdown = artifact.bodyMarkdown
    backendJobStatus = .succeeded
}
```

Важно: `reload(meetingId:)` **не** вызывает `resetBackendRefineSession()`.

- [ ] **Step 5: Run tests — expect PASS**

Та же команда `xcodebuild … -only-testing:MeetingRaftTests/MeetingsViewModelTests`.  
Expected: PASS для четырёх новых + старых Meetings тестов.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/Sources/Meetings/MeetingsViewModel.swift \
        apps/macos/Tests/MeetingsViewModelTests.swift
git commit -m "$(cat <<'EOF'
feat: poll stub refine job в MeetingsViewModel

EOF
)"
```

---

### Task 2: Artifacts UI

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingDetailView.swift`

**Interfaces:**
- Consumes: `submitBackendRefine`, `resetBackendRefineSession`, `backendJobStatus`, `backendJobId`, `backendArtifactMarkdown`
- Produces: UI only

- [ ] **Step 1: Wire button + panel + onDisappear**

В `artifacts` HStack кнопок добавить:

```swift
Button("Submit refine (stub)", systemImage: "cloud") {
    viewModel.submitBackendRefine(meetingId: meeting.id)
}
.help("Отправить refine job на backend stub")
.disabled(
    viewModel.finalTranscript == nil
        || viewModel.backendJobStatus == .submitting
        || viewModel.backendJobStatus == .polling
)
```

Под `HSplitView` (после закрывающей скобки split, всё ещё внутри `artifacts` `VStack`):

```swift
Divider()

backendRefinePanel
```

Добавить:

```swift
private var backendRefinePanel: some View {
    VStack(alignment: .leading, spacing: 8) {
        Text("Backend refine (stub)")
            .font(.headline)
        Text("Stub job refine · не заменяет local Brief")
            .font(.caption)
            .foregroundStyle(.secondary)
        Text("Status: \(viewModel.backendJobStatus.rawValue)")
            .font(.caption.monospaced())
        if !viewModel.backendJobId.isEmpty {
            Text("Job: \(viewModel.backendJobId)")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
        if !viewModel.backendArtifactMarkdown.isEmpty {
            HStack {
                Spacer()
                Button("Copy", systemImage: "doc.on.doc") {
                    copy(viewModel.backendArtifactMarkdown)
                }
            }
            ScrollView {
                Text(markdown(viewModel.backendArtifactMarkdown))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 120, maxHeight: 220)
        }
    }
    .padding()
}
```

На корневой `VStack` detail (рядом с `.onAppear`):

```swift
.onDisappear {
    viewModel.resetBackendRefineSession()
}
```

- [ ] **Step 2: Lint**

```bash
cd apps/macos && swiftformat Sources Tests --lint
```

Expected: 0 files require formatting. Если нет — `swiftformat Sources Tests`.

- [ ] **Step 3: Commit**

```bash
git add apps/macos/Sources/Meetings/MeetingDetailView.swift
git commit -m "$(cat <<'EOF'
feat: UI Submit refine (stub) на Artifacts

EOF
)"
```

---

### Task 3: Docs + verify

**Files:**
- Modify: `docs/backlog.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/architecture-and-install.md`

- [ ] **Step 1: Docs**

`docs/backlog.md` — у пункта Backend HTTP добавить отмеченный подпункт:

```markdown
- [x] Meetings UI: Submit refine (stub) → poll → show artifact
  (`feat/meetings-backend-refine-stub`)
```

(оставить slice A API как `[x]` если уже отмечен после merge stub; если там ещё `[ ]` — отметить `[x]` для stub + UI.)

`docs/roadmap.md` Phase 6 follow-up — дописать что Meetings poll UI done на этой ветке.

`docs/architecture-and-install.md`:
- В диаграмме/потоке Settings→Jobs добавить Meetings Artifacts → Submit refine.
- В процедуре: после Test API — опционально Meetings → Artifacts → Submit refine (stub).

- [ ] **Step 2: Full macOS tests + format**

```bash
cd apps/macos && swiftformat Sources Tests --lint
xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

Expected: all tests PASS.

- [ ] **Step 3: Manual smoke (если docker доступен)**

```bash
docker compose up -d
# Settings: apiBaseUrl http://127.0.0.1:8080 + token → Test API OK
# Meetings → meeting with Final → Artifacts → Submit refine (stub) → markdown
```

- [ ] **Step 4: Commit**

```bash
git add docs/backlog.md docs/roadmap.md docs/architecture-and-install.md
git commit -m "$(cat <<'EOF'
docs: Meetings ↔ stub refine e2e в backlog/roadmap/install

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| Protocol extension | Task 1 |
| Poll 20×250ms + immediate succeeded | Task 1 |
| Happy / submit error / no Final / timeout tests | Task 1 |
| Button + panel + provenance caption | Task 2 |
| onDisappear reset | Task 2 |
| reload не сбрасывает backend state | Task 1 (explicit non-call) |
| Docs touchpoints | Task 3 |
| Local Brief untouched | Task 2 (no generate changes) |
