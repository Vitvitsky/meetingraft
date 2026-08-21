#!/usr/bin/env swift
//
//  Прибор для дерева Accessibility (вопрос 2026-08-20).
//
//  Отвечает ровно на один вопрос: **видно ли из чужого приложения, кто
//  говорит прямо сейчас**. Имена участников из дерева достанутся почти
//  наверняка — Zoom нативный и поддерживает VoiceOver. Подсветка
//  активного говорящего — другое дело: жёлтая рамка вокруг плитки может
//  быть чистой отрисовкой и в дереве не отражаться вовсе. Три возможных
//  ответа — атрибут `AXSelected`, меняющийся заголовок, отдельный
//  элемент — и разница между ними это разница между готовой атрибуцией
//  и ничем.
//
//  Прибор ничего не решает и никуда не пишет. Он показывает дерево и
//  то, что в нём меняется, когда говорящий сменился.
//
//  **Каждый запуск начинается с заведомо положительного и заведомо
//  отрицательного случая.** Правило писано кровью:
//  `scripts/count-audio-taps.swift` печатал «tap'ов: 0» при заведомо
//  идущей записи, ноль прочли как «утечки нет», а скрипт был слеп
//  (`CLAUDE.md`). Пустое дерево от слепого прибора выглядит ровно так
//  же, как приложение, которое ничего не отдаёт.
//
//  Три исхода этот прибор различает и никогда не сливает в один:
//
//  1. **Прибор слеп** — разрешения нет или дерево не читается вовсе.
//     Настоящих данных не касаемся.
//  2. **Сравнивать нечего** — целевое приложение не запущено, окон нет.
//  3. **Смотрели и не нашли** — дерево прочитано, изменений при смене
//     говорящего нет. Вот это ответ, и он отрицательный.
//
//  Запуск (Swift есть только на Маке):
//
//      swift scripts/ax-probe.swift check
//      swift scripts/ax-probe.swift dump [--app <id|имя>] [--depth N]
//      swift scripts/ax-probe.swift names [--app <id|имя>]
//      swift scripts/ax-probe.swift watch [секунды] [--app <id|имя>]
//
//  **Разрешение достаётся не скрипту, а ответственному процессу.**
//  `swift scripts/ax-probe.swift` — не отдельная программа: `swift`
//  исполняет скрипт в себе, а TCC смотрит не на просящий процесс, а на
//  responsible process — приложение, из которого выросла цепочка
//  запуска. Для команды в терминале это сам терминал.
//
//  Отсюда: право следует за терминалом, а не за скриптом. Запуск из
//  iTerm вместо Terminal требует отдельного разрешения; из встроенного
//  терминала IDE ответственным станет IDE. Выдать право самому `swift`
//  нельзя осмысленно — бинарь тулчейна не подписан как приложение и
//  меняет идентичность с каждым обновлением.
//
//  MeetingRaft тут ни при чём: если обход дерева переедет внутрь
//  приложения, это будет другой грант.
//
//  Забыть об этом — получить отказ и прочитать его как «Zoom ничего не
//  отдаёт».

import AppKit
import ApplicationServices

// MARK: - Границы обхода

/// Глубина дерева. Zoom рисует плитки участников неглубоко, но дерево
/// целиком велико, и без границы обход уходит в минуты.
let defaultMaxDepth = 14
/// Потолок по числу узлов за один снимок.
let maxNodes = 20000
/// Период опроса в режиме слежения. `AXObserver` на смену подсветки
/// уведомления может не прислать вовсе — потому опрос, а не подписка.
let sampleIntervalMs = 250

/// Строка, которой в дереве быть не может. Отрицательный контроль:
/// поиск обязан уметь отвечать «нет».
let sentinel = "ЗАВЕДОМО-НЕТ-В-ДЕРЕВЕ-7F3A9C"

/// Кого смотрим по умолчанию.
let defaultBundleId = "us.zoom.xos"

// MARK: - Чтение атрибутов

