@testable import MeetingRaft
import XCTest

/// Разделы настроек: список фиксирован ТЗ, и его легко случайно
/// сократить при правке.
final class SettingsSectionTests: XCTestCase {
    func testSevenSectionsFromSpec() {
        XCTAssertEqual(SettingsSection.allCases.count, 7)
    }

    func testEverySectionHasTitleAndIcon() {
        for section in SettingsSection.allCases {
            XCTAssertFalse(section.title.isEmpty, "\(section)")
            XCTAssertFalse(section.systemImage.isEmpty, "\(section)")
        }
    }

    /// Заголовки не должны ссылаться на внутренние документы: раньше в
    /// них стояли номера ADR, о которых пользователь не знает.
    func testTitlesDoNotLeakInternalReferences() {
        for section in SettingsSection.allCases {
            XCTAssertFalse(section.title.contains("ADR"), "\(section.title)")
        }
    }

    func testIdentifiersAreUnique() {
        let ids = SettingsSection.allCases.map(\.id)

        XCTAssertEqual(Set(ids).count, ids.count)
    }
}

@MainActor
final class SettingsModelTests: XCTestCase {
    func testEngineLabelReflectsMissingModel() {
        let model = SettingsModel()

        // Модель не загружена: путь пуст, движок обязан честно назваться
        // заглушкой, а не Whisper.
        XCTAssertFalse(model.isModelReady)
        XCTAssertTrue(model.liveEngineLabel.lowercased().contains("mock"))
    }

    func testInstalledLookupMatchesFilename() {
        let model = SettingsModel()

        XCTAssertFalse(model.isInstalled(.small))
        // `auto` не имеет файла: он установлен, если установлено хоть что-то.
        XCTAssertFalse(model.isInstalled(.auto))
    }

    /// Память на голоса выключена до `load`, то есть и на экране, который
    /// ещё не дотянулся до ядра.
    ///
    /// Это не мелочь вёрстки: тумблер, нарисованный включённым по
    /// умолчанию, сам по себе обещание, которого никто не давал. Значение
    /// приходит из ядра, а до него — «выключено», единственная сторона, в
    /// которую здесь можно ошибиться безопасно.
    func testVoiceMemoryIsOffBeforeTheCoreIsOpened() {
        let model = SettingsModel()

        XCTAssertFalse(model.voiceMemoryEnabled)
        XCTAssertTrue(model.knownVoices.isEmpty)
    }

    func testDownloadErrorMessages() {
        XCTAssertTrue(
            SettingsModel.message(for: WhisperModelDownloaderError.downloadFailed(statusCode: 404))
                .contains("404")
        )
        XCTAssertFalse(
            SettingsModel.message(for: WhisperModelDownloaderError.notDownloadable).isEmpty
        )
    }
}
