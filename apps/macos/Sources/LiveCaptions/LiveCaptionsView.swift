import SwiftUI

/// Экран live captions; логика в ViewModel / coordinator.
struct LiveCaptionsView: View {
    @Bindable var viewModel: LiveCaptionsViewModel
    @Bindable var capture: AudioCaptureCoordinator
    let primaryLanguage: SpeechLanguage

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Session language: \(primaryLanguage.displayName)")
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Start demo") { viewModel.startDemo() }
                Button("Stop demo") { viewModel.stopDemo() }
            }
            .padding(.horizontal)

            HStack {
                if capture.isRecording || viewModel.isLiveSession {
                    Label("Live", systemImage: "record.circle.fill")
                        .foregroundStyle(.red)
                    Text("chunks: \(capture.chunkCount)")
                        .foregroundStyle(.secondary)
                    Text("captions: \(capture.captionEventCount)")
                        .foregroundStyle(.secondary)
                    Text("STT: \(capture.sttBackend)")
                        .foregroundStyle(.secondary)
                    if !capture.systemAudioAvailable {
                        Text("mic only")
                            .foregroundStyle(.orange)
                    }
                    Button("Stop Live") { viewModel.stopLive(capture: capture) }
                } else {
                    Button("Start Live") {
                        Task { await viewModel.startLive(capture: capture) }
                    }
                }
            }
            .padding(.horizontal)

            if let error = capture.lastError {
                Text(error)
                    .foregroundStyle(.red)
                    .padding(.horizontal)
            }

            List(viewModel.lines) { line in
                Text(line.text)
                    .font(line.phase == .partial ? .body.italic() : .body)
                    .foregroundStyle(line.phase == .partial ? .secondary : .primary)
                    .accessibilityLabel("\(line.phase == .partial ? "Partial" : "Final"): \(line.text)")
            }
        }
        .navigationTitle("Live Captions")
        .onDisappear {
            viewModel.stopAll(capture: capture)
        }
    }
}
