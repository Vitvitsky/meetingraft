#!/usr/bin/env python3
"""Прибор полноты локализации (подпроект 1b).

Swift на Linux-машине не собирается, но каталог и исходники — текст, и
полноту перевода можно проверить разбором, а не обещанием. Прибор
проверяет четыре вещи:

1. ни одного кириллического строкового литерала в `Sources/`, кроме
   явного белого списка;
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
# Имена и обёртки, по которым подстановка считается числовой.
#
# Тип из текста не выводится, и это лучшая доступная догадка. Цена
# ошибки известна и невелика: ключ не совпадёт с каталогом, и строка
# покажется по-английски. Несовпадение Xcode назовёт при сборке.
NUMERIC_HINTS = re.compile(
    r"\b(?:Int|count|number|samples|seconds|months|statusCode|version|prints|"
    r"signed|cleared|unknown|imported|skipped|deletedCount|labelled|candidates|"
    r"withoutVector|signedFromMemory|sourceVersion|unidentifiedSegments)\b"
)


def catalog_key(literal: str) -> str:
    r"""Литерал в том виде, в каком ключом его увидит Xcode.

    `Text("Meeting \(stamp)")` даёт ключ `Meeting %@`, а не текст
    исходника. Без этой нормализации проверка полноты каталога врала бы
    ровно на строках с подстановкой — там, где ошибиться проще всего.
    """
    return INTERPOLATION.sub(
        lambda match: "%lld" if NUMERIC_HINTS.search(match.group(0)) else "%@",
        literal,
    )


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
    return bool(LETTER.search(INTERPOLATION.sub("", key.replace("%@", "").replace("%lld", ""))))


def scan() -> tuple[dict[str, list[str]], list[str], set[str]]:
    """Ключи из кода, кириллические литералы и сработавшие послабления."""
    keys: dict[str, list[str]] = {}
    cyrillic_hits: list[str] = []
    used_allowances: set[str] = set()

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
                if CYRILLIC.search(literal):
                    if line_allowed:
                        continue
                    if allowed:
                        used_allowances.add(rel)
                        continue
                    where = kind or "строка"
                    cyrillic_hits.append(f"{rel}:{number}  {where}  «{literal}»")
            previous = line
    return keys, cyrillic_hits, used_allowances


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
    keys, cyrillic_hits, used_allowances = scan()
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
    dead = sorted(key for key in strings if key not in keys and key not in PLURAL_KEYS)
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
        key for key, entry in strings.items() if russian_translation(entry) is None
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

    # Отдельным списком — ключи с подстановкой. Не провал: спецификатор
    # выведен догадкой по имени переменной, и подтвердить его может
    # только сборка на Маке.
    interpolated = sorted(key for key in keys if "%" in key)
    if interpolated:
        print(f"\n  ключей с подстановкой: {len(interpolated)} — спецификатор проверяется сборкой")
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