/// Значение атрибута как строка, либо `nil`, если атрибута нет.
func attributeString(_ element: AXUIElement, _ name: String) -> String? {
    var raw: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &raw) == .success,
          let raw
    else {
        return nil
    }
    if let text = raw as? String {
        return text.isEmpty ? nil : text
    }
    if let number = raw as? NSNumber {
        return number.stringValue
    }
    return nil
}

func attributeNames(_ element: AXUIElement) -> [String] {
    var raw: CFArray?
    guard AXUIElementCopyAttributeNames(element, &raw) == .success,
          let names = raw as? [String]
    else {
        return []
    }
    return names
}

func children(_ element: AXUIElement) -> [AXUIElement] {
    var raw: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &raw) == .success,
          let raw
    else {
        return []
    }
    guard CFGetTypeID(raw) == CFArrayGetTypeID() else { return [] }
    return (raw as? [AXUIElement]) ?? []
}

// MARK: - Снимок дерева

/// Узел дерева в том виде, в каком его сравнивают между снимками.
///
/// Путь — индексы от корня. Это единственный устойчивый способ
/// опознать узел: идентификаторов у элементов Zoom может не быть вовсе,
/// а по заголовку узлы не различить — их десятки с пустым заголовком.
struct Node {
    let path: String
    let depth: Int
    /// Атрибуты, по которым ищется признак говорящего. Позиция и размер
    /// сюда не входят: они дрожат от анимации и утопили бы диff в шуме.
    let attributes: [String: String]
    /// Все имена атрибутов узла — в дампе печатаются целиком, потому что
    /// дерево не документировано и заранее неизвестно, что искать.
    let allAttributeNames: [String]

    var role: String { attributes["AXRole"] ?? "—" }

    /// Подпись для сравнения снимков.
    var signature: String {
        attributes
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=\($0.value)" }
            .joined(separator: "|")
    }
}

/// Атрибуты, которые снимаются у каждого узла.
let watchedAttributes = [
    "AXRole",
    "AXSubrole",
    "AXRoleDescription",
    "AXTitle",
    "AXValue",
    "AXDescription",
    "AXHelp",
    "AXIdentifier",
    "AXSelected",
    "AXFocused",
    "AXEnabled",
]

struct Snapshot {
    var nodes: [Node] = []
    /// Обход упёрся в границу. Печатается всегда: молчаливое обрезание
    /// читается как «обошли всё».
    var hitNodeCap = false
    var hitDepthCap = false
    var deepestReached = 0
}

func snapshot(of root: AXUIElement, maxDepth: Int) -> Snapshot {
    var result = Snapshot()
    var stack: [(element: AXUIElement, path: String, depth: Int)] = [(root, "0", 0)]

    while let current = stack.popLast() {
        if result.nodes.count >= maxNodes {
            result.hitNodeCap = true
            break
        }
        result.deepestReached = max(result.deepestReached, current.depth)

        var attributes: [String: String] = [:]
        for name in watchedAttributes {
            if let value = attributeString(current.element, name) {
                attributes[name] = value
            }
        }
        result.nodes.append(
            Node(
                path: current.path,
                depth: current.depth,
                attributes: attributes,
                allAttributeNames: attributeNames(current.element)
            )
        )

        if current.depth >= maxDepth {
            result.hitDepthCap = true
            continue
        }
        let kids = children(current.element)
        for (index, kid) in kids.enumerated().reversed() {
            stack.append((kid, "\(current.path).\(index)", current.depth + 1))
        }
    }
    return result
}

// MARK: - Цель

struct Target {
    let app: NSRunningApplication
    let element: AXUIElement
}

/// Найти запущенное приложение по bundle id или по имени.
func findApp(_ needle: String) -> NSRunningApplication? {
    let running = NSWorkspace.shared.runningApplications
    if let exact = running.first(where: { $0.bundleIdentifier == needle }) {
        return exact
    }
    let lowered = needle.lowercased()
    return running.first { app in
        (app.localizedName?.lowercased().contains(lowered) ?? false)
            || (app.bundleIdentifier?.lowercased().contains(lowered) ?? false)
    }
}

func makeTarget(_ needle: String) -> Target? {
    guard let app = findApp(needle), app.processIdentifier > 0 else { return nil }
    return Target(app: app, element: AXUIElementCreateApplication(app.processIdentifier))
}

