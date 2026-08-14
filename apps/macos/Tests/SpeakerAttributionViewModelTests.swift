@testable import MeetingRaft
import XCTest

@MainActor
final class SpeakerAttributionViewModelTests: XCTestCase {
    // MARK: - Строки экрана

    func testRowsCarryStatsAndUseCurrentSpeakerName() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            stats: [stat("s1", name: "устаревшее", channel: "system", count: 3, ms: 6000, share: 0.6)]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 2)

        XCTAssertEqual(viewModel.rows.count, 1)
        XCTAssertEqual(viewModel.rows[0].displayName, "Пётр", "имя берётся из актуальной записи")
        XCTAssertEqual(viewModel.rows[0].segmentCount, 3)
        XCTAssertEqual(viewModel.rows[0].speakingMs, 6000)
    }

    /// Участник, заведённый вручную, не должен пропасть до первой реплики.
    func testSpeakerWithoutSegmentsStillAppears() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр"), speaker("s2", "Гость")],
            stats: [stat("s1", name: "Пётр", channel: "system", count: 1, ms: 1000, share: 1)]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 2)

        XCTAssertEqual(viewModel.rows.map(\.id), ["s1", "s2"])
        XCTAssertEqual(viewModel.rows[1].segmentCount, 0)
        XCTAssertTrue(viewModel.rows[1].channelCode.isEmpty)
    }

    /// Удалённая запись оставляет реплики без имени, но со временем:
    /// прятать их значило бы врать про длительность встречи.
    func testStatWithoutSpeakerRecordKeepsTime() {
        let core = AttributionCoreSpy(
            speakers: [],
            stats: [stat("ghost", name: "", channel: "mic", count: 2, ms: 4000, share: 1)]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertEqual(viewModel.rows[0].speakingMs, 4000)
        XCTAssertEqual(viewModel.rows[0].label, "Без имени")
    }

    // MARK: - Версия

    /// Версии Final нет — сегментов и статистики просить не у кого.
    func testMissingVersionSkipsSegmentQueries() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: nil)

        XCTAssertEqual(core.listSegmentsCallCount, 0)
        XCTAssertFalse(viewModel.hasSegments)
        XCTAssertFalse(viewModel.canAssign)
        XCTAssertEqual(viewModel.rows.count, 1, "участники видны и без Final")
    }

    /// У версии, собранной до re-ASR, сегментов нет — назначать нечего.
    func testVersionWithoutSegmentsForbidsAssignment() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertFalse(viewModel.canAssign)
    }

    // MARK: - Назначение

    func testAssignChannelForwardsVersionAndReloads() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [segment(0, channel: "system", speakerId: "")]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 3)

        viewModel.assignChannel("system", to: "s1")

        XCTAssertEqual(core.channelAssignments.count, 1)
        XCTAssertEqual(core.channelAssignments[0].version, 3)
        XCTAssertEqual(core.channelAssignments[0].channelCode, "system")
        XCTAssertEqual(core.channelAssignments[0].speakerId, "s1")
        XCTAssertEqual(core.listSegmentsCallCount, 2, "после назначения список перечитан")
    }

    /// Без версии назначать некуда: молча уходить в ядро с версией 0
    /// означало бы править чужой транскрипт.
    func testAssignWithoutVersionDoesNotReachCore() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.assignChannel("mic", to: "s1")
        viewModel.assignSegment(index: 0, to: "s1")
        viewModel.unpinSegment(index: 0)

        XCTAssertTrue(core.channelAssignments.isEmpty)
        XCTAssertTrue(core.segmentAssignments.isEmpty)
        XCTAssertTrue(core.unpinnedIndexes.isEmpty)
    }

    func testAssignSegmentSurfacesCoreError() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [segment(0, channel: "mic", speakerId: "")]
        )
        core.assignSegmentError = "boom"
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.assignSegment(index: 0, to: "s1")

        XCTAssertEqual(viewModel.errorMessage, "boom")
        XCTAssertEqual(core.listSegmentsCallCount, 1, "неудача не перечитывает данные")
    }

    // MARK: - Подпись канала

    /// Подпись канала — это большинство непоправленных реплик: одна
    /// правка не должна переписывать заголовок всей дорожки.
    func testChannelSpeakerIgnoresPinnedSegments() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр"), speaker("s2", "Гость")],
            segments: [
                segment(0, channel: "system", speakerId: "s1", source: "channel"),
                segment(1, channel: "system", speakerId: "s1", source: "channel"),
                segment(2, channel: "system", speakerId: "s2", source: "human"),
            ]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertEqual(viewModel.channelSpeakerName("system"), "Пётр")
        XCTAssertTrue(viewModel.channelSpeakerName("mic").isEmpty)
    }

    // MARK: - Слепки голоса (ADR-013)

    /// Неопознанные — это **реплики без имени**, а не «голоса, которых не
    /// узнали»: голосов как отдельных сущностей в схеме со слепками нет.
    ///
    /// Заведомо положительный случай внутри: подписанные реплики в списке
    /// быть не должны, иначе проверка проходила бы и на списке «все
    /// реплики подряд».
    func testUnidentifiedRepliesAreTheOnesWithoutAName() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [
                segment(0, channel: "mic", speakerId: "s1", source: "human"),
                segment(1, channel: "mic", speakerId: ""),
                segment(2, channel: "system", speakerId: "s1", source: "voiceprint"),
                segment(3, channel: "system", speakerId: ""),
            ]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertEqual(viewModel.unidentifiedSegments.map(\.index), [1, 3])
    }

    /// Без единой ручной подписи складывать слепок не из чего, и пересчёт
    /// вернул бы ноль. Кнопка в этом случае не нажимается — «не работает
    /// молча» здесь означало бы «нажал и ничего не понял».
    func testRecomputeIsUnavailableUntilSomethingIsLabelledByHand() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [
                segment(0, channel: "mic", speakerId: "s1", source: "channel"),
                segment(1, channel: "mic", speakerId: ""),
            ]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertFalse(viewModel.canRecomputeVoicePrints, "подпись по каналу слепка не даёт")
        XCTAssertEqual(viewModel.humanLabelledCount, 0)

        core.segments[0] = segment(0, channel: "mic", speakerId: "s1", source: "human")
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertTrue(viewModel.canRecomputeVoicePrints)
        XCTAssertEqual(viewModel.humanLabelledCount, 1)
    }

    func testRecomputeUsesTheDefaultThresholdsAndKeepsTheResult() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [segment(0, channel: "mic", speakerId: "s1", source: "human")]
        )
        core.recomputeResult = FfiVoicePrintPass(
            error: "",
            prints: 1,
            signed: 7,
            cleared: 0,
            unknown: 3,
            withoutVector: 2,
            signedFromMemory: 0,
            modelId: "cam++"
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.recomputeVoicePrints()

        XCTAssertEqual(
            core.recomputeCalls,
            [AttributionCoreSpy.Thresholds(accept: 0.45, margin: 0.05)]
        )
        XCTAssertEqual(viewModel.lastPass?.signed, 7)
        XCTAssertNil(viewModel.errorMessage)
    }

    /// Отказ обязан быть виден **и** остаться числом отчёта: без первого
    /// он потерялся бы, без второго исчез бы при следующем действии.
    ///
    /// Худший исход здесь — «готово, 0 подписано»: человек прочтёт это как
    /// «голоса не разошлись» и поверит.
    func testRefusalToRecomputeIsShownNotSwallowed() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [segment(0, channel: "mic", speakerId: "s1", source: "human")]
        )
        core.recomputeResult = FfiVoicePrintPass(
            error: "разделение голосов не собрано",
            prints: 0,
            signed: 0,
            cleared: 0,
            unknown: 0,
            withoutVector: 0,
            signedFromMemory: 0,
            modelId: ""
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.recomputeVoicePrints()

        XCTAssertEqual(viewModel.errorMessage, "разделение голосов не собрано")
        XCTAssertEqual(viewModel.lastPass?.error, "разделение голосов не собрано")
    }

    /// Смена модели — не поломка: слепки не выбрасываются и не
    /// используются, а человеку говорится пересчитать.
    func testAPrintFromAnotherModelAsksToBeRecomputed() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.voicePrints = [voicePrint("s1", modelMatches: true)]
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertFalse(viewModel.voicePrintsNeedRecompute)

        core.voicePrints = [voicePrint("s1", modelMatches: false)]
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertTrue(viewModel.voicePrintsNeedRecompute)
    }

    /// «Померили и не узнали» и «мерить было нечего» — разные ответы, и в
    /// отчёте они разные числа. Сложенные в одно, они выдали бы нехватку
    /// материала за несходство голосов.
    func testPassSummaryKeepsUnknownApartFromWithoutAudio() {
        let summary = SpeakerFormat.passSummary(
            FfiVoicePrintPass(
                error: "",
                prints: 2,
                signed: 10,
                cleared: 1,
                unknown: 4,
                withoutVector: 3,
                signedFromMemory: 0,
                modelId: "cam++"
            )
        )

        XCTAssertTrue(summary.contains("без имени 4"), summary)
        XCTAssertTrue(summary.contains("без звука 3"), summary)
        XCTAssertTrue(summary.contains("снято 1"), summary)
    }

    /// Ноль реплик без звука не показывается вовсе: строка отчёта должна
    /// называть случившееся, а не перечислять все возможные исходы.
    func testPassSummaryStaysSilentAboutZeroes() {
        let summary = SpeakerFormat.passSummary(
            FfiVoicePrintPass(
                error: "",
                prints: 1,
                signed: 5,
                cleared: 0,
                unknown: 2,
                withoutVector: 0,
                signedFromMemory: 0,
                modelId: "cam++"
            )
        )

        XCTAssertFalse(summary.contains("без звука"), summary)
        XCTAssertFalse(summary.contains("снято"), summary)
    }

    /// Без собранного движка вся ветка слепков не показывается.
    ///
    /// Это не «модель не скачана» — то человек исправит. Фичи, которой нет
    /// в бинаре, он не добавит ничем, и кнопка, отказывающая всегда, — та
    /// самая заглушка в интерфейсе, которой в этом продукте быть не должно.
    ///
    /// Заведомо положительный случай рядом: с движком та же встреча
    /// пересчитывается. Без него проверка проходила бы и у модели, которая
    /// запрещает всё подряд.
    func testWithoutTheEngineTheWholeVoiceprintBranchIsHidden() {
        let core = AttributionCoreSpy(
            speakers: [speaker("s1", "Пётр")],
            segments: [segment(0, channel: "mic", speakerId: "s1", source: "human")]
        )
        core.voiceMemoryEnabled = true
        core.voicePrints = [voicePrint("s1", modelMatches: true)]
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 1)
        XCTAssertTrue(viewModel.voiceEngineAvailable)
        XCTAssertTrue(viewModel.canRecomputeVoicePrints, "с движком пересчёт доступен")
        XCTAssertTrue(viewModel.canRememberVoice(speakerId: "s1"))

        core.voiceEngineAvailable = false
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertFalse(viewModel.voiceEngineAvailable)
        XCTAssertFalse(viewModel.canRecomputeVoicePrints)
        XCTAssertFalse(viewModel.canRememberVoice(speakerId: "s1"))
    }

    /// Два счётчика в строке участника означают разное, и подписи у них
    /// обязаны различаться.
    ///
    /// «N репл.» — сколько реплик участнику досталось, растёт после
    /// пересчёта. Слепок — из скольки **ваших подписей** он сложен, и не
    /// растёт вовсе. Пока оба назывались «репл.», совпадение второго с
    /// числом своих подписей читалось как «пересчёт ничего не сделал».
    func testThePrintChipDoesNotSpeakOfRepliesLikeTheCountBesideIt() {
        let chip = SpeakerFormat.voicePrintText(voicePrint("s1", modelMatches: true))
        let count = SpeakerFormat.segmentCountText(120)

        XCTAssertTrue(chip.contains("подписей"), chip)
        XCTAssertFalse(chip.contains("репл."), "две разные величины названы одним словом: \(chip)")
        XCTAssertTrue(count.contains("репл."), count)
    }

    // MARK: - Память на голоса (задача 7)

    /// Три условия сразу, и каждое отказывает по своей причине. Кнопка
    /// без любого из них предлагала бы действие, которое откажет.
    ///
    /// Заведомо положительный случай — последним: при всех трёх
    /// выполненных условиях запоминать можно. Без него проверка
    /// «нельзя» проходила бы и у функции, всегда возвращающей `false`.
    func testRememberingNeedsMemoryOnAPrintAndAName() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.voicePrints = [voicePrint("s1", modelMatches: true)]
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 1)
        XCTAssertFalse(viewModel.canRememberVoice(speakerId: "s1"), "память выключена")

        core.voiceMemoryEnabled = true
        core.voicePrints = []
        viewModel.load(meetingId: "m1", version: 1)
        XCTAssertFalse(viewModel.canRememberVoice(speakerId: "s1"), "слепка нет")

        core.voicePrints = [voicePrint("s1", modelMatches: true)]
        core.speakers = [speaker("s1", "  ")]
        viewModel.load(meetingId: "m1", version: 1)
        XCTAssertFalse(viewModel.canRememberVoice(speakerId: "s1"), "имени нет")

        core.speakers = [speaker("s1", "Пётр")]
        viewModel.load(meetingId: "m1", version: 1)
        XCTAssertTrue(viewModel.canRememberVoice(speakerId: "s1"))
    }

    func testRememberingPassesTheSpeakerToTheCore() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.voiceMemoryEnabled = true
        core.voicePrints = [voicePrint("s1", modelMatches: true)]
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.rememberVoice(speakerId: "s1")

        XCTAssertEqual(core.rememberedSpeakerIds, ["s1"])
        XCTAssertNil(viewModel.errorMessage)
    }

    /// Отказ ядра виден. Молча проглоченный, он оставил бы человека в
    /// уверенности, что голос запомнен, — и это худший исход для функции,
    /// которая как раз о доверии.
    func testRefusalToRememberIsShown() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.voiceMemoryEnabled = true
        core.voicePrints = [voicePrint("s1", modelMatches: true)]
        core.rememberError = "память на голоса выключена"
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.rememberVoice(speakerId: "s1")

        XCTAssertEqual(viewModel.errorMessage, "память на голоса выключена")
        XCTAssertTrue(core.rememberedSpeakerIds.isEmpty)
    }

    /// Узнанное по памяти считается отдельно от подписанного слепками
    /// этой встречи: человек включил биометрию осознанно и вправе видеть,
    /// сколько она сделала.
    func testPassSummaryCountsMemoryApartFromThisMeeting() {
        let summary = SpeakerFormat.passSummary(
            FfiVoicePrintPass(
                error: "",
                prints: 2,
                signed: 10,
                cleared: 0,
                unknown: 4,
                withoutVector: 0,
                signedFromMemory: 3,
                modelId: "cam++"
            )
        )

        XCTAssertTrue(summary.contains("подписано 10"), summary)
        XCTAssertTrue(summary.contains("узнано по памяти 3"), summary)
    }

    // MARK: - Спикеры

    func testAddSpeakerUsesLanguageOfInterface() {
        let core = AttributionCoreSpy()
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.addSpeaker(primaryLanguage: "ru")
        XCTAssertEqual(core.lastUpsertDisplayName, "Спикер 1")

        viewModel.addSpeaker(primaryLanguage: "en")
        XCTAssertEqual(core.lastUpsertDisplayName, "Speaker 2")
    }

    /// Пустое имя — это промах по Enter, а не переименование.
    func testRenameIgnoresBlankAndUnchangedNames() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.rename(id: "s1", displayName: "   ")
        viewModel.rename(id: "s1", displayName: "Пётр")

        XCTAssertNil(core.lastUpsertDisplayName)
    }

    func testRenameTrimsWhitespace() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.rename(id: "s1", displayName: "  Пётр Иванов ")

        XCTAssertEqual(core.lastUpsertDisplayName, "Пётр Иванов")
    }

    /// Набранное имя сохраняется не только по Enter: экран зовёт то же
    /// правило по уходу фокуса и по исчезновению строки.
    ///
    /// До этого сохранял один Enter, и имя, набранное и оставленное
    /// переключением вкладки, пропадало молча — возврат показывал
    /// прежнее «Спикер 3».
    func testDraftNameIsCommittedWhenItDiffers() {
        let row = SpeakerRowModel(
            id: "s1",
            displayName: "Спикер 3",
            channelCode: "mic",
            segmentCount: 2,
            speakingMs: 4000,
            share: 0.5
        )

        XCTAssertEqual(row.nameToCommit(draft: "  Пётр Иванов "), "Пётр Иванов")
    }

    /// Пустое поле сохранять нечего: участник остался бы без подписи, а
    /// это неотличимо от сбоя атрибуции.
    func testBlankAndUnchangedDraftsAreNotCommitted() {
        let row = SpeakerRowModel(
            id: "s1",
            displayName: "Пётр",
            channelCode: "mic",
            segmentCount: 1,
            speakingMs: 1000,
            share: 1
        )

        XCTAssertNil(row.nameToCommit(draft: "   "))
        XCTAssertNil(row.nameToCommit(draft: ""))
        XCTAssertNil(row.nameToCommit(draft: "Пётр"), "то же имя — лишняя пересборка markdown")
        XCTAssertNil(row.nameToCommit(draft: "  Пётр  "), "разница только в пробелах")
    }

    /// Кнопка прослушивания показывается по признаку встречи, а не
    /// реплики: чтение фрагмента — поход на диск, а список
    /// перерисовывается на каждое нажатие клавиши.
    func testAudioAvailabilityFollowsTheMeeting() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.audioBytes = 4096
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: nil)

        XCTAssertTrue(viewModel.audioAvailable)
    }

    /// Запись удалена — прослушивать нечего, и кнопки быть не должно.
    func testDeletedAudioLeavesNothingToPlay() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.audioBytes = 0
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: nil)

        XCTAssertFalse(viewModel.audioAvailable, "у встречи без записи кнопка была бы заглушкой")
    }

    /// Отсутствие звука за конкретный кусок называется вслух: кнопка была
    /// показана, человек нажал, и молчание он прочтёт как поломку.
    func testMissingFragmentIsReportedOutLoud() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.reportMissingFragment()

        XCTAssertNotNil(viewModel.errorMessage)
    }

    func testRemoveSurfacesCoreError() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.deleteError = "занят"
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.remove(id: "s1")

        XCTAssertEqual(viewModel.errorMessage, "занят")
    }

    // MARK: - Правка текста

    /// Esc не трогает ядро: иначе отказ от правки её бы и сохранял.
    func testCancelEditDoesNotCallCore() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [segment(
                0,
                text: "упирается в UniFFI",
                originalText: "упирается в юни-эф-эф-ай"
            )]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.beginEdit(index: 0)
        viewModel.draftText = "совсем другое"
        viewModel.cancelEdit()

        XCTAssertTrue(core.editedTexts.isEmpty)
        XCTAssertNil(viewModel.editingIndex)
    }

    /// Сохранение отдаёт ядру ровно введённое.
    func testCommitEditSendsDraftToCore() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [segment(0, text: "упирается в юни-эф-эф-ай")]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.beginEdit(index: 0)
        viewModel.draftText = "упирается в UniFFI"
        viewModel.commitEdit()

        XCTAssertEqual(core.editedTexts, ["упирается в UniFFI"])
        XCTAssertNil(viewModel.editingIndex)
    }

    /// Возврат к исходному отправляет распознанное — ядро само удалит
    /// правку из журнала. Тексты в данных заведомо разные: совпади они,
    /// тест прошёл бы, ничего не проверив.
    func testRevertSendsRecognizedText() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [segment(
                0,
                text: "упирается в UniFFI",
                originalText: "упирается в юни-эф-эф-ай"
            )]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.revertToOriginal(index: 0)

        XCTAssertEqual(core.editedTexts, ["упирается в юни-эф-эф-ай"])
    }

    /// Неправленый сегмент возвращать не к чему — ядро не дёргаем.
    func testRevertOnUntouchedSegmentDoesNothing() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [segment(0, text: "обычная реплика")]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.revertToOriginal(index: 0)

        XCTAssertTrue(core.editedTexts.isEmpty)
    }

    /// Ошибка ядра видна, а не проглочена.
    func testCommitEditSurfacesCoreError() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [segment(0, text: "реплика")]
        )
        core.editError = "сегмент 0 не найден"
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.beginEdit(index: 0)
        viewModel.draftText = "другое"
        viewModel.commitEdit()

        XCTAssertEqual(viewModel.errorMessage, "сегмент 0 не найден")
    }

    /// «Заменять всюду» предлагается ровно когда ядро дало id подсказки.
    func testCanPromoteFollowsCoreDecision() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [
                segment(
                    0,
                    text: "упирается в UniFFI",
                    originalText: "упирается в юни-эф-эф-ай",
                    promotableTermId: "t1"
                ),
                segment(
                    1,
                    text: "правленое, но термина нет",
                    originalText: "распознанное длиннее трёх слов совсем"
                ),
            ]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertTrue(viewModel.canPromote(index: 0))
        XCTAssertFalse(viewModel.canPromote(index: 1), "пустой id — кнопки быть не должно")
    }

    /// Повышение уходит в ядро с тем id, что оно само и дало.
    func testPromoteTermSendsCoreProvidedId() {
        let core = AttributionCoreSpy(
            speakers: [],
            segments: [segment(
                0,
                text: "упирается в UniFFI",
                originalText: "упирается в юни-эф-эф-ай",
                promotableTermId: "t1"
            )]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.promoteTerm(index: 0)

        XCTAssertEqual(core.promotedTermIds, ["t1"])
    }

    /// Неприменившиеся правки читаются и без версии Final.
    ///
    /// Версии в данных нет намеренно: правка, не легшая ни на одну
    /// версию, — как раз тот случай, ради которого раздел и заведён.
    /// Читай их модель под `guard let version`, человек бы их не увидел
    /// ровно тогда, когда они есть.
    func testUnappliedEditsAreReadWithoutVersion() {
        let core = AttributionCoreSpy(speakers: [])
        core.unapplied = [edit("e1")]
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: nil)

        XCTAssertEqual(viewModel.unappliedEdits.map(\.id), ["e1"])
    }

    /// Снятие правки уходит в ядро именно тем id, по которому нажали.
    ///
    /// Правок в данных две: с одной тест не отличил бы «передал нужный
    /// id» от «передал единственный».
    func testDismissUnappliedCallsCoreWithThatId() {
        let core = AttributionCoreSpy(speakers: [])
        core.unapplied = [edit("e1"), edit("e2")]
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        viewModel.dismissUnapplied(id: "e2")

        XCTAssertEqual(core.deletedEditIds, ["e2"])
    }

    /// Пустой список — пустая плашка, а заглушек в интерфейсе быть не должно.
    func testNoUnappliedEditsMeansNothingToShow() {
        let core = AttributionCoreSpy(speakers: [])
        core.unapplied = []
        let viewModel = SpeakerAttributionViewModel(core: core)

        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertTrue(viewModel.unappliedEdits.isEmpty)
    }

    // MARK: - Формат

    func testShareTextNeverShowsSpeechAsZero() {
        XCTAssertEqual(SpeakerFormat.shareText(0), "0%")
        XCTAssertEqual(SpeakerFormat.shareText(0.001), "1%", "одна фраза — всё-таки речь")
        XCTAssertEqual(SpeakerFormat.shareText(0.426), "43%")
        XCTAssertEqual(SpeakerFormat.shareText(1), "100%")
    }

    func testDurationAndTimecodeFormats() {
        XCTAssertEqual(SpeakerFormat.durationText(ms: 65000), "1:05")
        XCTAssertEqual(SpeakerFormat.durationText(ms: 3_725_000), "1:02:05")
        XCTAssertEqual(SpeakerFormat.timecode(ms: 65000), "01:05")
        XCTAssertEqual(SpeakerFormat.timecode(ms: 3_725_000), "1:02:05")
    }

    func testChannelLabelAndInitial() {
        XCTAssertEqual(SpeakerFormat.channelLabel("mic"), "Mic")
        XCTAssertEqual(SpeakerFormat.channelLabel("system"), "System")
        XCTAssertTrue(SpeakerFormat.channelLabel("").isEmpty)
        XCTAssertEqual(SpeakerFormat.initial("пётр"), "П")
        XCTAssertEqual(SpeakerFormat.initial("  "), "?")
    }

    // MARK: - Фабрики

    private func speaker(_ id: String, _ name: String) -> FfiSpeaker {
        FfiSpeaker(id: id, meetingId: "m1", displayName: name, sortIndex: 0)
    }

    private func voicePrint(_ speakerId: String, modelMatches: Bool) -> FfiVoicePrint {
        FfiVoicePrint(
            speakerId: speakerId,
            speakerName: "Пётр",
            modelId: modelMatches ? "cam++" : "english",
            samples: 3,
            seconds: 12.5,
            modelMatches: modelMatches
        )
    }

    private func stat(
        _ id: String,
        name: String,
        channel: String,
        count: UInt32,
        ms: UInt64,
        share: Double
    ) -> FfiSpeakerStat {
        FfiSpeakerStat(
            speakerId: id,
            displayName: name,
            channel: channel,
            segmentCount: count,
            speakingMs: ms,
            share: share
        )
    }

    private func segment(
        _ index: UInt32,
        channel: String = "mic",
        speakerId: String = "",
        source: String = "",
        text: String = "текст",
        originalText: String = "",
        promotableTermId: String = ""
    ) -> FfiFinalSegment {
        FfiFinalSegment(
            index: index,
            startMs: UInt64(index) * 1000,
            endMs: UInt64(index) * 1000 + 900,
            channel: channel,
            speakerId: speakerId,
            speakerName: "",
            speakerSource: source,
            text: text,
            textEdited: !originalText.isEmpty,
            originalText: originalText,
            promotableTermId: promotableTermId
        )
    }

    private func edit(_ id: String) -> FfiSegmentEdit {
        FfiSegmentEdit(
            id: id,
            channel: "mic",
            startMs: 0,
            endMs: 900,
            originalText: "юни-эф-эф-ай",
            editedText: "UniFFI"
        )
    }
}

