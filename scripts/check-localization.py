#!/usr/bin/env python3
"""Прибор полноты локализации (подпроект 1b).

Swift на Linux-машине не собирается, но каталог и исходники — текст, и
полноту перевода можно проверить разбором, а не обещанием. Прибор
проверяет четыре вещи:

1. ни одного кириллического строкового литерала в `Sources/`, кроме
   явного белого списка;
5. у каждой подстановки установлен тип: либо `Int(...)`, либо выражение
   из списка заведомо строковых. Иначе спецификатор ключа неизвестен, и
   перевод недостижим;
6. тесты не утверждают про переведённый текст: строка, совпавшая с
   русским значением из каталога, делает тест зависимым от языка машины,
   а чаще — просто красным;
2. каждый ключ из кода есть в каталоге;
3. в каталоге нет ключей, которых нет в коде;
4. у каждого ключа есть русский перевод.

Первая проверка сперва смотрела только на позиции вида `Text(` — и
белый список при этом не срабатывал ни разу, потому что демо-речь и
эндонимы в такие позиции не попадают вовсе. Защита, которая не может
сработать, хуже её отсутствия: она создаёт уверенность (`CLAUDE.md`).
Хуже того, русский из `ProviderSettingsStore` доезжает до экрана через
переменную и мимо всех таких шаблонов. Проверка поэтому смотрит на
**все** строковые литералы, а белый список получил настоящую работу.

**Чего прибор не делает.** Он не компилирует Swift. Опечатка в
синтаксисе, неверный тип аргумента, сломанная интерполяция — всё это
останется невидимым до Мака. Здесь проверяются полнота каталога и
отсутствие русского в интерфейсе, и только это.

Запуск: python3 scripts/check-localization.py
Выход: 0 — всё сошлось, 1 — расхождение.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCES = ROOT / "apps/macos/Sources"
TESTS = ROOT / "apps/macos/Tests"
CATALOG = SOURCES / "Resources/Localizable.xcstrings"

CYRILLIC = re.compile(r"[Ѐ-ӿ]")

# Позиции, в которых литерал становится ключом локализации.
#
# `Text(`, `Button(`, `.help(`, `.navigationTitle(`, `.alert(` берут
# `LocalizedStringKey` — литерал в них локализуем как есть. `String(
# localized:` — то же самое вне вьюхи.
KEY_POSITIONS = [
    (re.compile(r"\bText\("), "Text"),
    (re.compile(r"\bButton\("), "Button"),
    (re.compile(r"\.help\("), "help"),
    (re.compile(r"\.navigationTitle\("), "navigationTitle"),
    (re.compile(r"\.alert\("), "alert"),
    (re.compile(r"\.confirmationDialog\("), "confirmationDialog"),
    (re.compile(r"\.accessibilityLabel\("), "accessibilityLabel"),
    (re.compile(r"\bContentUnavailableView\("), "ContentUnavailableView"),
    (re.compile(r"\bPicker\("), "Picker"),
    (re.compile(r"\bToggle\("), "Toggle"),
    (re.compile(r"\bLabel\("), "Label"),
    (re.compile(r"\bMenu\("), "Menu"),
    (re.compile(r"\bCommandMenu\("), "CommandMenu"),
    (re.compile(r"\bTextField\("), "TextField"),
    (re.compile(r"\blocalized:"), "String(localized:)"),
]

# Файлы, которым кириллица положена по существу (спека, решение 3).
#
# Белый список нужен явно: без него проверка краснела бы на честном коде,
# и её отключили бы целиком. Каждая строка списка обязана что-то
# исключать — прибор это и проверяет сам, см. `report_unused_allowances`.
CYRILLIC_ALLOWED = {
    # Демо-речь: это содержимое распознавания, которым показывают русские
    # субтитры, а не подпись элемента. Перевод сломал бы демонстрацию.
    "LiveCaptions/FakeCaptionStream.swift",
    # Эндонимы языков: список языков принято показывать на них самих.
    "App/SpeechLanguage.swift",
}

# Построчное послабление: `// loc:allow` в конце строки.
#
# Пофайловый список для этого груб. Есть строки, которым кириллица
# положена не потому, что файл особый, а потому, что строка — данные:
# имя спикера по умолчанию уходит в транскрипт и следует языку встречи,
# а не языку интерфейса. Помечать такое надо там, где оно живёт.
LINE_ALLOW = "// loc:allow"

# Ключи, которые в коде не встречаются буквально, потому что собираются
# из вариаций числа. Каталог обязан их содержать, код — нет.
PLURAL_KEYS: set[str] = set()


def string_literals(line: str) -> list[tuple[int, str]]:
    """Строковые литералы строки: `(позиция начала, содержимое)`.

    Написано сканером, а не регулярным выражением, ради двух вещей:
    комментарий `//` внутри литерала не должен обрывать разбор, а
    кириллица в комментарии не должна считаться литералом. Комментариев
    по-русски в этом проекте больше, чем кода.
    """
    out: list[tuple[int, str]] = []
    index = 0
    length = len(line)
    while index < length:
        char = line[index]
        if char == "/" and index + 1 < length and line[index + 1] == "/":
            break
        if char == '"':
            start = index
            index += 1
            body: list[str] = []
            while index < length:
                if line[index] == "\\" and index + 1 < length:
                    body.append(line[index : index + 2])
                    index += 2
                    continue
                if line[index] == '"':
                    break
                body.append(line[index])
                index += 1
            out.append((start, "".join(body)))
            index += 1
            continue
        index += 1
    return out


INTERPOLATION = re.compile(r"\\\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\)")
# Имена, по которым подстановка выглядит числовой.
#
# Раньше по этому списку **угадывался** спецификатор, и каталог был
# написан по той же догадке — то есть проверки сравнивали догадку сама с
# собой и зеленели всегда. Ровно тот случай, о котором предупреждает
# `CLAUDE.md`: прибор, который не может покраснеть.
#
# Теперь список работает наоборот: он не назначает спецификатор, а
# **требует** обёртки `Int(...)`. Правило простое и проверяемое — число
# в локализуемой строке подаётся как `Int(...)`, — и тогда `%lld` не
# догадка, а следствие.
#
# Почему это важно: UniFFI отображает `u32` в Swift `UInt32`, а
# интерполяция `UInt32` даёт спецификатор `%u`, не `%lld`. Ключ
# `подписей %lld` не нашёлся бы никогда, и русский перевод девяти строк
# был бы недостижим — вместе с единственными двумя блоками форм числа,
# написанными вручную.
# Выражения, про которые известно, что они строки.
#
# Список закрытый и работает как разрешение: **всё, что не `Int(...)` и
# не здесь, — провал**. Так задумано после второго разбора ревью.
#
# Прежняя версия шла от обратного: перечисляла числовые *имена* и всё
# прочее молча считала строкой. Дыра нашлась сразу — `~\(size) MB`, где
# `size` это `Int?`: имени `size` в списке не было, ключ ушёл в каталог
# как `~%@ MB`, а рантайм спросил бы `~%lld MB`. Закрытый список
# запретов не закрывает ничего: мимо него проходит любое новое имя.
STRING_EXPRESSIONS = {
    "llmModelId", "llmProviderId", "connectionError", "displayName",
    "error", "error.localizedDescription", "term.surface", "term.canonical",
    "term.language.uppercased()", "segment.originalText", "edit.originalText",
    "edit.editedText", "rebuild.provenance", "providerStore.exportFolderPath",
    "stamp", "freed", "detectedApp.displayName", "model.postCallEngineNote",
    "SpeakerFormat.channelLabel(code)",
    "Self.deletionDate.string(from: date(meeting.audioDeletedAtMs))",
}

INT_WRAPPED = re.compile(r"^\\\(Int\(")
# Спецификаторы в готовом значении каталога.
INTERPOLATION_SPEC = re.compile(r"%(?:lld|@|u|d|lf|f)")


def catalog_key(literal: str) -> str:
    r"""Литерал в том виде, в каком ключом его увидит Xcode.

    `Text("Meeting \(stamp)")` даёт ключ `Meeting %@`, а не текст
    исходника. Без этой нормализации проверка полноты каталога врала бы
    ровно на строках с подстановкой — там, где ошибиться проще всего.

    Спецификатор здесь не угадывается: `Int(...)` даёт `%lld`, всё
    остальное — `%@`. За тем, чтобы числа приходили обёрнутыми, следит
    отдельная проверка.
    """
    # Литеральный процент Xcode экранирует: `\(n)%` даёт ключ `%lld%%`.
    # Вопрос был нерешаем без тулчейна, и первая же сборка на Маке его
    # решила — каталог тогда разъехался с кодом на двух ключах.
    escaped = literal.replace("%", "%%")
    return INTERPOLATION.sub(
        lambda match: "%lld" if INT_WRAPPED.match(match.group(0)) else "%@",
        escaped,
    )


def unwrapped_numbers(literal: str) -> list[str]:
    """Подстановки, спецификатор которых неизвестен.

    Ни `Int(...)`, ни строка из списка — значит тип не установлен, а
    вместе с ним и спецификатор ключа. Такую строку прибор не пропускает
    вовсе: догадка здесь уже стоила девяти недостижимых переводов.
    """
    out = []
    for match in INTERPOLATION.finditer(literal):
        expression = match.group(0)
        if INT_WRAPPED.match(expression):
            continue
        if expression[2:-1].strip() in STRING_EXPRESSIONS:
            continue
        out.append(expression)
    return out


def russian_values(strings: dict) -> dict[str, str]:
    """Русские значения каталога — по ним ищутся утверждения в тестах."""
    out: dict[str, str] = {}
    for key, entry in strings.items():
        ru = entry.get("localizations", {}).get("ru", {})
        unit = ru.get("stringUnit")
        if unit and unit.get("value"):
            out[unit["value"]] = key
        for form in ru.get("variations", {}).get("plural", {}).values():
            value = form.get("stringUnit", {}).get("value") if isinstance(form, dict) else None
            if value:
                out[value] = key
    return out


def tests_asserting_translations(values: dict[str, str]) -> list[str]:
    """Литералы в тестах, совпавшие с переводом.

    Прибор смотрел только в `Sources/`, и шесть тестов, утверждавших про
    русский текст интерфейса, пережили перевод незамеченными: часть из
    них падает в любой локали, часть зеленеет только на русской машине.
    Сравнение идёт и по вхождению: `XCTAssertTrue(text.contains("…"))`
    так же ломается, как равенство.
    """
    if not TESTS.exists():
        return []
    hits: list[str] = []
    for path in sorted(TESTS.rglob("*.swift")):
        rel = path.relative_to(TESTS)
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if LINE_ALLOW in line:
                continue
            for _, literal in string_literals(line):
                if not literal or not CYRILLIC.search(literal):
                    continue
                for value, key in values.items():
                    # Перевод без подстановок — то, с чем тест сравнил бы
                    # напрямую или через `contains`.
                    #
                    # Одиночное слово совпадением не считается: `текст` в
                    # тестовых данных совпадает с переводом чипа, а к
                    # интерфейсу отношения не имеет. Отсюда и предел
                    # проверки: тест, сравнивающий с однословным
                    # переводом, она пропустит. Это сито, а не
                    # доказательство.
                    fragment = INTERPOLATION_SPEC.sub(" ", value).strip()
                    if len(fragment.split()) < 2:
                        continue
                    if literal == value or fragment in literal:
                        hits.append(f"{rel}:{number}  «{literal}»  ← перевод ключа «{key}»")
                        break
    return hits


def swift_files() -> list[Path]:
    return sorted(SOURCES.rglob("*.swift"))


def relative(path: Path) -> str:
    return str(path.relative_to(SOURCES))


def key_kind(prefix: str, previous: str) -> str | None:
    """Позиция литерала — ключ локализации или обычная строка.

    Три вещи, каждая из которых стоила прибору правки.

    Смотрит и на предыдущую строку: в `.alert(\n    "текст",` перед
    литералом на его собственной строке нет ничего, кроме отступа. Пока
    префикс брался только из текущей, ключи многострочных вызовов не
    находились вовсе — и проверка полноты каталога их не требовала.
    Нашла это проверка мёртвых ключей.

    Вызов ищется где угодно в префиксе: у второй ветви тернарного
    оператора `isPlaying ? "Stop" : "Play"` перед литералом стоит `:`, а
    `.help(` осталось в начале строки.

    Но ключом становится только **первый** литерал вызова: иначе
    `Button("Title", systemImage: "mic.fill")` требовал бы перевода для
    имени иконки. Признак — между вызовом и литералом нет чужой кавычки,
    либо сразу перед литералом стоит `?` или `:` тернарного оператора.
    """
    for source in (prefix, "" if prefix.strip() else previous.rstrip()):
        if not source:
            continue
        best = None
        for pattern, kind in KEY_POSITIONS:
            for match in pattern.finditer(source):
                if best is None or match.end() > best[0]:
                    best = (match.end(), kind)
        if best is None:
            continue
        tail = source[best[0]:]
        if NON_TEXT_ARGUMENT.search(tail):
            return None
        if '"' not in tail:
            return best[1]
        after_last_quote = tail.rsplit('"', 1)[1]
        if TERNARY_BRANCH.match(after_last_quote):
            return best[1]
    return None


TERNARY_BRANCH = re.compile(r"^\s*[?:]\s*$")

# Именованные аргументы, значение которых человеку не показывают.
#
# `Label(Self.sizeText(bytes), systemImage: "waveform")` — первый литерал
# вызова здесь имя иконки, потому что заголовок литералом не был.
# Правило «ключом становится первый литерал» на такое не рассчитано.
NON_TEXT_ARGUMENT = re.compile(
    r"\b(?:systemImage|systemName|image|imageName|key|forKey|withIdentifier|"
    r"identifier|tag|id|separator|encoding|ofType|named)\s*:\s*$"
)

# Ключ без единой буквы переводить нечего: `—`, `, `, `%@ → %@` — это
# разметка, а не текст. Требовать для них строку в каталоге значило бы
# засорять его и приучать проходить проверку не глядя.
LETTER = re.compile(r"[^\W\d_]", re.UNICODE)


def needs_catalog(key: str) -> bool:
    """Ключу нужен перевод, только если в нём есть буквы.

    Пустой ключ приходит из `Picker("", selection:)` — метка там скрыта
    `labelsHidden()`, а Xcode всё равно извлекает пустую строку. Ключи
    вроде `%@ → %@, %@` — разметка. Требовать для них перевода значило бы
    засорять каталог и приучать проходить проверку не глядя.
    """
    return bool(LETTER.search(INTERPOLATION.sub("", key.replace("%@", "").replace("%lld", ""))))


def scan() -> tuple[dict[str, list[str]], list[str], set[str], list[str]]:
    """Ключи, кириллица, сработавшие послабления и необёрнутые числа."""
    keys: dict[str, list[str]] = {}
    cyrillic_hits: list[str] = []
    used_allowances: set[str] = set()
    unwrapped: list[str] = []

    for path in swift_files():
        rel = relative(path)
        allowed = rel in CYRILLIC_ALLOWED
        previous = ""
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            line_allowed = LINE_ALLOW in line
            for start, literal in string_literals(line):
                if not literal:
                    continue
                kind = key_kind(line[:start], previous)
                if kind is not None:
                    keys.setdefault(catalog_key(literal), []).append(f"{rel}:{number}")
                    for expression in unwrapped_numbers(literal):
                        unwrapped.append(f"{rel}:{number}  {expression}")
                if CYRILLIC.search(literal):
                    if line_allowed:
                        continue
                    if allowed:
                        used_allowances.add(rel)
                        continue
                    where = kind or "строка"
                    cyrillic_hits.append(f"{rel}:{number}  {where}  «{literal}»")
            previous = line
    return keys, cyrillic_hits, used_allowances, unwrapped


def load_catalog() -> tuple[dict, str | None]:
    if not CATALOG.exists():
        return {}, f"каталога нет вовсе: {CATALOG.relative_to(ROOT)}"
    try:
        return json.loads(CATALOG.read_text(encoding="utf-8")), None
    except json.JSONDecodeError as error:
        return {}, f"каталог не разбирается как JSON: {error}"


def russian_translation(entry: dict) -> str | None:
    """Русский перевод ключа, или `None`, если его нет.

    Вариации по числу считаются переводом: у них строка лежит глубже, в
    `variations.plural.<форма>.stringUnit.value`.
    """
    ru = entry.get("localizations", {}).get("ru")
    if not ru:
        return None
    unit = ru.get("stringUnit")
    if unit and unit.get("value"):
        return unit["value"]
    plural = ru.get("variations", {}).get("plural")
    if plural:
        forms = [
            form.get("stringUnit", {}).get("value")
            for form in plural.values()
            if isinstance(form, dict)
        ]
        if any(forms):
            return " / ".join(value for value in forms if value)
    return None


def main() -> int:
    keys, cyrillic_hits, used_allowances, unwrapped = scan()
    catalog, catalog_error = load_catalog()
    failures: list[str] = []

    print("=== Прибор полноты локализации ===")
    print(f"  файлов Swift: {len(swift_files())}")
    print(f"  ключей в коде: {len(keys)}")

    # Проверка 1 — русского в интерфейсе нет.
    if cyrillic_hits:
        failures.append(f"кириллических литералов: {len(cyrillic_hits)}")
        print(f"\n[1] ПРОВАЛ — кириллических литералов вне белого списка: {len(cyrillic_hits)}")
        for hit in cyrillic_hits[:40]:
            print(f"      {hit}")
        if len(cyrillic_hits) > 40:
            print(f"      … ещё {len(cyrillic_hits) - 40}")
    else:
        print("\n[1] ок — кириллических литералов вне белого списка нет")

    # Послабление, которое ничего не исключает, — мёртвая строка, и она
    # опаснее отсутствующей: следующий читатель решит, что файл проверен.
    unused = sorted(CYRILLIC_ALLOWED - used_allowances)
    if unused:
        failures.append(f"мёртвых послаблений: {len(unused)}")
        print(f"    ПРОВАЛ — послабления, которые ничего не исключают: {len(unused)}")
        for entry in unused:
            print(f"      {entry}")

    if catalog_error:
        for index in (2, 3, 4):
            print(f"[{index}] ПРОВАЛ — {catalog_error}")
        failures.append(catalog_error)
        print("\nПровалов: " + str(len(failures)))
        return 1

    strings = catalog.get("strings", {})
    print(f"  ключей в каталоге: {len(strings)}")

    # Проверка 2 — каждый ключ из кода есть в каталоге.
    missing = sorted(key for key in keys if key not in strings and needs_catalog(key))
    if missing:
        failures.append(f"ключей нет в каталоге: {len(missing)}")
        print(f"\n[2] ПРОВАЛ — ключей из кода нет в каталоге: {len(missing)}")
        for key in missing[:40]:
            print(f"      «{key}»  ({keys[key][0]})")
        if len(missing) > 40:
            print(f"      … ещё {len(missing) - 40}")
    else:
        print("[2] ок — каждый ключ из кода есть в каталоге")

    # Проверка 3 — мёртвых ключей нет.
    dead = sorted(
        key
        for key in strings
        if key not in keys and key not in PLURAL_KEYS and needs_catalog(key)
    )
    if dead:
        failures.append(f"мёртвых ключей: {len(dead)}")
        print(f"\n[3] ПРОВАЛ — в каталоге ключи, которых нет в коде: {len(dead)}")
        for key in dead[:40]:
            print(f"      «{key}»")
        if len(dead) > 40:
            print(f"      … ещё {len(dead) - 40}")
    else:
        print("[3] ок — мёртвых ключей в каталоге нет")

    # Проверка 4 — у каждого ключа есть русский.
    untranslated = sorted(
        key
        for key, entry in strings.items()
        if russian_translation(entry) is None and needs_catalog(key)
    )
    if untranslated:
        failures.append(f"без русского перевода: {len(untranslated)}")
        print(f"\n[4] ПРОВАЛ — ключей без русского перевода: {len(untranslated)}")
        for key in untranslated[:40]:
            print(f"      «{key}»")
        if len(untranslated) > 40:
            print(f"      … ещё {len(untranslated) - 40}")
    else:
        print("[4] ок — у каждого ключа есть русский перевод")

    # Проверка 5 — числа подаются обёрнутыми, иначе спецификатор ключа
    # неизвестен и перевод недостижим.
    if unwrapped:
        failures.append(f"подстановок без типа: {len(unwrapped)}")
        print(f"\n[5] ПРОВАЛ — подстановок с неустановленным типом: {len(unwrapped)}")
        print("      UniFFI отдаёт u32 как UInt32, и его интерполяция даёт %u, а не %lld:")
        print("      ключ не совпадёт с каталогом, и перевод не найдётся никогда.")
        for hit in unwrapped[:20]:
            print(f"      {hit}")
        if len(unwrapped) > 20:
            print(f"      … ещё {len(unwrapped) - 20}")
    else:
        print("[5] ок — у каждой подстановки установлен тип")

    # Проверка 6 — тесты не утверждают про переведённый текст.
    test_hits = tests_asserting_translations(russian_values(strings))
    if test_hits:
        failures.append(f"тестов на переведённом тексте: {len(test_hits)}")
        print(f"\n[6] ПРОВАЛ — утверждений о переведённом тексте в тестах: {len(test_hits)}")
        for hit in test_hits[:25]:
            print(f"      {hit}")
        if len(test_hits) > 25:
            print(f"      … ещё {len(test_hits) - 25}")
    else:
        print("[6] ок — тесты не утверждают про переведённый текст")

    # Отдельным списком — ключи с подстановкой. Не провал: спецификатор
    # выведен догадкой по имени переменной, и подтвердить его может
    # только сборка на Маке.
    interpolated = sorted(key for key in keys if "%" in key)
    if interpolated:
        print(f"\n  ключей с подстановкой: {len(interpolated)}")
        for key in interpolated[:15]:
            print(f"      «{key}»")
        if len(interpolated) > 15:
            print(f"      … ещё {len(interpolated) - 15}")

    if failures:
        print("\nПровалов: " + str(len(failures)))
        for failure in failures:
            print(f"  · {failure}")
        return 1

    print("\nВсё сошлось. Компиляцию это не заменяет: синтаксис Swift здесь не проверяется.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