// MARK: - Самопроверка

/// Проверка прибора до всяких данных.
///
/// Порядок обратный интуитивному: сперва доказываем, что дерево вообще
/// читается, и лишь потом смотрим на цель. Иначе пустой результат по
/// Zoom невозможно отличить от отсутствия разрешения.
func selfCheck() -> Bool {
    print("=== Самопроверка ===")

    // 1. Разрешение. Без prompt: диалог посреди прибора превратил бы
    //    отказ в «наверное, я что-то не так нажал».
    let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: false] as CFDictionary
    let trusted = AXIsProcessTrustedWithOptions(options)
    print("  разрешение Accessibility: \(trusted ? "есть" : "НЕТ")")
    if !trusted {
        print("""

          Разрешения нет. Выдать его надо **приложению, из которого
          запущена команда**: Системные настройки → Конфиденциальность и
          безопасность → Универсальный доступ → добавить Терминал (или
          iTerm, или ту IDE, чей терминал используется).

          Не Swift и не MeetingRaft. TCC выдаёт право не просящему
          процессу, а ответственному за него: `swift` исполняет скрипт в
          себе, и ответственным остаётся терминал. Сменишь терминал —
          понадобится выдать право заново.
        """)
        return false
    }

    // 2. Заведомо положительный случай: приложение, про которое известно
    //    из другого источника, что оно есть и на экране. Пустое дерево
    //    здесь означает слепоту прибора, а не молчание приложения.
    guard let front = NSWorkspace.shared.frontmostApplication else {
        print("  положительный контроль: НЕТ активного приложения — проверять нечем")
        return false
    }
    let frontName = front.localizedName ?? "—"
    let control = snapshot(of: AXUIElementCreateApplication(front.processIdentifier), maxDepth: 6)
    let withText = control.nodes.filter { $0.attributes.count > 1 }.count
    print("  положительный контроль: \(frontName) — узлов \(control.nodes.count), с атрибутами \(withText)")
    if control.nodes.count < 2 || withText == 0 {
        print("""

          Прибор слеп: у заведомо видимого приложения дерево пустое.
          До настоящих данных дело не дошло — любой ноль ниже был бы
          нолём прибора, а не ответом про Zoom.
        """)
        return false
    }

    // 3. Заведомо отрицательный случай: поиск обязан уметь отвечать
    //    «нет». Прибор, который всё находит, бесполезен ровно так же,
    //    как прибор, который ничего не находит.
    let sentinelHits = control.nodes.filter { $0.signature.contains(sentinel) }.count
    print("  отрицательный контроль: строки-пустышки найдено \(sentinelHits) (ожидается 0)")
    if sentinelHits != 0 {
        print("\n  Поиск находит то, чего нет. Прибору верить нельзя.")
        return false
    }

    // 4. Отказ на несуществующем процессе. Проверяем, что путь отказа
    //    вообще работает и не выглядит успехом с пустым результатом.
    let bogus = AXUIElementCreateApplication(pid_t(999_999))
    var raw: CFTypeRef?
    let bogusError = AXUIElementCopyAttributeValue(bogus, kAXChildrenAttribute as CFString, &raw)
    print("  отказ на несуществующем процессе: \(bogusError == .success ? "УСПЕХ — плохо" : "ошибка, как и должно")")
    if bogusError == .success {
        print("\n  Несуществующий процесс отвечает успехом. Различать исходы нечем.")
        return false
    }

    print("  прибор годен\n")
    return true
}

// MARK: - Печать

func stamp() -> String {
    let formatter = DateFormatter()
    formatter.dateFormat = "HH:mm:ss.SSS"
    return formatter.string(from: Date())
}

func printCaps(_ snap: Snapshot, maxDepth: Int) {
    if snap.hitNodeCap {
        print("  ! обход упёрся в потолок узлов (\(maxNodes)) — дерево показано не целиком")
    }
    if snap.hitDepthCap {
        print("  ! обход упёрся в глубину \(maxDepth) — ниже не смотрели (--depth больше)")
    }
}