private final class AttributionCoreSpy: SpeakerAttributionCoreProviding {
    /// Размер записи встречи; ноль — записи нет (удалена, Epic 22).
    var audioBytes: UInt64 = 1024

    struct ChannelAssignment: Equatable {
        let version: UInt32
        let channelCode: String
        let speakerId: String
    }

    struct SegmentAssignment: Equatable {
        let version: UInt32
        let index: UInt32
        let speakerId: String
    }

    struct Thresholds: Equatable {
        let accept: Float
        let margin: Float
    }

    var speakers: [FfiSpeaker]
    var segments: [FfiFinalSegment]
    var stats: [FfiSpeakerStat]
    var voicePrints: [FfiVoicePrint] = []
    var recomputeResult = FfiVoicePrintPass(
        error: "",
        prints: 0,
        signed: 0,
        cleared: 0,
        unknown: 0,
        withoutVector: 0,
        signedFromMemory: 0,
        modelId: ""
    )
    private(set) var recomputeCalls: [Thresholds] = []
    var voiceEngineAvailable = true
    var voiceMemoryEnabled = false
    var rememberError = ""
    private(set) var rememberedSpeakerIds: [String] = []
    var assignSegmentError = ""
    var deleteError = ""
    var unapplied: [FfiSegmentEdit] = []
    var editError = ""
    private(set) var editedTexts: [String] = []
    private(set) var deletedEditIds: [String] = []
    private(set) var promotedTermIds: [String] = []
    private(set) var listSegmentsCallCount = 0
    private(set) var channelAssignments: [ChannelAssignment] = []
    private(set) var segmentAssignments: [SegmentAssignment] = []
    private(set) var unpinnedIndexes: [UInt32] = []
    private(set) var lastUpsertDisplayName: String?

