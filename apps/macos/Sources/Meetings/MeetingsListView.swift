import Foundation
import SwiftUI

/// Локальная история встреч с переходом к сохранённым материалам.
struct MeetingsListView: View {
    @Bindable var viewModel: MeetingsViewModel

    var body: some View {
        NavigationStack {
            List(viewModel.meetings, id: \.id) { meeting in
                NavigationLink(value: meeting) {
                    meetingRow(meeting)
                }
            }
            .overlay {
                if viewModel.meetings.isEmpty {
                    ContentUnavailableView(
                        "Встреч пока нет",
                        systemImage: "calendar",
                        description: Text("Завершённые записи появятся здесь")
                    )
                }
            }
            .navigationTitle("Meetings")
            .navigationDestination(for: FfiMeetingSummary.self) { meeting in
                MeetingDetailView(meeting: meeting, viewModel: viewModel)
            }
            .onAppear {
                viewModel.reload()
            }
        }
    }

    private func meetingRow(_ meeting: FfiMeetingSummary) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(shortId(meeting.id))
                    .font(.headline.monospaced())
                Spacer()
                Text(meetingDate(meeting.startedAtMs))
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 8) {
                if meeting.hasFinal {
                    Label("Final", systemImage: "checkmark.seal.fill")
                        .foregroundStyle(.green)
                }
                if meeting.artifactCount > 0 {
                    Label(
                        "\(meeting.artifactCount)",
                        systemImage: "doc.text.fill"
                    )
                    .foregroundStyle(.secondary)
                }
            }
            .font(.caption)
        }
        .padding(.vertical, 4)
    }

    private func shortId(_ id: String) -> String {
        String(id.prefix(8))
    }

    private func meetingDate(_ timestampMs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
        return date.formatted(
            date: .abbreviated,
            time: .shortened
        )
    }
}
