import SwiftUI

/// Экран live captions; логика в ViewModel.
struct LiveCaptionsView: View {
    @Bindable var viewModel: LiveCaptionsViewModel
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

            List(viewModel.lines) { line in
                Text(line.text)
                    .font(line.phase == .partial ? .body.italic() : .body)
                    .foregroundStyle(line.phase == .partial ? .secondary : .primary)
                    .accessibilityLabel("\(line.phase == .partial ? "Partial" : "Final"): \(line.text)")
            }
        }
        .navigationTitle("Live Captions")
        .onDisappear { viewModel.stop() }
    }
}
