//! Импорт терминов из CSV-контракта Glossary MVP.

use domain::{GlossaryScope, GlossaryTerm, SpeechLanguage};
use uuid::Uuid;

const REQUIRED_COLUMNS: [&str; 4] = ["surface", "canonical", "language", "scope"];

/// Разбирает CSV, пропуская некорректные строки и возвращая их количество.
pub fn parse_csv(csv: &str) -> Result<(Vec<GlossaryTerm>, u32), String> {
    let mut lines = csv.lines();
    let header = lines
        .next()
        .ok_or_else(|| "CSV не содержит заголовок".to_owned())?;
    let columns: Vec<&str> = header.split(',').map(str::trim).collect();
    validate_header(&columns)?;

    let meeting_id_index = columns.iter().position(|column| *column == "meeting_id");
    let mut terms = Vec::new();
    let mut skipped = 0_u32;

    for line in lines.filter(|line| !line.trim().is_empty()) {
        let values: Vec<&str> = line.split(',').map(str::trim).collect();
        match parse_row(&values, meeting_id_index) {
            Some(term) => terms.push(term),
            None => skipped += 1,
        }
    }

    Ok((terms, skipped))
}

fn validate_header(columns: &[&str]) -> Result<(), String> {
    if columns.len() < REQUIRED_COLUMNS.len()
        || columns[..REQUIRED_COLUMNS.len()] != REQUIRED_COLUMNS
    {
        return Err("Ожидается заголовок surface,canonical,language,scope".to_owned());
    }
    Ok(())
}

fn parse_row(values: &[&str], meeting_id_index: Option<usize>) -> Option<GlossaryTerm> {
    if values.len() < REQUIRED_COLUMNS.len() {
        return None;
    }

    let surface = values[0];
    let canonical = values[1];
    if surface.is_empty() || canonical.is_empty() {
        return None;
    }

    let language = parse_language(values[2])?;
    let scope = parse_scope(
        values[3],
        meeting_id_index.and_then(|index| values.get(index).copied()),
    )?;

    Some(GlossaryTerm {
        id: Uuid::new_v4().to_string(),
        surface: surface.to_owned(),
        canonical: canonical.to_owned(),
        language,
        scope,
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
