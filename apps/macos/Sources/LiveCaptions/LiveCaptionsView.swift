import SwiftUI

/// Экран live captions; перевод — отдельная лента (ADR-008 backends).
struct LiveCaptionsView: View {
    @Bindable var viewModel: LiveCaptionsViewModel
    @Bindable var capture: AudioCaptureCoordinator
    @Environment(TranslationSettingsStore.self) private var translationStore
    let primaryLanguage: SpeechLanguage

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Session language: \(primaryLanguage.displayName)")
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Start demo") { viewModel.startDemo(translation: translationStore) }
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
                        Task {
                            await viewModel.startLive(
                                capture: capture,
                                translation: translationStore
                            )
                        }
                    }
                }
            }
            .padding(.horizontal)

            HStack {
                Toggle("Live translation", isOn: Bindable(translationStore).enabled)
                if translationStore.enabled {
                    Picker("To", selection: Bindable(translationStore).target) {
                        ForEach(SpeechLanguage.allCases.filter { $0 != primaryLanguage }) { language in
                            Text(language.displayName).tag(language)
                        }
                    }
                    .frame(width: 120)
                    Picker("Engine", selection: Bindable(translationStore).backend) {
                        ForEach(translationStore.backends.filter { $0 != .off }) { kind in
                            Text(kind.displayName).tag(kind)
                        }
                    }
                    .frame(width: 150)
                    Text("effective: \(viewModel.effectiveTranslationBackend)")
                        .foregroundStyle(.tertiary)
                        .font(.caption)
                }
                Spacer()
            }
            .padding(.horizontal)
            .onChange(of: translationStore.enabled) { _, _ in
                viewModel.applyTranslationSettings(translationStore)
            }
            .onChange(of: translationStore.target) { _, _ in
                viewModel.applyTranslationSettings(translationStore)
            }
            .onChange(of: translationStore.backend) { _, _ in
                viewModel.applyTranslationSettings(translationStore)
            }

            if let error = capture.lastError {
                Text(error)
                    .foregroundStyle(.red)
                    .padding(.horizontal)
            }

            if translationStore.enabled {
                HSplitView {
                    captionList(title: "Captions", lines: viewModel.lines)
                    captionList(title: "Translation", lines: viewModel.translationLines)
                }
            } else {
                captionList(title: "Captions", lines: viewModel.lines)
            }
        }
        .navigationTitle("Live Captions")
        .onAppear {
            viewModel.applyTranslationSettings(translationStore)
        }
        .onDisappear {
            viewModel.stopAll(capture: capture)
        }
    }

    private func captionList(title: String, lines: [CaptionLine]) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.headline)
                .padding(.horizontal)
                .padding(.bottom, 4)
            List(lines) { line in
                Text(line.text)
                    .font(line.phase == .partial ? .body.italic() : .body)
                    .foregroundStyle(line.phase == .partial ? .secondary : .primary)
                    .accessibilityLabel("\(line.phase == .partial ? "Partial" : "Final"): \(line.text)")
            }
        }
    }
}