    init(
        speakers: [FfiSpeaker] = [],
        segments: [FfiFinalSegment] = [],
        stats: [FfiSpeakerStat] = []
    ) {
        self.speakers = speakers
        self.segments = segments
        self.stats = stats
    }

    func listSpeakers(meetingId _: String) -> [FfiSpeaker] {
        speakers
    }

    func upsertSpeaker(
        meetingId: String,
        id: String,
        displayName: String,
        sortIndex: Int64
    ) -> String {
        lastUpsertDisplayName = displayName
        let savedId = id.isEmpty ? "speaker-\(speakers.count + 1)" : id
        let saved = FfiSpeaker(
            id: savedId,
            meetingId: meetingId,
            displayName: displayName,
            sortIndex: sortIndex
        )
        if let index = speakers.firstIndex(where: { $0.id == savedId }) {
            speakers[index] = saved
        } else {
            speakers.append(saved)
        }
        return ""
    }

    func deleteSpeaker(meetingId _: String, id _: String) -> String {
        deleteError
    }

    func listFinalSegments(meetingId _: String, version _: UInt32) -> [FfiFinalSegment] {
        listSegmentsCallCount += 1
        return segments
    }

    func listSpeakerStats(meetingId _: String, version _: UInt32) -> [FfiSpeakerStat] {
        stats
    }

