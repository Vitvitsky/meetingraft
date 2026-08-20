import AppKit
import Combine
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
    @State private var overlay = OverlayWindowController()
    @Environment(PresenceSettingsStore.self) private var presenceStore
    @Environment(AppearanceSettingsStore.self) private var appearanceStore
    @Environment(RecordingBridge.self) private var recordingBridge
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
        // Тема идёт из настроек; `nil` означает системную.
        .background(Theme.surfaceRoot)
        // Минимум окна: без него его можно сжать так, что управлению
        // внизу экрана просто некуда поместиться.
        .frame(minWidth: 880, minHeight: 560)
        .preferredColorScheme(appearanceStore.colorScheme)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Picker("Language", selection: Bindable(languageStore).primary) {
                    ForEach(languageStore.allowed) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                .frame(width: 140)
                // Демо-поток остался только в меню Session: витрине
                // продукта не место кнопке для разработчика, и она делила
                // ⌘R с настоящей записью.
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
            // Разрешение на системный звук спрашивается при открытии окна,
            // а не по нажатию «запись»: там запрос ложится в начало
            // встречи, и первые слова созвона теряются целиком. Заодно к
            // моменту записи tap уже разведан, и нажатие не ждёт
            // `coreaudiod`.
            captureCoordinator.warmUpSystemAudio()
            // Модель качается на старте, а не при заходе в Settings.
            await modelBootstrap.ensureModel(core: core)
        }
        // Накладка следует за состоянием записи и за настройками: их
        // можно поменять посреди сессии, и решение должно пересчитаться.
        .onChange(of: captureCoordinator.isRecording) { _, isRecording in
            recordingBridge.setRecording(isRecording)
            applyPresence()
        }
        .onAppear {
            // Строка меню живёт в отдельной сцене и не видит координатор;
            // окно отдаёт ей действия, а не состояние целиком.
            recordingBridge.toggle = { toggleRecording() }
            recordingBridge.openWindow = { overlay.restoreMainWindow() }
        }
        // Запись больше не привязана к экрану субтитров, поэтому выход из
        // приложения — последний момент, когда её можно закрыть штатно.
        // Без этого Final не собирается и сессия остаётся незакрытой в
        // базе: тишина вместо получасовой записи.
        .onReceive(
            NotificationCenter.default.publisher(
                for: NSApplication.willTerminateNotification
            )
        ) { _ in
            captionsViewModel.stopAll(capture: captureCoordinator)
        }
        .onChange(of: presenceStore.showsOverlay) { _, _ in
            applyPresence()
        }
        .onChange(of: presenceStore.minimizesMainWindow) { _, _ in
            applyPresence()
        }
        .onChange(of: appearanceStore.preference) { _, _ in
            // Накладка живёт вне иерархии SwiftUI: сама она о смене темы
            // не узнает, её пересобирает тот же путь, что и всё остальное
            // присутствие.
            if overlay.isVisible {
                applyPresence()
            }
        }
        .onChange(of: captionsViewModel.lines) { _, _ in
            // Содержимое накладки обновляется вместе с лентой.
            if overlay.isVisible {
                applyPresence()
            }
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

    /// Старт или остановка записи из строки меню.
    private func toggleRecording() {
        if captureCoordinator.isRecording {
            captionsViewModel.stopLive(capture: captureCoordinator)
        } else {
            startLiveSession()
        }
    }

    /// Привести окна в соответствие с состоянием записи.
    private func applyPresence() {
        let decision = PresenceDecision.make(
            isRecording: captureCoordinator.isRecording,
            settings: presenceStore
        )

        if decision.showsOverlay {
            overlay.show(
                content: CaptionOverlayView(
                    lines: captionsViewModel.recentLines(limit: 2),
                    isRecording: captureCoordinator.isRecording,
                    showsSpeaker: captureCoordinator.systemAudioAvailable,
                    opacity: presenceStore.overlayOpacity
                ) {
                    captionsViewModel.stopLive(capture: captureCoordinator)
                },
                preference: appearanceStore.preference
            )
        } else {
            overlay.hide()
        }

        if decision.hidesMainWindow {
            overlay.hideMainWindow()
        } else {
            overlay.restoreMainWindow()
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
