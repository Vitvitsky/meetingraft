import Foundation

/// Источник PCM-сэмплов одного канала (ADR-004).
///
/// Существует ради тестируемости координатора: реальный микрофон и
/// системный process tap юнит-тестами не покрыть, поэтому в тестах
/// подставляются фейки.
protocol AudioTapping: AnyObject {
    /// Готов ли источник отдавать звук. Для системного tap зависит от
    /// разрешения и версии macOS, поэтому проверяется после `prepare()`.
    var isAvailable: Bool { get }

    /// Разведка без старта: узнать доступность, не открывая поток.
    func prepare()

    /// `onSamples` вызывается off-main, 16 kHz mono Float32.
    func start(onSamples: @escaping ([Float]) -> Void) throws

    func stop()
}

extension AudioTapping {
    /// По умолчанию источник доступен и не требует разведки — так ведёт
    /// себя микрофон, у которого доступность решается разрешением TCC.
    var isAvailable: Bool { true }

    func prepare() {}
}
