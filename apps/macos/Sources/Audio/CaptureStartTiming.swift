import Foundation

/// Один шаг подъёма захвата и его цена.
struct CaptureStartStep: Equatable {
    /// Имя шага, каким оно уйдёт в журнал: `system:create_tap`.
    let name: String
    /// Цена шага, мс. Ноль значит «меньше миллисекунды» — то есть «не этот
    /// шаг съедает секунду», а не «шага не было».
    let elapsedMs: UInt64
}

/// Секундомер по шагам: сколько стоил каждый вызов на пути к первому буферу.
///
/// Существует потому, что системный канал начинается на секунду позже
/// микрофонного (задача 3 Epic 25), и **из кода не видно, какой шаг эту
/// секунду съедает**: `AudioHardwareCreateProcessTap`, сборка aggregate и
/// `AudioDeviceStart` уходят в `coreaudiod`, и цена у них не наша.
///
/// Мерить, а не рассуждать: правка, объяснённая рассуждением вместо замера,
/// в этом проекте уже ухудшала ровно то, что бралась улучшать (`CLAUDE.md`).
/// Поэтому сперва прибор, и только потом перестановка шагов.
struct CaptureStepTimer {
    private let clock: HostClock
    private var mark: UInt64
    private(set) var steps: [CaptureStartStep] = []

    init(clock: HostClock = .system) {
        self.clock = clock
        mark = clock.now()
    }

    /// Закрыть шаг: время от прошлой отметки уходит в `steps`.
    mutating func step(_ name: String) {
        let now = clock.now()
        steps.append(CaptureStartStep(name: name, elapsedMs: clock.elapsedMs(from: mark, to: now)))
        mark = now
    }
}
