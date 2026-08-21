import SwiftUI
import UniformTypeIdentifiers

/// Глоссарий: области, поиск, термины (ТЗ редизайна §4.5).
///
/// Глоссарий — то, что накапливается годами и делает переход к другому
/// продукту дорогим. Поэтому здесь важнее не красота строки, а то, чтобы
/// нужный термин находился среди сотен.
struct GlossaryView: View {
    @Bindable var viewModel: GlossaryViewModel
    let liveSessionId: String?

    @State private var editorDraft: GlossaryEditorDraft?
    @State private var isImportPresented = false

    private var visible: [FfiGlossaryTerm] {
        viewModel.visibleTerms(liveSessionId: liveSessionId)
    }

    var body: some View {
        VStack(spacing: 0) {
            scopeBar
            Divider().overlay(Theme.borderSubtle)
            termList
        }
        .background(Theme.surfaceRoot)
        .navigationTitle("Glossary")
        .searchable(
            text: $viewModel.query,
            placement: .toolbar,
            prompt: Text("Search terms")
        )
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

    // MARK: - Области

    private var scopeBar: some View {
        HStack(spacing: Theme.Space.xs) {
            ForEach(GlossaryFilter.allCases) { item in
                let count = viewModel.count(for: item, liveSessionId: liveSessionId)
                Button {
                    viewModel.filter = item
                } label: {
                    Chip(
                        text: count > 0 ? "\(item.title) \(count)" : item.title,
                        isSelected: viewModel.filter == item
                    )
                }
                .buttonStyle(.plain)
                // «Эта встреча» без записи пуста по определению — гасим,
                // чтобы не выглядело поломкой.
                .disabled(count == 0 && item != .all)
                .opacity(count == 0 && item != .all ? 0.4 : 1)
            }
            Spacer()
        }
        .padding(.horizontal, Theme.Space.md)
        .padding(.vertical, Theme.Space.xs)
        .background(Theme.surface)
    }

    // MARK: - Термины

    private var termList: some View {
        List {
            ForEach(visible, id: \.id) { term in
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
            if visible.isEmpty {
                emptyState
            }
        }
    }

    /// Пустой словарь, пустая выборка и пустой поиск — три разных
    /// сообщения: одинаковый текст на них вводит в заблуждение.
    @ViewBuilder
    private var emptyState: some View {
        if viewModel.terms.isEmpty {
            ContentUnavailableView(
                "Glossary is empty",
                systemImage: "book",
                description: Text("Add a term or import a CSV file")
            )
        } else if !viewModel.query.isEmpty {
            ContentUnavailableView.search(text: viewModel.query)
        } else {
            ContentUnavailableView(
                "Nothing in this scope",
                systemImage: "line.3.horizontal.decrease.circle",
                description: Text("Pick another scope")
            )
        }
    }

    private func glossaryRow(_ term: FfiGlossaryTerm) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Theme.Space.sm) {
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                HStack(spacing: Theme.Space.xs) {
                    Text(term.surface)
                        .font(Theme.Text.body)
                        .foregroundStyle(Theme.textSecondary)
                    Image(systemName: "arrow.right")
                        .font(Theme.Text.caption)
                        .foregroundStyle(Theme.textTertiary)
                    Text(term.canonical)
                        .font(Theme.Text.body.weight(.semibold))
                        .foregroundStyle(Theme.textPrimary)
                }
                Text(scopeDescription(term))
                    .font(Theme.Text.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            Spacer()
            Chip(text: term.language.uppercased())
        }
        .padding(.vertical, Theme.Space.xxs)
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            if viewModel.canEdit(term, liveSessionId: liveSessionId) {
                editorDraft = GlossaryEditorDraft(term: term)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(term.surface) → \(term.canonical), \(term.language.uppercased())")
    }

    private func scopeDescription(_ term: FfiGlossaryTerm) -> String {
        switch term.scope {
        case .global:
            String(localized: "Global")
        case .meeting:
            String(localized: "This meeting only")
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
                viewModel.showError(String(localized: "The CSV file must be UTF-8 encoded"))
                return
            }
            viewModel.importCsv(csv)
        } catch {
            viewModel.showError(String(localized: "Could not read the CSV: \(error.localizedDescription)"))
        }
    }
}

private struct GlossaryEditorDraft: Identifiable {
    let id: String
    var surface: String
    var canonical: String
    var language: SpeechLanguage
    var scope: GlossaryScopeSelection
    /// Вид записи термина. Экран его не показывает и не меняет: подсказка
    /// становится заменой только по отдельному жесту «заменять всюду».
    /// Здесь он лишь переносится обратно неизменным — иначе сохранение
    /// правки словаря молча превращало бы подсказку в замену.
    let kind: FfiGlossaryKind

    init() {
        id = UUID().uuidString
        surface = ""
        canonical = ""
        language = .ru
        scope = .global
        // Термин, заведённый руками на этом экране, — осознанная замена:
        // человек сам вписал обе формы.
        kind = .replacement
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
        kind = term.kind
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
                .buttonStyle(.themedSecondary)
                Button("Save") {
                    onSave(makeTerm())
                    dismiss()
                }
                .buttonStyle(.themedPrimary)
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
            meetingId: meetingId,
            kind: draft.kind
        )
    }
}
