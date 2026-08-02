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
                Button("Start demo") { viewModel.start() }
                Button("Stop") { viewModel.stop() }
            }
            .padding(.horizontal)

            HStack {
                if capture.isRecording {
                    Label("Recording", systemImage: "record.circle.fill")
                        .foregroundStyle(.red)
                    Text("chunks: \(capture.chunkCount)")
                        .foregroundStyle(.secondary)
                    if !capture.systemAudioAvailable {
                        Text("mic only")
                            .foregroundStyle(.orange)
                    }
                    Button("Stop Recording") { capture.stopRecording() }
                } else {
                    Button("Start Recording") {
                        Task { await capture.startRecording() }
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
            viewModel.stop()
            capture.stopRecording()
        }
    }
}