/// Цель есть? Иначе — «сравнивать нечего», и это не то же самое, что
/// «смотрели и не нашли».
func requireTarget(_ needle: String) -> Target? {
    guard let target = makeTarget(needle) else {
        print("""
        Сравнивать нечего: приложение «\(needle)» не запущено.

        Это не ответ на вопрос про спикеров. Запусти встречу и повтори.
        """)
        return nil
    }
    print("Цель: \(target.app.localizedName ?? "—") · \(target.app.bundleIdentifier ?? "—") · pid \(target.app.processIdentifier)")
    return target
}

// MARK: - Режимы

func runDump(_ needle: String, maxDepth: Int) -> Int32 {
    guard let target = requireTarget(needle) else { return 2 }
    let snap = snapshot(of: target.element, maxDepth: maxDepth)
    print("Узлов: \(snap.nodes.count), глубже всего \(snap.deepestReached)")
    printCaps(snap, maxDepth: maxDepth)

    if snap.nodes.count <= 1 {
        print("""

        Дерево пустое, но **прибор при этом зрячий** — положительный
        контроль выше прошёл. Значит это ответ: приложение своего
        интерфейса в Accessibility не отдаёт.
        """)
        return 0
    }

    print("\n--- дерево ---")
    for node in snap.nodes.sorted(by: { $0.path < $1.path }) {
        let indent = String(repeating: "  ", count: node.depth)
        let described = node.attributes
            .filter { $0.key != "AXRole" }
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=«\($0.value)»" }
            .joined(separator: " ")
        print("\(indent)\(node.path) \(node.role) \(described)")
    }

    // Имена атрибутов печатаются отдельным списком: дерево не
    // документировано, и признак говорящего может лежать в атрибуте,
    // которого нет в `watchedAttributes`.
    var seenAttributes = Set<String>()
    for node in snap.nodes { seenAttributes.formUnion(node.allAttributeNames) }
    let unwatched = seenAttributes.subtracting(watchedAttributes).sorted()
    print("\n--- атрибуты, встреченные в дереве, но не снимаемые ---")
    print(unwatched.isEmpty ? "  (нет)" : "  " + unwatched.joined(separator: ", "))
    return 0
}

func runNames(_ needle: String, maxDepth: Int) -> Int32 {
    guard let target = requireTarget(needle) else { return 2 }
    let snap = snapshot(of: target.element, maxDepth: maxDepth)
    printCaps(snap, maxDepth: maxDepth)

    var candidates: [String: Int] = [:]
    for node in snap.nodes {
        guard node.role == "AXStaticText" || node.role == "AXButton" || node.role == "AXCell" else { continue }
        for key in ["AXValue", "AXTitle", "AXDescription"] {
            guard let value = node.attributes[key] else { continue }
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard (2 ... 60).contains(trimmed.count) else { continue }
            candidates[trimmed, default: 0] += 1
        }
    }

    print("\n--- строки, похожие на подписи участников (\(candidates.count)) ---")
    if candidates.isEmpty {
        print("""
          Ни одной. Прибор зрячий — значит это ответ, а не отказ.
        """)
        return 0
    }
    for (text, count) in candidates.sorted(by: { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }) {
        print("  \(count)×  «\(text)»")
    }
    print("""

    Сверь список с тем, кто в звонке на самом деле. Имена в нём — это
    ещё не атрибуция: вопрос в том, меняется ли что-нибудь, когда
    говорящий сменился. Это `watch`.
    """)
    return 0
}

