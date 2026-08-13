import Foundation

/// Источник PCM-сэмплов одного канала (ADR-004).
///
/// Существует ради тестируемости координатора: реальный микрофон и
/// системный process tap юнит-тестами не покрыть, поэтому в тестах
/// подставляются фейки.
protocol AudioTapping: AnyObject {
    /// Порция сэмплов и момент её записи в тиках `mach_absolute_time`.
    ///
    /// Момент нужен потому, что счётчик кадров канала не знает, когда
    /// канал начался: оба канала считали бы от своего нуля, а стартуют
    /// они с разницей около секунды. Общее время — только из этих тиков
    /// (см. `HostClock`).
    typealias SamplesHandler = (_ samples: [Float], _ hostTime: UInt64) -> Void

    /// Готов ли источник отдавать звук. Для системного tap зависит от
    /// разрешения и версии macOS, поэтому проверяется после `prepare()`.
    var isAvailable: Bool { get }

    /// Разведка без старта: узнать доступность, не открывая поток.
    func prepare()

    /// `onSamples` вызывается off-main, 16 kHz mono Float32.
    func start(onSamples: @escaping SamplesHandler) throws

    func stop()
}

extension AudioTapping {
    /// По умолчанию источник доступен и не требует разведки — так ведёт
    /// себя микрофон, у которого доступность решается разрешением TCC.
    var isAvailable: Bool {
        true
    }

    func prepare() {}
}
