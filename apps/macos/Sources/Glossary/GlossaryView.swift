import SwiftUI
import UniformTypeIdentifiers

/// Список терминов глоссария с CRUD и CSV-импортом.
struct GlossaryView: View {
    @Bindable var viewModel: GlossaryViewModel
    let liveSessionId: String?

    @State private var editorDraft: GlossaryEditorDraft?
    @State private var isImportPresented = false

    var body: some View {
        List {
            ForEach(viewModel.terms, id: \.id) { term in
                glossaryRow(term)
                    .contextMenu {
                        Button("Edit") {
                            editorDraft = GlossaryEditorDraft(term: term)
                        }
                        .disabled(!viewModel.canEdit(term, liveSessionId: liveSessionId))
                        Button("Delete", role: .destructive) {
                            viewModel.delete(id: term.id)
                        }
                        .disabled(!viewModel.canEdit(term, liveSessionId: liveSessionId))
                    }
            }
        }
        .overlay {
            if viewModel.terms.isEmpty {
                ContentUnavailableView(
                    "Glossary is empty",
                    systemImage: "book",
                    description: Text("Add a term or import a CSV file")
                )
            }
        }
        .navigationTitle("Glossary")
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button("Import CSV", systemImage: "square.and.arrow.down") {
                    isImportPresented = true
                }
                Button("Add", systemImage: "plus") {
                    editorDraft = GlossaryEditorDraft()
                }
            }
        }
        .onAppear {
            viewModel.reload()
        }
        .sheet(item: $editorDraft) { draft in
            GlossaryEditorView(
                draft: draft,
                availableScopes: viewModel.availableScopes(liveSessionId: liveSessionId),
                liveSessionId: liveSessionId
            ) { term in
                viewModel.upsert(term)
            }
        }
        .fileImporter(
            isPresented: $isImportPresented,
            allowedContentTypes: [.commaSeparatedText, .plainText],
            allowsMultipleSelection: false,
            onCompletion: importFile
        )
        .alert(
            "Glossary error",
            isPresented: Binding(
                get: { viewModel.errorMessage != nil },
                set: {
                    if !$0 {
                        viewModel.dismissError()
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(viewModel.errorMessage ?? "")
        }
        .alert(
            "CSV import",
            isPresented: Binding(
                get: { viewModel.importMessage != nil },
                set: {
                    if !$0 {
                        viewModel.dismissImportMessage()
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(viewModel.importMessage ?? "")
        }
    }

    private func glossaryRow(_ term: FfiGlossaryTerm) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(term.surface)
                    .font(.headline)
                Image(systemName: "arrow.right")
                    .foregroundStyle(.secondary)
                Text(term.canonical)
                Spacer()
                Text(term.language.uppercased())
                    .foregroundStyle(.secondary)
            }
            Text(scopeDescription(term))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            if viewModel.canEdit(term, liveSessionId: liveSessionId) {
                editorDraft = GlossaryEditorDraft(term: term)
            }
        }
    }

    private func scopeDescription(_ term: FfiGlossaryTerm) -> String {
        switch term.scope {
        case .global:
            "Global"
        case .meeting:
            "Meeting · \(term.meetingId)"
        }
    }

    private func importFile(_ result: Result<[URL], any Error>) {
        do {
            guard let url = try result.get().first else {
                return
            }
            let hasAccess = url.startAccessingSecurityScopedResource()
            defer {
                if hasAccess {
                    url.stopAccessingSecurityScopedResource()
                }
            }
            let data = try Data(contentsOf: url)
            guard let csv = String(data: data, encoding: .utf8) else {
                viewModel.showError("CSV-файл должен быть в кодировке UTF-8")
                return
            }
            viewModel.importCsv(csv)
        } catch {
            viewModel.showError("Не удалось прочитать CSV: \(error.localizedDescription)")
        }
    }
}

private struct GlossaryEditorDraft: Identifiable {
    let id: String
    var surface: String
    var canonical: String
    var language: SpeechLanguage
    var scope: GlossaryScopeSelection

    init() {
        id = UUID().uuidString
        surface = ""
        canonical = ""
        language = .ru
        scope = .global
    }

    init(term: FfiGlossaryTerm) {
        id = term.id
        surface = term.surface
        canonical = term.canonical
        language = SpeechLanguage(rawValue: term.language) ?? .ru
        scope = switch term.scope {
        case .global: .global
        case .meeting: .meeting
        }
    }
}

private struct GlossaryEditorView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var draft: GlossaryEditorDraft

    let availableScopes: [GlossaryScopeSelection]
    let liveSessionId: String?
    let onSave: (FfiGlossaryTerm) -> Void

    init(
        draft: GlossaryEditorDraft,
        availableScopes: [GlossaryScopeSelection],
        liveSessionId: String?,
        onSave: @escaping (FfiGlossaryTerm) -> Void
    ) {
        _draft = State(initialValue: draft)
        self.availableScopes = availableScopes
        self.liveSessionId = liveSessionId
        self.onSave = onSave
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Glossary term")
                .font(.title2)

            Form {
                TextField("Surface", text: $draft.surface)
                TextField("Canonical", text: $draft.canonical)
                Picker("Language", selection: $draft.language) {
                    ForEach(SpeechLanguage.allCases) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                Picker("Scope", selection: $draft.scope) {
                    ForEach(availableScopes) { scope in
                        Text(scope.title).tag(scope)
                    }
                }
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) {
                    dismiss()
                }
                Button("Save") {
                    onSave(makeTerm())
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!canSave)
            }
        }
        .padding()
        .frame(width: 420, height: 280)
    }

    private var canSave: Bool {
        !draft.surface.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !draft.canonical.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func makeTerm() -> FfiGlossaryTerm {
        let meetingId = draft.scope == .meeting ? liveSessionId ?? "" : ""
        return FfiGlossaryTerm(
            id: draft.id,
            surface: draft.surface.trimmingCharacters(in: .whitespacesAndNewlines),
            canonical: draft.canonical.trimmingCharacters(in: .whitespacesAndNewlines),
            language: draft.language.rawValue,
            scope: draft.scope == .meeting ? .meeting : .global,
            meetingId: meetingId
        )
    }
}
