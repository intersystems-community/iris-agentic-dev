/// Format column headers as a tab-separated line.
/// Embedded tabs in column names are escaped as `\t`.
pub fn tsv_header(columns: &[&str]) -> String {
    columns
        .iter()
        .map(|c| escape_cell(c))
        .collect::<Vec<_>>()
        .join("\t")
}

/// Format rows as tab-separated lines, one row per line.
/// Embedded tabs and newlines in cell values are escaped.
pub fn rows_to_tsv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let line = row
            .iter()
            .map(|cell| escape_cell(cell))
            .collect::<Vec<_>>()
            .join("\t");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The backslash is escaped first, so the encoding can be reversed. Control characters, DEL and C1
/// characters become `\xNN`, so IRIS data cannot write a terminal escape sequence (spec 141 C4).
fn escape_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c < ' ' || c == '\x7f' || ('\u{80}'..='\u{9f}').contains(&c) => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Extract column names from an Atelier query result body.
/// Atelier /action/query returns `result.content` as an array of row-objects
/// (each row is a JSON object with column names as keys). Column order is taken
/// from the first row's key insertion order.
pub fn extract_columns(body: &serde_json::Value) -> Vec<String> {
    body["result"]["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|first| first.as_object())
        .map(|obj| obj.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default()
}

/// Extract data rows from an Atelier query result body.
/// Rows are in `result.content`, each being a JSON object with column-name keys.
/// Column order follows `extract_columns` (key insertion order of the first row).
pub fn extract_rows(body: &serde_json::Value) -> Vec<Vec<String>> {
    let content = match body["result"]["content"].as_array() {
        Some(c) if !c.is_empty() => c,
        _ => return vec![],
    };
    // Derive column order from first row
    let cols: Vec<String> = content
        .first()
        .and_then(|r| r.as_object())
        .map(|obj| obj.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();

    content
        .iter()
        .filter_map(|row| row.as_object())
        .map(|obj| {
            cols.iter()
                .map(|col| match obj.get(col) {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                })
                .collect()
        })
        .collect()
}
