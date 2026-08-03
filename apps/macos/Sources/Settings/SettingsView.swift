import SwiftUI

/// Окно настроек: список разделов слева, содержимое справа.
///
/// До этого всё лежало в одном `Form` длиной в экран с лишним, а
/// заголовки секций ссылались на номера ADR — внутренние документы, о
/// которых пользователь не знает. Разделы из ТЗ редизайна §3.2.
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @Environment(TranslationSettingsStore.self) private var translationStore
    @Environment(ProviderSettingsStore.self) private var providerStore

    @State private var selection: SettingsSection = .general
    @State private var model = SettingsModel()

    var body: some View {
        NavigationSplitView {
            List(SettingsSection.allCases, selection: $selection) { section in
                Label(section.title, systemImage: section.systemImage)
                    .tag(section)
            }
            // Содержимое сайдбара уходит под заголовок окна, и кнопки
            // закрытия оказываются вплотную к первому пункту. Отступ
            // сверху освобождает под них полосу.
            .safeAreaInset(edge: .top) {
                Color.clear.frame(height: Theme.Space.lg)
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 200, max: 240)
        } detail: {
            ScrollView {
                content
                    .padding(Theme.Space.lg)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(Theme.surfaceRoot)
            .navigationTitle(selection.title)
        }
        .frame(minWidth: 720, minHeight: 520)
        .preferredColorScheme(.dark)
        .onAppear {
            model.load(providerStore: providerStore)
        }
        .onChange(of: providerStore.selectedSttModelId) { _, _ in
            model.applySttPreference(providerStore)
        }
        .onChange(of: providerStore.apiBaseUrl) { _, _ in
            model.applyProviderConfig(providerStore)
        }
        .onChange(of: providerStore.apiToken) { _, _ in
            model.applyProviderConfig(providerStore)
        }
        .onChange(of: providerStore.llmEngine) { _, engine in
            model.applyProviderConfig(providerStore)
            if engine.needsBackendModelPicker {
                model.refreshBackendLlmModels(providerStore)
            }
        }
        .onChange(of: providerStore.llmModelId) { _, _ in
            model.applyProviderConfig(providerStore)
        }
        .onChange(of: providerStore.llmProviderId) { _, _ in
            model.applyProviderConfig(providerStore)
        }
        .onChange(of: providerStore.llmBaseUrl) { _, _ in
            model.applyProviderConfig(providerStore)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch selection {
        case .general:
            GeneralSettingsSection()
        case .audio:
            AudioSettingsSection()
        case .sttEngine:
            SttSettingsSection(model: model)
        case .translation:
            TranslationSettingsSection()
        case .llm:
            LlmSettingsSection(model: model)
        case .backend:
            BackendSettingsSection(model: model)
        case .data:
            DataSettingsSection(model: model)
        }
    }
}
