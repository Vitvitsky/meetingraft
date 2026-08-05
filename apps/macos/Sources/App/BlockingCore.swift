import Foundation

/// Увод блокирующего вызова ядра с главного потока.
///
/// Методы `MeetingCore`, которые ходят в сеть, блокируют вызывающий поток
/// на весь таймаут запроса — 10 секунд у backend API и минута у LLM.
/// Из `@MainActor` это замораживает интерфейс целиком.
///
/// Берётся именно поток из `DispatchQueue.global`, а не `Task.detached`:
/// пул кооперативных потоков Swift Concurrency рассчитан на то, что
/// задача не встаёт надолго, а здесь она встаёт по построению.
func offMainThread<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
    await withCheckedContinuation { continuation in
        DispatchQueue.global(qos: .userInitiated).async {
            continuation.resume(returning: work())
        }
    }
}
