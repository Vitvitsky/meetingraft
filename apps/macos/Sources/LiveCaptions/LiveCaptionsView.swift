import SwiftUI

/// Экран live captions; перевод — отдельная лента (ADR-008 backends).
struct LiveCaptionsView: View {
    @Bindable var viewModel: LiveCaptionsViewModel
    @Bindable var capture: AudioCaptureCoordinator
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(ProviderSettingsStore.self) private var providerStore
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
                        SystemAudioUnavailableBadge(status: capture.systemAudioStatus)
                    }
                    Button("Stop Live") { viewModel.stopLive(capture: capture) }
                } else {
                    Button("Start Live") {
                        Task {
                            await viewModel.startLive(
                                capture: capture,
                                translation: translationStore,
                                stt: providerStore
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
                captionRow(line)
            }
        }
    }

    /// Подпись говорящего показывается только когда системный канал
    /// действительно пишется: в монологе она бессмысленна.
    @ViewBuilder
    private func captionRow(_ line: CaptionLine) -> some View {
        let phaseLabel = line.phase == .partial ? "Partial" : "Final"
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            if capture.systemAudioAvailable {
                Text(line.speaker.label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(line.speaker == .you ? Color.accentColor : Color.secondary)
                    .frame(width: 56, alignment: .leading)
            }
            Text(line.text)
                .font(line.phase == .partial ? .body.italic() : .body)
                .foregroundStyle(line.phase == .partial ? .secondary : .primary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            capture.systemAudioAvailable
                ? "\(line.speaker.label), \(phaseLabel): \(line.text)"
                : "\(phaseLabel): \(line.text)"
        )
    }
}
