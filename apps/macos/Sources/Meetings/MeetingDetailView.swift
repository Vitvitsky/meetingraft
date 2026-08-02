import AppKit
import Foundation
import SwiftUI

/// Сохранённые live/final данные и post-call артефакты встречи.
struct MeetingDetailView: View {
    let meeting: FfiMeetingSummary
    @Bindable var viewModel: MeetingsViewModel
    @Environment(ProviderSettingsStore.self) private var providerStore

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
            case .artifacts:
                artifacts
            }
        }
        .navigationTitle(String(meeting.id.prefix(8)))
        .onAppear {
            viewModel.reload(meetingId: meeting.id)
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

    @ViewBuilder
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
                    viewModel.generate(meetingId: meeting.id, kind: .brief)
                }
                .help(generateHelp)
                Button("Generate Follow-up", systemImage: "envelope") {
                    viewModel.generate(meetingId: meeting.id, kind: .followUp)
                }
                .help(generateHelp)
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
        }
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
        case .artifacts:
            "Artifacts"
        }
    }
}
