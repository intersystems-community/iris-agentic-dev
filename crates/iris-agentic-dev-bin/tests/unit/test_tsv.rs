use iris_agentic_dev::cmd::tsv::{rows_to_tsv, tsv_header};

#[test]
fn test_header_single_column() {
    let h = tsv_header(&["Name"]);
    assert_eq!(h, "Name");
}

#[test]
fn test_header_multi_column() {
    let h = tsv_header(&["Name", "Age", "City"]);
    assert_eq!(h, "Name\tAge\tCity");
}

#[test]
fn test_header_empty() {
    let h = tsv_header(&[]);
    assert_eq!(h, "");
}

#[test]
fn test_rows_to_tsv_single_column() {
    let rows = vec![vec!["Alice".to_string()], vec!["Bob".to_string()]];
    let out = rows_to_tsv(&rows);
    assert_eq!(out, "Alice\nBob\n");
}

#[test]
fn test_rows_to_tsv_multi_column() {
    let rows = vec![
        vec!["Alice".to_string(), "30".to_string(), "Boston".to_string()],
        vec!["Bob".to_string(), "25".to_string(), "NYC".to_string()],
    ];
    let out = rows_to_tsv(&rows);
    assert_eq!(out, "Alice\t30\tBoston\nBob\t25\tNYC\n");
}

#[test]
fn test_rows_to_tsv_zero_rows() {
    let rows: Vec<Vec<String>> = vec![];
    let out = rows_to_tsv(&rows);
    assert_eq!(out, "");
}

#[test]
fn test_rows_to_tsv_embedded_tab_escaped() {
    // An embedded tab in a cell value must be escaped as literal \t
    let rows = vec![vec!["hello\tworld".to_string()]];
    let out = rows_to_tsv(&rows);
    assert_eq!(out, "hello\\tworld\n");
}

#[test]
fn test_rows_to_tsv_embedded_newline_escaped() {
    let rows = vec![vec!["line1\nline2".to_string()]];
    let out = rows_to_tsv(&rows);
    assert_eq!(out, "line1\\nline2\n");
}

#[test]
fn test_rows_to_tsv_no_padding_or_alignment() {
    // Values of different lengths should not be padded
    let rows = vec![
        vec!["short".to_string(), "a much longer value".to_string()],
        vec!["x".to_string(), "y".to_string()],
    ];
    let out = rows_to_tsv(&rows);
    // No extra spaces around values
    assert_eq!(out, "short\ta much longer value\nx\ty\n");
}

#[test]
fn test_tsv_header_no_tabs_in_column_names() {
    // Column name with tab — escape it
    let h = tsv_header(&["Col\tName"]);
    assert_eq!(h, "Col\\tName");
}

/// Spec 141 C4: data from IRIS can carry terminal escape sequences (a window title, an OSC 52
/// clipboard write), so no control byte, DEL or C1 character reaches the output.
#[test]
fn escape_cell_leaves_no_control_byte() {
    let samples = [
        "\r",
        "\x1b[2J",
        "a\\tb",
        "\0",
        "\x7f",
        "\u{9b}",
        "\u{7}]52;c;aGk=\u{7}",
        "a\r\nb",
    ];
    for cell in samples {
        let line = rows_to_tsv(&[vec![cell.to_string()]]);
        let body = line.strip_suffix('\n').expect("row terminator");
        assert!(
            body.chars()
                .all(|c| !(c < ' ' || c == '\x7f' || ('\u{80}'..='\u{9f}').contains(&c))),
            "control character left in {cell:?}: {body:?}"
        );
    }
}

/// The backslash is escaped first, so a literal `\t` in a cell cannot be read as a tab.
#[test]
fn escaped_backslash_is_not_a_tab() {
    assert_eq!(rows_to_tsv(&[vec!["a\\tb".to_string()]]), "a\\\\tb\n");
    assert_eq!(rows_to_tsv(&[vec!["\t".to_string()]]), "\\t\n");
}

/// Reverses `escape_cell`: `\\`, `\t`, `\n`, `\r` and `\xNN`. Test-only, so the encoding is checked
/// by parsing it back rather than by comparing it with a copy of itself.
fn unescape_cell(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars
            .next()
            .expect("a backslash is always followed by an escape")
        {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                let code = u32::from_str_radix(&hex, 16).expect("two hex digits");
                out.push(char::from_u32(code).expect("a scalar value"));
            }
            other => panic!("unknown escape \\{other}"),
        }
    }
    out
}

/// Spec 141 C4: every C0 control byte, the backslash and the tab survive a write and a parse back,
/// alone and between other characters. A cell that did not come back unchanged would make the
/// output ambiguous, and a tab that was not escaped would split one cell into two.
#[test]
fn tsv_cell_round_trips_every_control_byte_backslash_and_tab() {
    let mut specials: Vec<char> = (0x00u32..=0x1f).filter_map(char::from_u32).collect();
    specials.push('\\');
    specials.push('\t');
    for c in specials {
        for cell in [
            c.to_string(),
            format!("a{c}b"),
            format!("{c}\\{c}"),
            format!("\\x{:02x}{c}", c as u32),
        ] {
            let line = rows_to_tsv(&[vec![cell.clone(), "tail".to_string()]]);
            let fields: Vec<&str> = line
                .strip_suffix('\n')
                .expect("row terminator")
                .split('\t')
                .collect();
            assert_eq!(fields.len(), 2, "one cell became two fields: {line:?}");
            assert_eq!(unescape_cell(fields[0]), cell, "cell {cell:?} as {line:?}");
            assert_eq!(fields[1], "tail");
        }
    }
}
