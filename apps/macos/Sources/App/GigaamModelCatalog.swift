import Foundation

/// Файлы русского движка GigaAM v3 (parity с Rust `stt::gigaam_path`).
///
/// Имена фиксированы и совпадают с теми, что ищет ядро: подстановка
/// файла из соседнего экспорта не сломала бы ничего видимого — движок
/// просто распознавал бы хуже.
enum GigaamModelFile: String, CaseIterable, Identifiable, Sendable {
    case encoder = "encoder.int8.onnx"
    case decoder = "decoder.onnx"
    case joiner = "joiner.onnx"
    case tokens = "tokens.txt"

    var id: String {
        rawValue
    }

    /// Примерный размер. Нужен не для красоты: прогресс по четырём
    /// файлам без весов дёргался бы рывками — три из них весят меньше
    /// процента от общей загрузки.
    var approximateBytes: Int64 {
        switch self {
        case .encoder: 224_570_814
        case .decoder: 3_331_651
        case .joiner: 1_440_448
        case .tokens: 196
        }
    }
}

/// Откуда качать и куда класть.
///
/// Версия экспорта зашита строкой — ровно как в
/// `scripts/fetch-gigaam-models.sh`, и по той же причине: эти файлы лежат
/// на Hugging Face, но **в релиз sherpa-onnx не попали**
/// (issue k2-fsa/sherpa-onnx#3619), так что «последняя версия» здесь
/// означает «неизвестно что». Меняется версия — правятся оба места, и
/// скрипт, и этот файл.
enum GigaamModelCatalog {
    static let export = "sherpa-onnx-nemo-transducer-giga-am-v3-russian-2025-12-16"

    /// Каталог модели внутри `models/` — тот же, что ищет ядро.
    static func directory(modelsDirectory: URL) -> URL {
        modelsDirectory.appendingPathComponent("gigaam", isDirectory: true)
    }

    static func sourceURL(for file: GigaamModelFile) -> URL? {
        URL(string: "https://huggingface.co/csukuangfj/\(export)/resolve/main/\(file.rawValue)")
    }

    static func destinationURL(modelsDirectory: URL, file: GigaamModelFile) -> URL {
        directory(modelsDirectory: modelsDirectory)
            .appendingPathComponent(file.rawValue, isDirectory: false)
    }

    /// Метка рядом со скачанным файлом: что именно в него скачано.
    ///
    /// Тот же формат, что пишет скрипт (`<экспорт>/<имя файла>`), и это
    /// не косметика. Скрипт по метке решает, перекачивать ли файл; если
    /// приложение скачает без метки, скрипт сочтёт файл неизвестным и
    /// перекачает 225 МБ заново. Две дороги к одному каталогу обязаны
    /// оставлять одинаковые следы.
    static func markerURL(for destination: URL) -> URL {
        URL(fileURLWithPath: destination.path + ".source")
    }

    static func markerContents(for file: GigaamModelFile) -> String {
        "\(export)/\(file.rawValue)"
    }

    /// Общий размер загрузки — показывается человеку до её начала.
    static var approximateTotalBytes: Int64 {
        GigaamModelFile.allCases.reduce(0) { $0 + $1.approximateBytes }
    }

    /// Все ли файлы на месте.
    ///
    /// Ответ приложения на вопрос «показывать ли кнопку». Настоящий
    /// ответ — «готов ли движок» — даёт ядро (`gigaamModelReady`), и он
    /// строже: там ещё и фича сборки.
    static func isInstalled(modelsDirectory: URL) -> Bool {
        GigaamModelFile.allCases.allSatisfy { file in
            FileManager.default.fileExists(
                atPath: destinationURL(modelsDirectory: modelsDirectory, file: file).path
            )
        }
    }
}