    func assignChannelSpeaker(
        meetingId _: String,
        version: UInt32,
        channelCode: String,
        speakerId: String
    ) -> String {
        channelAssignments.append(
            ChannelAssignment(version: version, channelCode: channelCode, speakerId: speakerId)
        )
        return ""
    }

    func assignSegmentSpeaker(
        meetingId _: String,
        version: UInt32,
        index: UInt32,
        speakerId: String
    ) -> String {
        guard assignSegmentError.isEmpty else { return assignSegmentError }
        segmentAssignments.append(
            SegmentAssignment(version: version, index: index, speakerId: speakerId)
        )
        return ""
    }

    func unpinSegmentSpeaker(meetingId _: String, version _: UInt32, index: UInt32) -> String {
        unpinnedIndexes.append(index)
        return ""
    }

    // MARK: - Правка (Epic 19)

    func editSegmentText(
        meetingId _: String,
        version _: UInt32,
        index _: UInt32,
        text: String
    ) -> String {
        editedTexts.append(text)
        return editError
    }

    func listUnappliedEdits(meetingId _: String) -> [FfiSegmentEdit] {
        unapplied
    }

    func deleteSegmentEdit(editId: String) -> String {
        deletedEditIds.append(editId)
        return ""
    }

    func promoteTermToReplacement(termId: String, meetingId _: String, version _: UInt32) -> String {
        promotedTermIds.append(termId)
        return ""
    }

