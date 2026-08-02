import SwiftUI

/// Корневой shell: sidebar + detail + toolbar.
struct AppShellView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @State private var selection: AppDestination? = .liveCaptions
    @State private var captionsViewModel = LiveCaptionsViewModel()

    var body: some View {
        NavigationSplitView {
            SidebarView(selection: $selection)
        } detail: {
            switch selection ?? .liveCaptions {
            case .liveCaptions:
                LiveCaptionsView(
                    viewModel: captionsViewModel,
                    primaryLanguage: languageStore.primary
                )
            case .meetings:
                ContentUnavailableView(
                    "Meetings",
                    systemImage: "calendar",
                    description: Text("Появится в следующих фазах")
                )
            }
        }
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
            }
        }
        .focusedValue(\.startCaptions) {
            startDemoCaptions()
        }
    }

    private func startDemoCaptions() {
        selection = .liveCaptions
        captionsViewModel.start()
    }
}
