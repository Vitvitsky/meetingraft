@preconcurrency import AVFoundation
import Foundation

/// Приведение произвольного аппаратного формата к 16 kHz mono Float32.
///
/// Общий код для микрофона (`AVAudioEngine`) и системного tap (Core Audio):
/// оба отдают буферы в формате устройства — обычно 44.1/48 kHz и стерео,
/// а STT ждёт моно 16 kHz (ADR-005).
final class PCMDownmixer {
    private let converter: AVAudioConverter
    private let targetFormat: AVAudioFormat

    /// `nil`, если формат источника непригоден или конвертер недоступен.
    init?(from sourceFormat: AVAudioFormat) {
        guard sourceFormat.sampleRate > 0, sourceFormat.channelCount > 0 else {
            return nil
        }
        guard
            let target = AVAudioFormat(
                commonFormat: .pcmFormatFloat32,
                sampleRate: AudioChunkPipeline.targetSampleRate,
                channels: 1,
                interleaved: false
            ),
            let converter = AVAudioConverter(from: sourceFormat, to: target)
        else {
            return nil
        }
        self.converter = converter
        targetFormat = target
    }

    /// Пустой массив, если конвертация не дала кадров.
    func convert(_ buffer: AVAudioPCMBuffer) -> [Float] {
        guard buffer.frameLength > 0 else { return [] }

        let ratio = targetFormat.sampleRate / buffer.format.sampleRate
        // Запас в 64 кадра — не округление, а слив отставания.
        //
        // Конвертер придерживает часть входа между вызовами и отдаёт её
        // со следующим чанком. Потолок буфера выше входной доли ровно на
        // эти 64 кадра, и на столько же за вызов рассасывается отставание.
        //
        // Больший запас делает **хуже**, и это замерено на Маке
        // 2026-08-08: с запасом в целый чанк конвертер переходит на
        // выдачу целыми блоками по 4096 входных кадров и после 48000
        // кадров держит внутри 987 вместо 262. Прежде чем увеличивать —
        // мерить, а не рассуждать.
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 64
        guard let out = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else {
            return []
        }

        // Конвертер живёт между вызовами: запись — один непрерывный поток,
        // а 100 мс — размер кадра живого пути, не граница звука.
        //
        // Отсюда `noDataNow`, а не `endOfStream`. `endOfStream` заставляет
        // ресемплер дожать хвост, добив вход нулями, и после него состояние
        // фильтра приходится сбрасывать — то есть на каждой границе чанка
        // возникал разрыв. Слышно его не было, но пакетный проход по записи
        // (ADR-011) считает именно по этим сэмплам, а там точность и есть
        // цель.
        //
        // Кадры при этом не теряются, а **отстают**: ресемплер придерживает
        // несколько сэмплов внутри и отдаёт их со следующим чанком. Отсюда
        // старое наблюдение «на 48→16 kHz теряется ~15%» — оно верно для
        // одного вызова и неверно для потока. Мерить надо накопительно.
        //
        // Цена: последние несколько миллисекунд записи остаются в
        // конвертере навсегда, потому что `endOfStream` не приходит уже
        // никогда. Это лучше разрыва каждые 100 мс, и если хвост
        // понадобится — это отдельная работа со своим `drain()`.
        var consumed = false
        var error: NSError?
        var samples: [Float] = []

        while true {
            let status = converter.convert(to: out, error: &error) { _, status in
                if consumed {
                    status.pointee = .noDataNow
                    return nil
                }
                consumed = true
                status.pointee = .haveData
                return buffer
            }

            if error != nil {
                break
            }
            if out.frameLength > 0, let channel = out.floatChannelData?[0] {
                samples.append(
                    contentsOf: UnsafeBufferPointer(start: channel, count: Int(out.frameLength))
                )
            }
            // `inputRanDry` — обычное завершение вызова: вход кончился, а
            // состояние осталось для следующего чанка.
            if status != .haveData || out.frameLength == 0 {
                break
            }
        }

        return samples
    }
}
