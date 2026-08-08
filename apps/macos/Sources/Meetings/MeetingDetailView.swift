import AppKit
import Foundation
import SwiftUI

/// Сохранённые live/final данные и post-call артефакты встречи.
struct MeetingDetailView: View {
    let meeting: FfiMeetingSummary
    @Bindable var viewModel: MeetingsViewModel
    @Environment(ProviderSettingsStore.self) private var providerStore
    @Environment(SessionLanguageStore.self) private var languageStore

    @State private var section: MeetingDetailSection = .live
    @State private var rebuild: FinalRebuildViewModel
    @State private var attribution: SpeakerAttributionViewModel

    init(meeting: FfiMeetingSummary, viewModel: MeetingsViewModel, core: MeetingCore) {
        self.meeting = meeting
        self.viewModel = viewModel
        _rebuild = State(initialValue: FinalRebuildViewModel(core: core))
        _attribution = State(initialValue: SpeakerAttributionViewModel(core: core))
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("Раздел", selection: $section) {
                ForEach(MeetingDetailSection.allCases) { section in
                    Text(section.title).tag(section)
                }
            }
            .pickerStyle(.segmented)
            .padding()

            Divider()

            switch section {
            case .live:
                liveCaptions
            case .final:
                finalTranscript
            case .compare:
                comparePanel
            case .speakers:
                speakersPanel
            case .artifacts:
                artifacts
            }
        }
        .navigationTitle(viewModel.displayTitle(for: meeting))
        .onAppear {
            applyProviderConfig()
            viewModel.reload(meetingId: meeting.id)
            reloadAttribution()
            rebuild.attach(meetingId: meeting.id)
        }
        .onDisappear {
            viewModel.resetBackendRefineSession()
            rebuild.stopPolling()
        }
        .onChange(of: viewModel.selectedFinalVersion) { _, _ in
            reloadAttribution()
        }
        .onChange(of: section) { _, newValue in
            // Правки в Final и Speakers переписывают текст транскрипта, а
            // признак отставания артефакта считается при чтении. Без
            // перечитывания плашка появилась бы только при повторном
            // заходе во встречу — то есть почти никогда.
            if newValue == .artifacts {
                viewModel.reload(meetingId: meeting.id)
            }
        }
        .onChange(of: rebuild.state) { _, newValue in
            // Проход закончился — перечитываем: появилась новая версия.
            if newValue == "succeeded" {
                viewModel.reload(meetingId: meeting.id)
                reloadAttribution()
            }
        }
        .alert(
            "Ошибка Meetings",
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
            "Ошибка атрибуции",
            isPresented: Binding(
                get: { attribution.errorMessage != nil },
                set: {
                    if !$0 {
                        attribution.dismissError()
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(attribution.errorMessage ?? "")
        }
    }

    private func reloadAttribution() {
        attribution.load(meetingId: meeting.id, version: viewModel.selectedFinalVersion)
    }

    private var liveCaptions: some View {
        VStack(spacing: 0) {
            provenanceBanner(
                "Источник: Live STT · не используется для Brief / Follow-up"
            )
            List(viewModel.captions, id: \.id) { caption in
                switch caption.phase {
                case .partial:
                    Text(caption.text)
                        .italic()
                        .foregroundStyle(.secondary)
                case .final:
                    Text(caption.text)
                }
            }
            .overlay {
                if viewModel.captions.isEmpty {
                    ContentUnavailableView(
                        "Live-транскрипт пуст",
                        systemImage: "captions.bubble"
                    )
                }
            }
        }
    }

    private var speakersPanel: some View {
        VStack(spacing: 0) {
            provenanceBanner(speakersProvenance)
            SpeakersPanelView(
                viewModel: attribution,
                primaryLanguage: languageStore.primary.rawValue
            )
        }
    }

    /// Provenance говорит, откуда взялась атрибуция: по дорожкам она
    /// точна, вручную — ровно настолько, насколько её проставили.
    private var speakersProvenance: String {
        attribution.hasSegments
            ? "Атрибуция по дорожкам записи · правки вручную сохраняются"
            : "Ручные метки · атрибуция по дорожкам появится после пересбора Final"
    }

    private var finalTranscript: some View {
        VStack(spacing: 0) {
            provenanceBanner(finalProvenance)
            rebuildBar
            if viewModel.finalVersions.isEmpty {
                ContentUnavailableView(
                    "Финальный транскрипт недоступен",
                    systemImage: "doc.text.magnifyingglass"
                )
            } else {
                finalVersionPicker
                Divider()
                finalBody
            }
        }
    }

    /// Версии до re-ASR (ADR-011) сегментов не имеют — показываем их
    /// абзацами, а не пустым списком.
    @ViewBuilder
    private var finalBody: some View {
        if attribution.hasSegments {
            FinalSegmentsView(viewModel: attribution)
        } else {
            ScrollView {
                Text(markdown(viewModel.selectedFinalBody))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
            }
        }
    }

    /// Provenance называет то, что фактически отработало: после
    /// настоящего прохода — его, до него — честную сборку из live-финалов.
    private var finalProvenance: String {
        rebuild.provenance.isEmpty
            ? "Источник: Live finals + glossary · вход для Brief / Follow-up"
            : "Источник: \(rebuild.provenance) · вход для Brief / Follow-up"
    }

    private var rebuildBar: some View {
        HStack(spacing: 12) {
            if rebuild.isRunning {
                ProgressView(value: rebuild.fraction)
                    .frame(width: 140)
                Button("Cancel") { rebuild.cancel() }
            } else {
                Button("Rebuild Final", systemImage: "arrow.clockwise") {
                    rebuild.start(meetingId: meeting.id)
                }
                .help("Повторно распознать сохранённое аудио большой моделью")
            }
            if !rebuild.statusText.isEmpty {
                Text(rebuild.statusText)
                    .font(.caption)
                    .foregroundStyle(rebuild.state == "failed" ? Color.red : Color.secondary)
                    .textSelection(.enabled)
            }
            Spacer()
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    private var comparePanel: some View {
        HSplitView {
            VStack(spacing: 0) {
                provenanceBanner("Live finals")
                let liveText = viewModel.liveFinalsText(from: viewModel.captions)
                ScrollView {
                    Text(liveText)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding()
                }
                .overlay {
                    if liveText.isEmpty {
                        ContentUnavailableView(
                            "Live finals пусты",
                            systemImage: "captions.bubble"
                        )
                    }
                }
            }
            .frame(minWidth: 220)

            VStack(spacing: 0) {
                provenanceBanner("Final")
                if viewModel.finalVersions.isEmpty {
                    ContentUnavailableView(
                        "Финальный транскрипт недоступен",
                        systemImage: "doc.text.magnifyingglass"
                    )
                } else {
                    finalVersionPicker
                    Divider()
                    ScrollView {
                        Text(markdown(viewModel.selectedFinalBody))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding()
                    }
                }
            }
            .frame(minWidth: 220)
        }
    }

    private var finalVersionPicker: some View {
        Picker("Версия Final", selection: $viewModel.selectedFinalVersion) {
            ForEach(viewModel.finalVersions, id: \.version) { transcript in
                Text(finalVersionLabel(transcript)).tag(Optional(transcript.version))
            }
        }
        .pickerStyle(.menu)
        .padding(.horizontal)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Метка picker: `v{N} · {short date/time}` из `createdAtMs`.
    private func finalVersionLabel(_ transcript: FfiFinalTranscript) -> String {
        "v\(transcript.version) · \(shortDateTime(fromMs: transcript.createdAtMs))"
    }

    private func shortDateTime(fromMs timestampMs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
        return date.formatted(date: .numeric, time: .shortened)
    }

    private var artifacts: some View {
        VStack(spacing: 0) {
            provenanceBanner(providerStore.artifactsPipelineCaption)

            HStack {
                Button("Generate Brief", systemImage: "doc.text") {
                    applyProviderConfig()
                    Task { await viewModel.generate(meetingId: meeting.id, kind: .brief) }
                }
                .help(generateHelp)
                .disabled(!canGenerateArtifacts || viewModel.isGeneratingArtifact)
                Button("Generate Follow-up", systemImage: "envelope") {
                    applyProviderConfig()
                    Task { await viewModel.generate(meetingId: meeting.id, kind: .followUp) }
                }
                .help(generateHelp)
                .disabled(!canGenerateArtifacts || viewModel.isGeneratingArtifact)
                Button("Submit refine (stub)", systemImage: "cloud") {
                    viewModel.submitBackendRefine(meetingId: meeting.id)
                }
                .help("Отправить refine job на backend stub")
                .disabled(
                    viewModel.finalTranscript == nil
                        || viewModel.backendJobStatus == .submitting
                        || viewModel.backendJobStatus == .polling
                )
                Button("Export to Markdown", systemImage: "square.and.arrow.up") {
                    exportMarkdown()
                }
                .help(exportHelp)
                .disabled(viewModel.finalTranscript == nil)
                Button("Choose folder…") {
                    chooseExportFolderAndExport()
                }
                .help("Выбрать папку и экспортировать")
                .disabled(viewModel.finalTranscript == nil)
                // Генерация больше не морозит окно, поэтому ожидание надо
                // показать: иначе нажатие выглядит как ничего не сделавшее.
                if viewModel.isGeneratingArtifact {
                    ProgressView()
                        .controlSize(.small)
                    Text("Generating…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding()

            if let catalogHelp = backendCatalogGenerateHelp {
                Text(catalogHelp)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal)
                    .padding(.bottom, 8)
            }

            if !viewModel.exportStatusMessage.isEmpty {
                Text(viewModel.exportStatusMessage)
                    .font(.caption)
                    .foregroundStyle(exportStatusColor)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal)
                    .padding(.bottom, 8)
                    .textSelection(.enabled)
            }

            Divider()

            HSplitView {
                List(viewModel.artifacts, id: \.id) { artifact in
                    Button {
                        viewModel.selectArtifact(artifact)
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(spacing: 4) {
                                Text(artifactTitle(artifact.kind))
                                    .font(.headline)
                                // Видно до открытия: иначе расхождение
                                // находится только случайным кликом.
                                if artifact.isStale {
                                    Image(systemName: "exclamationmark.triangle.fill")
                                        .font(.caption)
                                        .foregroundStyle(Theme.warning)
                                        .help("Собран до правок транскрипта")
                                }
                            }
                            Text(artifactDate(artifact.createdAtMs))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .frame(minWidth: 180, idealWidth: 220)
                .overlay {
                    if viewModel.artifacts.isEmpty {
                        ContentUnavailableView(
                            "Артефактов нет",
                            systemImage: "doc.badge.plus"
                        )
                    }
                }

                artifactBody
                    .frame(minWidth: 320)
            }

            Divider()

            backendRefinePanel
        }
    }

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

    private var canGenerateArtifacts: Bool {
        viewModel.finalTranscript != nil && providerStore.allowsArtifactGeneration
    }

    private var backendCatalogGenerateHelp: String? {
        guard providerStore.llmEngine == .backend, !providerStore.allowsArtifactGeneration else {
            return nil
        }
        return providerStore.backendCatalogMissingHelp
    }

    private var generateHelp: String {
        if viewModel.finalTranscript == nil {
            return "Нужен Final transcript"
        }
        if let backendCatalogGenerateHelp {
            return backendCatalogGenerateHelp
        }
        return "Собрать артефакт из Final"
    }

    private var exportHelp: String {
        viewModel.finalTranscript == nil
            ? "Нужен Final transcript"
            : "Экспорт Final и артефактов в \(providerStore.exportFolderPath)"
    }

    private var exportStatusColor: Color {
        viewModel.exportStatusMessage.hasPrefix("Exported") ? .secondary : .red
    }

    private func expandedExportFolderURL() -> URL {
        let path = NSString(string: providerStore.exportFolderPath).expandingTildeInPath
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    private func exportMarkdown(to folderURL: URL? = nil) {
        let url = folderURL ?? expandedExportFolderURL()
        _ = viewModel.exportMarkdown(
            meetingId: meeting.id,
            startedAtMs: meeting.startedAtMs,
            folderURL: url
        )
    }

    private func chooseExportFolderAndExport() {
        guard let url = DirectoryPicker.chooseDirectory(prompt: "Export") else { return }
        providerStore.exportFolderPath = url.path
        exportMarkdown(to: url)
    }

    @ViewBuilder
    private var artifactBody: some View {
        if let artifact = viewModel.selectedArtifact {
            VStack(spacing: 0) {
                HStack {
                    Text(artifactTitle(artifact.kind))
                        .font(.headline)
                    Spacer()
                    Button("Copy", systemImage: "doc.on.doc") {
                        copy(artifact.bodyMarkdown)
                    }
                }
                .padding()

                if artifact.isStale {
                    stalenessBanner(artifact)
                }

                Divider()

                ScrollView {
                    Text(markdown(artifact.bodyMarkdown))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding()
                }
            }
        } else {
            ContentUnavailableView(
                "Выберите артефакт",
                systemImage: "doc.text"
            )
        }
    }

    /// Транскрипт изменился после сборки артефакта.
    ///
    /// Пересборка предлагается, но не делается сама: текст мог быть уже
    /// отправлен или доработан руками, и молча его переписать — та же
    /// ошибка, что и молча оставить расхождение.
    private func stalenessBanner(_ artifact: FfiArtifact) -> some View {
        HStack(spacing: Theme.Space.sm) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(Theme.warning)
            VStack(alignment: .leading, spacing: 2) {
                Text("Собран до правок транскрипта")
                    .font(.caption.weight(.semibold))
                Text(stalenessDetail(artifact))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Пересобрать") {
                applyProviderConfig()
                Task { await viewModel.generate(meetingId: meeting.id, kind: artifact.kind) }
            }
            .disabled(!canGenerateArtifacts || viewModel.isGeneratingArtifact)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Theme.warning.opacity(0.12))
    }

    /// Что именно разошлось: другая версия Final или правка внутри той же.
    private func stalenessDetail(_ artifact: FfiArtifact) -> String {
        guard let current = viewModel.finalTranscript else {
            return "Текст транскрипта изменился после сборки"
        }
        if artifact.sourceVersion != current.version {
            return "Собран по версии \(artifact.sourceVersion), сейчас \(current.version)"
        }
        return "Версия та же, но текст правился после сборки"
    }

    private func provenanceBanner(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal)
            .padding(.vertical, 8)
            .background(Color.primary.opacity(0.04))
    }

    private func applyProviderConfig() {
        viewModel.applyProviderConfig(
            apiBaseUrl: providerStore.apiBaseUrl,
            apiToken: providerStore.apiToken,
            llmEngineCode: providerStore.llmEngine.rawValue,
            llmModelId: providerStore.llmModelId,
            llmBaseUrl: providerStore.llmBaseUrl,
            llmProviderId: providerStore.llmProviderId
        )
    }

    private func markdown(_ source: String) -> AttributedString {
        (try? AttributedString(markdown: source)) ?? AttributedString(source)
    }

    private func artifactTitle(_ kind: FfiArtifactKind) -> String {
        switch kind {
        case .brief:
            "Brief"
        case .followUp:
            "Follow-up"
        }
    }

    private func artifactDate(_ timestampMs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
        return date.formatted(date: .abbreviated, time: .shortened)
    }

    private func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

private enum MeetingDetailSection: String, CaseIterable, Identifiable {
    case live
    case final
    case compare
    case speakers
    case artifacts

    var id: String {
        rawValue
    }

    var title: String {
        switch self {
        case .live:
            "Live"
        case .final:
            "Final"
        case .compare:
            "Compare"
        case .speakers:
            "Speakers"
        case .artifacts:
            "Artifacts"
        }
    }
}
