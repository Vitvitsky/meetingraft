import Darwin
import Foundation

/// Общие часы двух каналов захвата.
///
/// `mach_absolute_time` — единственные часы, которые видят оба источника:
/// у микрофонного tap'а это `AVAudioTime.hostTime`, у системного IOProc —
/// `AudioTimeStamp.mHostTime`, и это одни и те же тики. Общее начало
/// отсчёта для mic и system берётся только отсюда.
///
/// Счёт кадрами при этом остаётся: **внутри** канала он точнее часов.
/// Часы нужны ровно на одно — привязать начало каждого канала к началу
/// записи. Без этого оба канала помечали своё начало нулём, и на встрече
/// `6CE19EC5` их разошедшиеся на 1150 мс дорожки выглядели одновременными.
struct HostClock: Sendable {
    /// Тики → наносекунды. На Apple Silicon `numer/denom` = 125/3, на
    /// Intel 1/1: считать множитель обязательно, тик не наносекунда.
    let numerator: UInt64
    let denominator: UInt64
    /// Сейчас, в тех же тиках, что у буферов захвата.
    ///
    /// Отдельным полем, а не прямым вызовом `mach_absolute_time`: без
    /// подменяемого «сейчас» тест не может задать разницу стартов и
    /// проверять ему остаётся нечего.
    let now: @Sendable () -> UInt64

    static let system = HostClock()

    init() {
        var info = mach_timebase_info_data_t(numer: 0, denom: 0)
        _ = mach_timebase_info(&info)
        // Ноль сделал бы любую разницу нулём или делением на ноль —
        // молчаливо сведя все метки к одному моменту.
        numerator = UInt64(max(info.numer, 1))
        denominator = UInt64(max(info.denom, 1))
        now = { mach_absolute_time() }
    }

    /// Часы с заданным множителем — для тестов на обеих архитектурах.
    init(numerator: UInt64, denominator: UInt64, now: @escaping @Sendable () -> UInt64) {
        self.numerator = max(numerator, 1)
        self.denominator = max(denominator, 1)
        self.now = now
    }

    /// Сколько миллисекунд прошло от `from` до `to`.
    ///
    /// Считается только разница, никогда абсолютное время: тики идут от
    /// загрузки машины, и умножение такого числа на числитель переполнило
    /// бы `UInt64`.
    ///
    /// Обратный порядок даёт ноль: буфер не может быть записан раньше
    /// старта записи, и отрицательного сдвига у канала не бывает. Ноль
    /// здесь не прячется — координатор пишет старт **каждого** канала в
    /// журнал, поэтому неожидаемый ноль виден в нём наравне с остальным.
    func elapsedMs(from: UInt64, to: UInt64) -> UInt64 {
        guard to > from else { return 0 }
        return (to - from) * numerator / denominator / 1_000_000
    }
}
