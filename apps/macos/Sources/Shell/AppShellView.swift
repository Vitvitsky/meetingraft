import Foundation
import SwiftUI

/// Корневой shell: sidebar + detail + toolbar.
struct AppShellView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(ProviderSettingsStore.self) private var providerStore
    @State private var selection: AppDestination? = .meetings
    @State private var captionsViewModel: LiveCaptionsViewModel
    @State private var captureCoordinator: AudioCaptureCoordinator
    @State private var glossaryViewModel: GlossaryViewModel
    @State private var meetingsViewModel: MeetingsViewModel
    @State private var modelBootstrap = FirstRunModelBootstrap()
    private let core: MeetingCore

    init() {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
        let root = support.appendingPathComponent("meetingraft", isDirectory: true)
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let core = MeetingCore.withDataRoot(dataRoot: root.path)
        self.core = core
        _captureCoordinator = State(initialValue: AudioCaptureCoordinator(core: core))
        _captionsViewModel = State(initialValue: LiveCaptionsViewModel(core: core))
        _glossaryViewModel = State(initialValue: GlossaryViewModel(core: core))
        _meetingsViewModel = State(initialValue: MeetingsViewModel(core: core))
    }

    var body: some View {
        NavigationSplitView {
            SidebarView(selection: $selection)
        } detail: {
            switch selection ?? .meetings {
            case .liveCaptions:
                LiveCaptionsView(
                    viewModel: captionsViewModel,
                    capture: captureCoordinator,
                    primaryLanguage: languageStore.primary
                )
            case .meetings:
                MeetingsListView(viewModel: meetingsViewModel, core: core)
            case .glossary:
                GlossaryView(
                    viewModel: glossaryViewModel,
                    liveSessionId: captureCoordinator.isRecording ? captureCoordinator.sessionId : nil
                )
            }
        }
        // Оболочка окна переведена на токены (ТЗ редизайна, D1, шаг 3).
        // Тема принудительно тёмная: светлая палитра вынесена за скобки,
        // и смешение с системной светлой дало бы нечитаемый контраст.
        .background(Theme.surfaceRoot)
        .preferredColorScheme(.dark)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Picker("Language", selection: Bindable(languageStore).primary) {
                    ForEach(languageStore.allowed) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                .frame(width: 140)
                Button("Start Captions", systemImage: "play.fill") {
                    startDemoCaptions()
                }
                .keyboardShortcut("r", modifiers: [.command])
                Button("Start Live", systemImage: "mic.fill") {
                    startLiveSession()
                }
                .keyboardShortcut("r", modifiers: [.command, .shift])
            }
        }
        .focusedValue(\.startCaptions) {
            startDemoCaptions()
        }
        .onAppear {
            captionsViewModel.applySessionLanguage(languageStore.primary)
            captionsViewModel.applyTranslationSettings(translationStore)
        }
        .task {
            // Модель качается на старте, а не при заходе в Settings.
            await modelBootstrap.ensureModel(core: core)
        }
        .onChange(of: languageStore.primary) { _, newValue in
            captionsViewModel.applySessionLanguage(newValue)
            if translationStore.target == newValue {
                translationStore.target =
                    SpeechLanguage.allCases.first { $0 != newValue } ?? .en
            }
            captionsViewModel.applyTranslationSettings(translationStore)
        }
        .alert(
            "Нет доступа к микрофону",
            isPresented: Binding(
                get: { captureCoordinator.lastError?.contains("микрофон") == true && !captureCoordinator.isRecording },
                set: {
                    if !$0 {
                        captureCoordinator.clearError()
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(captureCoordinator.lastError ?? "")
        }
    }

    private func startDemoCaptions() {
        selection = .liveCaptions
        captionsViewModel.applySessionLanguage(languageStore.primary)
        captionsViewModel.startDemo(translation: translationStore)
    }

    private func startLiveSession() {
        selection = .liveCaptions
        captionsViewModel.applySessionLanguage(languageStore.primary)
        Task {
            await captionsViewModel.startLive(
                capture: captureCoordinator,
                translation: translationStore,
                stt: providerStore
            )
        }
    }
}
