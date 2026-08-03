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
            case .speakers:
                speakersPanel
            case .artifacts:
                artifacts
            }
        }
        .navigationTitle(String(meeting.id.prefix(8)))
        .onAppear {
            applyProviderConfig()
            viewModel.reload(meetingId: meeting.id)
        }
        .onDisappear {
            viewModel.resetBackendRefineSession()
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
            provenanceBanner("Ручные метки · diarization — скоро")

            HStack {
                Button("Add", systemImage: "person.badge.plus") {
                    viewModel.addSpeaker(
                        meetingId: meeting.id,
                        primaryLanguage: languageStore.primary.rawValue
                    )
                }
                Spacer()
            }
            .padding()

            Divider()

            List {
                ForEach(viewModel.speakers, id: \.id) { speaker in
                    SpeakerRow(
                        speaker: speaker,
                        onRename: { displayName in
                            viewModel.renameSpeaker(
                                meetingId: meeting.id,
                                id: speaker.id,
                                displayName: displayName
                            )
                        },
                        onDelete: {
                            viewModel.removeSpeaker(
                                id: speaker.id,
                                meetingId: meeting.id
                            )
                        }
                    )
                }
            }
            .overlay {
                if viewModel.speakers.isEmpty {
                    ContentUnavailableView(
                        "Спикеров нет",
                        systemImage: "person.3"
                    )
                }
            }
        }
    }

    private var finalTranscript: some View {
        VStack(spacing: 0) {
            provenanceBanner(
                "Источник: Live finals + glossary · вход для Brief / Follow-up"
            )
            if let transcript = viewModel.finalTranscript {
                ScrollView {
                    Text(markdown(transcript.bodyMarkdown))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding()
                }
            } else {
                ContentUnavailableView(
                    "Финальный транскрипт недоступен",
                    systemImage: "doc.text.magnifyingglass"
                )
            }
        }
    }

    private var artifacts: some View {
        VStack(spacing: 0) {
            provenanceBanner(providerStore.artifactsPipelineCaption)

            HStack {
                Button("Generate Brief", systemImage: "doc.text") {
                    applyProviderConfig()
                    viewModel.generate(meetingId: meeting.id, kind: .brief)
                }
                .help(generateHelp)
                Button("Generate Follow-up", systemImage: "envelope") {
                    applyProviderConfig()
                    viewModel.generate(meetingId: meeting.id, kind: .followUp)
                }
                .help(generateHelp)
                Button("Submit refine (stub)", systemImage: "cloud") {
                    viewModel.submitBackendRefine(meetingId: meeting.id)
                }
                .help("Отправить refine job на backend stub")
                .disabled(
                    viewModel.finalTranscript == nil
                        || viewModel.backendJobStatus == .submitting
                        || viewModel.backendJobStatus == .polling
                )
                Spacer()
            }
            .disabled(viewModel.finalTranscript == nil)
            .padding()

            Divider()

            HSplitView {
                List(viewModel.artifacts, id: \.id) { artifact in
                    Button {
                        viewModel.selectArtifact(artifact)
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(artifactTitle(artifact.kind))
                                .font(.headline)
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

    private var generateHelp: String {
        viewModel.finalTranscript == nil
            ? "Нужен Final transcript"
            : "Собрать артефакт из Final"
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
            llmBaseUrl: providerStore.llmBaseUrl
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
        case .speakers:
            "Speakers"
        case .artifacts:
            "Artifacts"
        }
    }
}

private struct SpeakerRow: View {
    let speaker: FfiSpeaker
    let onRename: (String) -> Void
    let onDelete: () -> Void

    @State private var displayName: String

    init(
        speaker: FfiSpeaker,
        onRename: @escaping (String) -> Void,
        onDelete: @escaping () -> Void
    ) {
        self.speaker = speaker
        self.onRename = onRename
        self.onDelete = onDelete
        _displayName = State(initialValue: speaker.displayName)
    }

    var body: some View {
        HStack {
            TextField("Speaker name", text: $displayName)
                .onSubmit {
                    onRename(displayName)
                }
            Button("Delete", systemImage: "trash", role: .destructive) {
                onDelete()
            }
            .labelStyle(.iconOnly)
        }
        .onChange(of: speaker.displayName) { _, newValue in
            displayName = newValue
        }
    }
}
