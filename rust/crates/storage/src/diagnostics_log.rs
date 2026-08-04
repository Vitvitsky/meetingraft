//! Локальный журнал диагностики распознавания (JSONL).
//!
//! Зачем он есть: отсев галлюцинаций (Epic 16) выбрасывает текст молча.
//! Если под нож попала настоящая речь, узнать об этом сегодня неоткуда —
//! ни пользователю, ни разработчику. Фильтр без журнала это доверие без
//! проверки.
//!
//! Чего он **не** делает: никуда не отправляется. Файл лежит рядом с
//! записями встречи, в том же каталоге данных, и уходит куда-либо только
//! если человек сам его отдаст. Телеметрия противоречила бы тому
//! единственному, что продукт обещает.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use domain::SttDiagnostic;

/// Потолок файла. Дальше журнал сдвигается: свежие записи важнее.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Сколько записей остаётся после сдвига.
const KEEP_LINES: usize = 2_000;

/// Дописывающий журнал в каталоге данных.
pub struct DiagnosticsLog {
    path: PathBuf,
    enabled: bool,
    max_bytes: u64,
    keep_lines: usize,
}

impl DiagnosticsLog {
    /// Журнал в `<data_root>/diagnostics.jsonl`.
    pub fn new(data_root: &Path, enabled: bool) -> Self {
        Self {
            path: data_root.join("diagnostics.jsonl"),
            enabled,
            max_bytes: MAX_BYTES,
            keep_lines: KEEP_LINES,
        }
    }

    /// Тот же журнал с маленькими порогами — только для тестов ротации:
    /// на боевых порогах она проверяется десятками тысяч записей.
    #[cfg(test)]
    fn with_limits(data_root: &Path, max_bytes: u64, keep_lines: usize) -> Self {
        Self {
            max_bytes,
            keep_lines,
            ..Self::new(data_root, true)
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Дописать записи. Ошибки записи глотаются намеренно: журнал —
    /// вспомогательный, и уронить из-за него запись встречи нельзя.
    pub fn append(&self, records: &[SttDiagnostic], at_ms: u64) {
        if !self.enabled || records.is_empty() {
            return;
        }
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        for record in records {
            let line = format!(
                "{{\"ts_ms\":{},\"kind\":\"{}\",\"buffer_ms\":{},\"text\":\"{}\"}}\n",
                at_ms,
                record.kind.code(),
                record.buffer_ms,
                escape(&record.text)
            );
            let _ = file.write_all(line.as_bytes());
        }
        let _ = file.flush();
        self.rotate_if_large();
    }

    /// Стереть журнал целиком — он содержит текст встреч.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }

    pub fn size_bytes(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }

    /// Сдвинуть журнал, оставив хвост.
    ///
    /// Именно хвост, а не начало: разбирают всегда свежую жалобу.
    fn rotate_if_large(&self) {
        if self.size_bytes() <= self.max_bytes {
            return;
        }
        let Ok(file) = File::open(&self.path) else {
            return;
        };
        let lines: Vec<String> = BufReader::new(file).lines().map_while(Result::ok).collect();
        let start = lines.len().saturating_sub(self.keep_lines);
        let Ok(mut out) = File::create(&self.path) else {
            return;
        };
        for line in &lines[start..] {
            let _ = writeln!(out, "{line}");
        }
    }
}

/// Экранирование для JSON-строки без внешней зависимости.
///
/// Управляющие символы вырезаются: в тексте реплики их быть не должно, а
/// сломанная строка испортила бы весь файл для любого парсера.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::SttDiagnosticKind;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mr-diag-{name}-{:?}-{}",
            std::thread::current().id(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        root
    }

    fn record(text: &str) -> SttDiagnostic {
        SttDiagnostic::new(SttDiagnosticKind::DroppedHallucination, text, 1_200)
    }

    #[test]
    fn appends_one_line_per_record() {
        let root = temp_root("append");
        let log = DiagnosticsLog::new(&root, true);

        log.append(&[record("Субтитры сделал DimaTorzok")], 100);
        log.append(&[record("Спасибо за просмотр")], 200);

        let body = std::fs::read_to_string(log.path()).expect("read");
        assert_eq!(body.lines().count(), 2);
        assert!(body.contains("dropped_hallucination"));
        assert!(body.contains("DimaTorzok"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Выключенный журнал не должен даже создавать файл: иначе «выключено»
    /// означало бы «пишем, но не показываем».
    #[test]
    fn disabled_log_writes_nothing() {
        let root = temp_root("disabled");
        let log = DiagnosticsLog::new(&root, false);

        log.append(&[record("текст")], 100);

        assert!(!log.path().exists());
        assert_eq!(log.size_bytes(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Кавычки и переводы строк не должны ломать файл для парсера.
    #[test]
    fn quotes_and_newlines_stay_inside_one_line() {
        let root = temp_root("escape");
        let log = DiagnosticsLog::new(&root, true);

        log.append(&[record("он сказал \"да\"\nи ушёл")], 1);

        let body = std::fs::read_to_string(log.path()).expect("read");
        assert_eq!(body.lines().count(), 1);
        assert!(body.contains("\\\"да\\\""));
        assert!(body.contains("\\n"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_removes_the_file() {
        let root = temp_root("clear");
        let log = DiagnosticsLog::new(&root, true);
        log.append(&[record("текст")], 1);

        log.clear();

        assert!(!log.path().exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Журнал не должен расти без предела, и оставаться должен хвост:
    /// разбирают всегда свежую жалобу.
    #[test]
    fn rotation_keeps_the_tail() {
        let root = temp_root("rotate");
        let log = DiagnosticsLog::with_limits(&root, 4_096, 10);

        for index in 0..200 {
            log.append(&[record(&format!("запись {index}"))], index as u64);
        }

        // Ротация срабатывает по превышению потолка, поэтому файл живёт
        // между «хвост» и «потолок» — важно, что он не растёт дальше.
        assert!(
            log.size_bytes() <= 4_096 + 256,
            "журнал вырос: {} байт",
            log.size_bytes()
        );
        let body = std::fs::read_to_string(log.path()).expect("read");
        let lines: Vec<&str> = body.lines().collect();
        assert!(lines.len() < 200, "ротации не было: {}", lines.len());
        assert!(
            lines.last().expect("last").contains("запись 199"),
            "последняя запись должна остаться"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