    func segmentAudio(
        meetingId _: String,
        channelCode _: String,
        startMs _: UInt64,
        endMs _: UInt64
    ) -> FfiAudioFragment {
        FfiAudioFragment(pcm: Data(), sampleRate: 0, durationMs: 0)
    }

    func meetingAudioBytes(meetingId _: String) -> UInt64 {
        audioBytes
    }

    // MARK: - Слепки голоса (ADR-013)

    func listVoiceprints(meetingId _: String) -> [FfiVoicePrint] {
        voicePrints
    }

    func recomputeVoiceprints(
        meetingId _: String,
        version _: UInt32,
        accept: Float,
        margin: Float
    ) -> FfiVoicePrintPass {
        recomputeCalls.append(Thresholds(accept: accept, margin: margin))
        return recomputeResult
    }

    func voiceprintDefaultAccept() -> Float {
        0.45
    }

    func voiceprintDefaultMargin() -> Float {
        0.05
    }

    func isVoiceEngineAvailable() -> Bool {
        voiceEngineAvailable
    }

    func isVoiceMemoryEnabled() -> Bool {
        voiceMemoryEnabled
    }

    func rememberVoice(meetingId _: String, speakerId: String) -> String {
        guard rememberError.isEmpty else { return rememberError }
        rememberedSpeakerIds.append(speakerId)
        return ""
    }
}
