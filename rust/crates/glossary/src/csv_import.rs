//! Импорт терминов из CSV-контракта Glossary MVP.

use domain::{GlossaryKind, GlossaryScope, GlossaryTerm, SpeechLanguage};
use uuid::Uuid;

const REQUIRED_COLUMNS: [&str; 4] = ["surface", "canonical", "language", "scope"];

/// Разбирает CSV, пропуская некорректные строки и возвращая их количество.
pub fn parse_csv(csv: &str) -> Result<(Vec<GlossaryTerm>, u32), String> {
    let mut lines = csv.lines();
    let header = lines
        .next()
        .ok_or_else(|| "CSV не содержит заголовок".to_owned())?;
    let columns = parse_record(header).ok_or_else(|| "Некорректный заголовок CSV".to_owned())?;
    validate_header(&columns)?;

    let meeting_id_index = columns.iter().position(|column| column == "meeting_id");
    let mut terms = Vec::new();
    let mut skipped = 0_u32;

    for line in lines.filter(|line| !line.trim().is_empty()) {
        let parsed = parse_record(line).and_then(|values| parse_row(&values, meeting_id_index));
        match parsed {
            Some(term) => terms.push(term),
            None => skipped += 1,
        }
    }

    Ok((terms, skipped))
}

fn parse_record(record: &str) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut value = String::new();
    let mut characters = record.chars().peekable();
    let mut quoted = false;
    let mut quote_closed = false;

    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' {
                if characters.peek() == Some(&'"') {
                    value.push('"');
                    characters.next();
                } else {
                    quoted = false;
                    quote_closed = true;
                }
            } else {
                value.push(character);
            }
            continue;
        }

        match character {
            ',' => {
                values.push(value.trim().to_owned());
                value.clear();
                quote_closed = false;
            }
            '"' if value.trim().is_empty() && !quote_closed => {
                value.clear();
                quoted = true;
            }
            character if quote_closed && character.is_whitespace() => {}
            _ if quote_closed => return None,
            _ => value.push(character),
        }
    }

    if quoted {
        return None;
    }
    values.push(value.trim().to_owned());
    Some(values)
}

fn validate_header(columns: &[String]) -> Result<(), String> {
    if columns.len() < REQUIRED_COLUMNS.len()
        || columns[..REQUIRED_COLUMNS.len()]
            .iter()
            .map(String::as_str)
            .ne(REQUIRED_COLUMNS)
    {
        return Err("Ожидается заголовок surface,canonical,language,scope".to_owned());
    }
    Ok(())
}

fn parse_row(values: &[String], meeting_id_index: Option<usize>) -> Option<GlossaryTerm> {
    if values.len() < REQUIRED_COLUMNS.len() {
        return None;
    }

    let surface = values[0].as_str();
    let canonical = values[1].as_str();
    if surface.is_empty() || canonical.is_empty() {
        return None;
    }

    let language = parse_language(&values[2])?;
    let scope = parse_scope(
        &values[3],
        meeting_id_index
            .and_then(|index| values.get(index))
            .map(String::as_str),
    )?;

    Some(GlossaryTerm {
        id: Uuid::new_v4().to_string(),
        surface: surface.to_owned(),
        canonical: canonical.to_owned(),
        language,
        scope,
        // Импортированные термины остаются заменами: человек привёз
        // готовый словарь замен, менять его смысл импортом нельзя.
        kind: GlossaryKind::Replacement,
    })
}

fn parse_language(value: &str) -> Option<SpeechLanguage> {
    match value {
        "" | "ru" => Some(SpeechLanguage::Ru),
        "en" => Some(SpeechLanguage::En),
        "es" => Some(SpeechLanguage::Es),
        _ => None,
    }
}

fn parse_scope(value: &str, meeting_id: Option<&str>) -> Option<GlossaryScope> {
    match value {
        "" | "global" => Some(GlossaryScope::Global),
        "meeting" => {
            let meeting_id = meeting_id?.trim();
            if meeting_id.is_empty() {
                return None;
            }
            Some(GlossaryScope::Meeting {
                meeting_id: meeting_id.to_owned(),
            })
        }
        _ => None,
    }
}
