@testable import MeetingRaft
import XCTest

final class CaptureReadinessTests: XCTestCase {
    /// Когда всё выдано — показывать нечего.
    ///
    /// Заведомо отрицательный случай ко всем остальным: без него любое
    /// утверждение «плашка появилась» выполнялось бы и функцией, которая
    /// жалуется всегда.
    func testGrantedPermissionsSayNothing() {
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .granted, systemAudio: .granted),
            .ready
        )
    }

    /// Пока разведка не проходила, про системный звук сказать нечего:
    /// разрешение может и быть. Молчание честнее догадки.
    func testUnknownSystemAudioIsNotAComplaint() {
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .granted, systemAudio: .unknown),
            .ready
        )
    }

    /// Неспрошенное разрешение видно до нажатия записи — ровно то, чего
    /// раньше не было нигде.
    func testNotAskedMicrophoneIsVisibleBeforeRecording() {
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .notAsked, systemAudio: .unknown),
            .microphoneWillBeAsked
        )
    }

    /// Запрет микрофона важнее всего: без него записи нет вовсе.
    func testDeniedMicrophoneOutranksSystemAudio() {
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .denied, systemAudio: .denied),
            .microphoneDenied
        )
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .denied, systemAudio: .granted),
            .microphoneDenied
        )
    }

    /// Отказ системного звука — про качество записи, а не про её
    /// возможность, и виден он, когда микрофон уже разрешён.
    func testDeniedSystemAudioIsShownOnceTheMicrophoneIsSettled() {
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .granted, systemAudio: .denied),
            .systemAudioUnavailable(.denied)
        )
        // Пока микрофон не спрошен, разговор о системном звуке подождёт —
        // но не пропадёт: после первого запроса микрофон станет `granted`.
        XCTAssertEqual(
            CaptureReadiness.of(microphone: .notAsked, systemAudio: .denied),
            .microphoneWillBeAsked
        )
    }

    /// Причина, по которой системного звука не будет, доносится вся: и
    /// отказ, и отсутствие устройства вывода, и несобравшийся aggregate.
    ///
    /// Исход у них один — собеседника в записи не будет, — и узнать о нём
    /// человек должен до встречи, а не по её транскрипту. Статус едет
    /// внутрь: объясняет причину `SystemAudioUnavailableBadge`, и второго
    /// набора объяснений заводить нельзя.
    func testEveryReasonForNoSystemAudioReachesTheBadge() {
        for status in [SystemAudioStatus.denied, .unsupported, .noOutputDevice, .aggregateFailed] {
            XCTAssertEqual(
                CaptureReadiness.of(microphone: .granted, systemAudio: status),
                .systemAudioUnavailable(status),
                "\(status) потерялся по дороге"
            )
        }
    }
}
