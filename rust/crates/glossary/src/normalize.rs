//! Замена surface-форм с сохранением canonical-регистра.

use domain::{GlossaryKind, GlossaryTerm};

#[allow(dead_code)]
pub(crate) fn normalize(text: &str, terms: &[GlossaryTerm]) -> String {
    normalize_with_kind(text, terms, None)
}

/// Заменяет surface-фразы только для термина указанного вида.
///
/// Если `kind_filter` это `Some(kind)`, применяются только термины этого вида.
/// Если `None`, применяются все термины.
pub(crate) fn normalize_with_kind(
    text: &str,
    terms: &[GlossaryTerm],
    kind_filter: Option<GlossaryKind>,
) -> String {
    if terms.is_empty() {
        return text.to_owned();
    }

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;

    while cursor < text.len() {
        if let Some((end, canonical)) = terms.iter().find_map(|term| {
            if let Some(kind) = kind_filter
                && term.kind != kind
            {
                return None;
            }
            match_end(text, cursor, &term.surface).map(|end| (end, &term.canonical))
        }) {
            output.push_str(canonical);
            cursor = end;
        } else {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("курсор всегда указывает внутрь строки");
            output.push(character);
            cursor += character.len_utf8();
        }
    }

    output
}

fn match_end(text: &str, start: usize, surface: &str) -> Option<usize> {
    if surface.is_empty() || !is_left_boundary(text, start) {
        return None;
    }

    let end = consume_characters(text, start, surface.chars().count())?;
    let candidate = &text[start..end];
    if lowercase(candidate) != lowercase(surface) || !is_right_boundary(text, end) {
        return None;
    }

    Some(end)
}

fn consume_characters(text: &str, start: usize, count: usize) -> Option<usize> {
    let mut end = start;
    let mut characters = text[start..].chars();
    for _ in 0..count {
        end += characters.next()?.len_utf8();
    }
    Some(end)
}

fn is_left_boundary(text: &str, position: usize) -> bool {
    text[..position]
        .chars()
        .next_back()
        .is_none_or(is_separator)
}

fn is_right_boundary(text: &str, position: usize) -> bool {
    text[position..].chars().next().is_none_or(is_separator)
}

fn is_separator(character: char) -> bool {
    !character.is_ascii_alphabetic() && !is_cyrillic(character)
}

fn is_cyrillic(character: char) -> bool {
    matches!(character, '\u{0400}'..='\u{052f}' | '\u{2de0}'..='\u{2dff}' | '\u{a640}'..='\u{a69f}')
}

fn lowercase(value: &str) -> String {
    value.chars().flat_map(char::to_lowercase).collect()
}
