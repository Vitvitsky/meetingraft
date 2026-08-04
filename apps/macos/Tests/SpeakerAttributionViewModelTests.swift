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
            segments: [segment(0, channel: "system", speakerId: "", pinned: false)]
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
            segments: [segment(0, channel: "mic", speakerId: "", pinned: false)]
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
                segment(0, channel: "system", speakerId: "s1", pinned: false),
                segment(1, channel: "system", speakerId: "s1", pinned: false),
                segment(2, channel: "system", speakerId: "s2", pinned: true),
            ]
        )
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: 1)

        XCTAssertEqual(viewModel.channelSpeakerName("system"), "Пётр")
        XCTAssertTrue(viewModel.channelSpeakerName("mic").isEmpty)
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

    func testRemoveSurfacesCoreError() {
        let core = AttributionCoreSpy(speakers: [speaker("s1", "Пётр")])
        core.deleteError = "занят"
        let viewModel = SpeakerAttributionViewModel(core: core)
        viewModel.load(meetingId: "m1", version: nil)

        viewModel.remove(id: "s1")

        XCTAssertEqual(viewModel.errorMessage, "занят")
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
        channel: String,
        speakerId: String,
        pinned: Bool
    ) -> FfiFinalSegment {
        FfiFinalSegment(
            index: index,
            startMs: UInt64(index) * 1000,
            endMs: UInt64(index) * 1000 + 900,
            channel: channel,
            speakerId: speakerId,
            speakerName: "",
            speakerPinned: pinned,
            text: "текст"
        )
    }
}

private final class AttributionCoreSpy: SpeakerAttributionCoreProviding {
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

    var speakers: [FfiSpeaker]
    var segments: [FfiFinalSegment]
    var stats: [FfiSpeakerStat]
    var assignSegmentError = ""
    var deleteError = ""
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
}
