import Foundation
import SwiftUI

extension FfiMeetingSummary: Identifiable {}

/// Библиотека встреч: список, поиск, переименование, удаление.
struct MeetingsListView: View {
    @Bindable var viewModel: MeetingsViewModel
    /// Нужен детали встречи для пересбора Final; список его не использует.
    let core: MeetingCore

    @State private var renaming: FfiMeetingSummary?
    @State private var renameDraft = ""
    @State private var pendingDeletion: FfiMeetingSummary?

    private var isSearchActive: Bool {
        !viewModel.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        NavigationStack {
            Group {
                if isSearchActive {
                    searchResults
                } else {
                    meetingList
                }
            }
            .navigationTitle("Meetings")
            .searchable(
                text: $viewModel.query,
                placement: .toolbar,
                prompt: Text("Search transcripts and artifacts")
            )
            .navigationDestination(for: FfiMeetingSummary.self) { meeting in
                MeetingDetailView(meeting: meeting, viewModel: viewModel, core: core)
            }
            .onAppear {
                viewModel.reload()
            }
        }
        .sheet(item: $renaming) { meeting in
            renameSheet(meeting)
        }
        .confirmationDialog(
            "Delete this meeting?",
            isPresented: Binding(
                get: { pendingDeletion != nil },
                set: {
                    if !$0 {
                        pendingDeletion = nil
                    }
                }
            ),
            presenting: pendingDeletion
        ) { meeting in
            Button("Delete", role: .destructive) {
                viewModel.delete(meetingId: meeting.id)
                pendingDeletion = nil
            }
            Button("Cancel", role: .cancel) {
                pendingDeletion = nil
            }
        } message: { _ in
            Text("Audio, live captions, transcripts and artifacts will be removed. This cannot be undone.")
        }
    }

    private var meetingList: some View {
        List(viewModel.meetings, id: \.id) { meeting in
            NavigationLink(value: meeting) {
                meetingRow(meeting)
            }
            .contextMenu {
                Button("Rename") {
                    renameDraft = viewModel.displayTitle(for: meeting)
                    renaming = meeting
                }
                Button("Delete", role: .destructive) {
                    pendingDeletion = meeting
                }
            }
        }
        .overlay {
            if viewModel.meetings.isEmpty {
                ContentUnavailableView(
                    "No meetings yet",
                    systemImage: "calendar",
                    description: Text("Finished recordings show up here")
                )
            }
        }
    }

    private var searchResults: some View {
        List(viewModel.searchHits, id: \.refId) { hit in
            if let meeting = viewModel.meetings.first(where: { $0.id == hit.meetingId }) {
                NavigationLink(value: meeting) {
                    searchRow(hit, meeting: meeting)
                }
            } else {
                searchRow(hit, meeting: nil)
            }
        }
        .overlay {
            if viewModel.searchHits.isEmpty, !viewModel.isSearching {
                ContentUnavailableView.search(text: viewModel.query)
            }
        }
    }

    private func meetingRow(_ meeting: FfiMeetingSummary) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(viewModel.displayTitle(for: meeting))
                .font(.headline)
                .lineLimit(1)

            HStack(spacing: 8) {
                Text(meetingDate(meeting.startedAtMs))
                if let duration = viewModel.duration(for: meeting) {
                    Text("·")
                    Text(duration.formatted(.time(pattern: .hourMinute)))
                }
                if meeting.hasFinal {
                    Label("Final", systemImage: "checkmark.seal.fill")
                        .foregroundStyle(.green)
                }
                if meeting.artifactCount > 0 {
                    Label("\(meeting.artifactCount)", systemImage: "doc.text.fill")
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }

    private func searchRow(_ hit: FfiSearchHit, meeting: FfiMeetingSummary?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(meeting.map(viewModel.displayTitle) ?? hit.meetingId)
                    .font(.headline)
                    .lineLimit(1)
                Spacer()
                Text(sectionLabel(hit.kind))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text(hit.snippet)
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(3)
        }
        .padding(.vertical, 4)
    }

    /// Куда ведёт результат — совпадает с вкладками детали встречи.
    private func sectionLabel(_ kind: String) -> String {
        switch kind {
        case "final": String(localized: "Final")
        case "artifact": String(localized: "Artifacts")
        default: String(localized: "Live")
        }
    }

    private func renameSheet(_ meeting: FfiMeetingSummary) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Rename meeting")
                .font(.title3)
            TextField("Title", text: $renameDraft)
                .textFieldStyle(.roundedBorder)
                .onSubmit { commitRename(meeting) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { renaming = nil }
                Button("Save") { commitRename(meeting) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(renameDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding()
        .frame(width: 380)
    }

    private func commitRename(_ meeting: FfiMeetingSummary) {
        viewModel.rename(meetingId: meeting.id, title: renameDraft)
        renaming = nil
    }

    private func meetingDate(_ timestampMs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}