func runWatch(_ needle: String, seconds: Int, maxDepth: Int) -> Int32 {
    guard let target = requireTarget(needle) else { return 2 }
    print("""
    Слежу \(seconds) с, снимок каждые \(sampleIntervalMs) мс.

    Говори по очереди и запоминай, кто и когда. Прибор печатает только
    изменения — если при смене говорящего не напечаталось ничего, значит
    подсветка в дереве не отражена, и это ответ.
    """)

    var previous: [String: String] = [:]
    var samples = 0
    var changeEvents = 0
    let started = Date()
    var reportedCaps = false

    while Date().timeIntervalSince(started) < Double(seconds) {
        let snap = snapshot(of: target.element, maxDepth: maxDepth)
        if !reportedCaps {
            printCaps(snap, maxDepth: maxDepth)
            reportedCaps = true
        }
        samples += 1

        var current: [String: String] = [:]
        for node in snap.nodes {
            current[node.path] = node.signature
        }

        if !previous.isEmpty {
            var lines: [String] = []
            var changedNodes = 0
            for (path, signature) in current where previous[path] != signature {
                changedNodes += 1
                if let was = previous[path] {
                    lines.append("    \(path) было: \(was)")
                    lines.append("    \(path) стало: \(signature)")
                } else {
                    lines.append("    \(path) появился: \(signature)")
                }
            }
            for path in previous.keys where current[path] == nil {
                changedNodes += 1
                lines.append("    \(path) исчез")
            }
            if !lines.isEmpty {
                changeEvents += 1
                print("\n[\(stamp())] узлов изменилось: \(changedNodes)")
                for line in lines.prefix(40) { print(line) }
                if lines.count > 40 {
                    print("    … ещё \(lines.count - 40) строк опущено")
                }
            }
        }
        previous = current
        Thread.sleep(forTimeInterval: Double(sampleIntervalMs) / 1000)
    }

    print("\n=== Итог ===")
    print("  снимков: \(samples), моментов с изменениями: \(changeEvents)")
    if changeEvents == 0 {
        print("""

          Дерево не изменилось ни разу за всё время наблюдения.

          Два разных смысла, и различить их может только человек:
          говорящий не менялся вовсе — или менялся, а дерево этого не
          показывает. Если менялся, то ответ отрицательный: подсветку
          активного спикера из Accessibility не видно, и атрибуцию по
          ней не построить.
        """)
    } else {
        print("""

          Изменения есть. Дальше глазами: найди среди путей выше тот,
          который менялся ровно в моменты смены говорящего, — и сверь
          с записанным по времени. Совпало у одного пути — это и есть
          признак активного спикера.
        """)
    }
    return 0
}

// MARK: - Разбор аргументов

let usage = """
Прибор для дерева Accessibility: видно ли, кто говорит.

  swift scripts/ax-probe.swift check
      Только самопроверка: есть ли разрешение и читается ли дерево.

  swift scripts/ax-probe.swift dump  [--app <id|имя>] [--depth N]
      Дерево целевого приложения целиком.

  swift scripts/ax-probe.swift names [--app <id|имя>] [--depth N]
      Строки, похожие на подписи участников.

  swift scripts/ax-probe.swift watch [секунды] [--app <id|имя>] [--depth N]
      Следить и печатать, что меняется. Главный режим.

По умолчанию цель — \(defaultBundleId), глубина \(defaultMaxDepth).
"""

var arguments = Array(CommandLine.arguments.dropFirst())
var appNeedle = defaultBundleId
var maxDepth = defaultMaxDepth

var index = 0
var positional: [String] = []
while index < arguments.count {
    switch arguments[index] {
    case "--app":
        guard index + 1 < arguments.count else {
            print("--app без значения")
            exit(2)
        }
        appNeedle = arguments[index + 1]
        index += 2
    case "--depth":
        guard index + 1 < arguments.count, let value = Int(arguments[index + 1]) else {
            print("--depth без числа")
            exit(2)
        }
        maxDepth = value
        index += 2
    default:
        positional.append(arguments[index])
        index += 1
    }
}

guard let mode = positional.first else {
    print(usage)
    exit(0)
}

// Сперва прибор, потом данные.
guard selfCheck() else {
    print("Прибор слеп: до настоящих данных дело не дошло.")
    exit(1)
}

switch mode {
case "check":
    exit(0)
case "dump":
    exit(runDump(appNeedle, maxDepth: maxDepth))
case "names":
    exit(runNames(appNeedle, maxDepth: maxDepth))
case "watch":
    let seconds = positional.count > 1 ? (Int(positional[1]) ?? 30) : 30
    exit(runWatch(appNeedle, seconds: seconds, maxDepth: maxDepth))
default:
    print(usage)
    exit(2)
}
