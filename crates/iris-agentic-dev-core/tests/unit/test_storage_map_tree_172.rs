//! Storage map tree parser and emitter tests (T027-T031)

use iris_agentic_dev_core::tools::storage_map::tree::*;
use std::fs;

/// T027: Order key tests
#[test]
fn test_order_key_collation() {
    // Canonical numbers sort numerically
    assert!(order_key(Some("1")) < order_key(Some("2")));
    assert!(order_key(Some("2")) < order_key(Some("10")));
    assert!(order_key(Some("10")) < order_key(Some("11")));

    // Negative numbers
    assert!(order_key(Some("-1")) < order_key(Some("0")));

    // Numbers before strings
    assert!(order_key(Some("11")) < order_key(Some("A")));
    assert!(order_key(Some("11")) < order_key(Some("a")));

    // String sorts by code point
    assert!(order_key(Some("A")) < order_key(Some("a")));

    // Non-canonical numbers are strings
    assert!(order_key(Some("01")) > order_key(Some("11")));
    assert!(order_key(Some("1.0")) > order_key(Some("11")));
}

#[test]
fn test_is_canonical_number() {
    // Valid canonical numbers
    assert!(is_canonical_number("0"));
    assert!(is_canonical_number("1"));
    assert!(is_canonical_number("123"));
    assert!(is_canonical_number("-1"));
    assert!(is_canonical_number("1.5"));
    assert!(is_canonical_number(".5"));
    assert!(is_canonical_number("123.456"));

    // NOT canonical: leading zeros
    assert!(!is_canonical_number("01"));
    assert!(!is_canonical_number("001"));

    // NOT canonical: trailing zeros after decimal
    assert!(!is_canonical_number("1.0"));
    assert!(!is_canonical_number("1.50"));

    // NOT canonical: other formats
    assert!(!is_canonical_number(""));
    assert!(!is_canonical_number("abc"));
    assert!(!is_canonical_number("1."));
}

#[test]
fn test_sort_key_ordering() {
    let mut nodes = [
        Node {
            tag: "Data".to_string(),
            name: Some("City".to_string()),
            value: None,
            children: None,
        },
        Node {
            tag: "Data".to_string(),
            name: Some("Age".to_string()),
            value: None,
            children: None,
        },
        Node {
            tag: "Subscript".to_string(),
            name: Some("2".to_string()),
            value: None,
            children: None,
        },
        Node {
            tag: "Subscript".to_string(),
            name: Some("10".to_string()),
            value: None,
            children: None,
        },
        Node {
            tag: "Subscript".to_string(),
            name: Some("1".to_string()),
            value: None,
            children: None,
        },
    ];

    nodes.sort_by_key(sort_key);

    // Data sorts before Subscript (case-insensitive tag sort)
    assert_eq!(nodes[0].tag, "Data");
    assert_eq!(nodes[1].tag, "Data");

    // Within Data, Age < City (code point)
    assert_eq!(nodes[0].name.as_deref(), Some("Age"));
    assert_eq!(nodes[1].name.as_deref(), Some("City"));

    // Within Subscript, numeric order
    assert_eq!(nodes[2].name.as_deref(), Some("1"));
    assert_eq!(nodes[3].name.as_deref(), Some("2"));
    assert_eq!(nodes[4].name.as_deref(), Some("10"));
}

/// Helper to extract Storage body from fixture file
fn storage_body_from_file(filename: &str) -> String {
    let path = format!("tests/fixtures/storage172/classes/{}.cls", filename);
    let content = fs::read_to_string(&path).unwrap();

    // Find "\nStorage " (Storage at start of line) followed by a name and "{\n"
    let storage_marker = "\nStorage ";
    if let Some(storage_pos) = content.find(storage_marker) {
        // Skip the leading newline to get the actual Storage line start
        let storage_line_start = storage_pos + 1;

        // Find the opening brace after Storage name
        if let Some(brace_pos) = content[storage_line_start..].find("\n{\n") {
            let body_start = storage_line_start + brace_pos + 3; // Skip "\n{\n"

            // Find the closing "}\n" - use find to get the FIRST one (not the class closing brace)
            if let Some(end_pos) = content[body_start..].find("\n}") {
                return content[body_start..body_start + end_pos].to_string();
            }
        }
    }
    panic!("Could not extract Storage body from {}", path);
}

/// T028: parse_body tests
#[test]
fn test_parse_member_storage() {
    let body = storage_body_from_file("Member");
    let nodes = parse_body(&body).unwrap();

    // Should have 3 top-level elements: SqlIdExpression, SQLMap, StreamLocation, Type
    assert_eq!(nodes.len(), 4);

    // Check tags are present in sorted order
    let tags: Vec<&str> = nodes.iter().map(|n| n.tag.as_str()).collect();
    assert!(tags.contains(&"SqlIdExpression"));
    assert!(tags.contains(&"SQLMap"));
    assert!(tags.contains(&"StreamLocation"));
    assert!(tags.contains(&"Type"));
}

#[test]
fn test_parse_shelf_with_nextcode() {
    let body = storage_body_from_file("Shelf");
    let result = parse_body(&body);
    assert!(
        result.is_ok(),
        "Shelf with NextCode containing }} should parse"
    );
}

#[test]
fn test_parse_notes_with_cdata() {
    let body = storage_body_from_file("Notes");
    let nodes = parse_body(&body).unwrap();

    // Should parse Description and other elements
    assert!(nodes.iter().any(|n| n.tag == "Description"));
}

/// T029: emit_body tests (will test after implementing emit)
#[test]
fn test_emit_roundtrip_member() {
    let body = storage_body_from_file("Member");
    let nodes = parse_body(&body).unwrap();
    let emitted = emit_body(&nodes);
    assert_eq!(
        emitted, body,
        "Round-trip of Member should be byte-identical"
    );
}

/// T030: normalize tests (will implement after normalize function)
#[test]
fn test_normalize_notes() {
    // Notes has BlockCount 0 and ConditionalWithHostVars 0 that should be dropped
    let body = storage_body_from_file("Notes");
    let nodes = parse_body(&body).unwrap();
    let normalized = normalize(nodes);
    let emitted = emit_body(&normalized);

    // The canonical form should not have BlockCount or ConditionalWithHostVars set to 0
    assert!(!emitted.contains("<BlockCount>0</BlockCount>"));
    assert!(!emitted.contains("<ConditionalWithHostVars>0</ConditionalWithHostVars>"));
}

/// T031: split_class tests
#[test]
fn test_split_shelf_class() {
    let path = "tests/fixtures/storage172/classes/Shelf.cls";
    let content = fs::read_to_string(path).unwrap();

    let spans = split_class(&content).unwrap();
    assert_eq!(spans.len(), 1, "Shelf has one Storage block");

    let span = &spans[0];
    assert_eq!(span.name, "SQLStorage");

    // The body should contain NextCode with } lines
    let body = &content[span.body_start..span.body_end];
    assert!(body.contains("NextCode"), "Should extract NextCode");
    assert!(body.contains("}"), "NextCode contains }} lines");
}

// ── Lead's full-coverage cases (tasks T027–T031 as written) ─────────────────────────────────

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172")
}

fn every_fixture() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(fixture_dir().join("classes"))
        .expect("fixtures dir")
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            (
                p.file_stem().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read fixture"),
            )
        })
        .collect();
    out.sort();
    assert!(
        out.len() >= 14,
        "expected the 14 committed fixtures, found {}",
        out.len()
    );
    out
}

#[test]
fn every_fixture_storage_body_round_trips_byte_for_byte() {
    use iris_agentic_dev_core::tools::storage_map::tree::{emit_body, parse_body, split_class};
    for (name, text) in every_fixture() {
        let spans = split_class(&text).unwrap_or_else(|e| panic!("{name}: split: {e:?}"));
        assert_eq!(spans.len(), 1, "{name}: one Storage block");
        let s = &spans[0];
        let body = &text[s.body_start..s.body_end];
        let nodes = parse_body(body).unwrap_or_else(|e| panic!("{name}: parse: {e:?}"));
        assert_eq!(
            emit_body(&nodes),
            body,
            "{name}: emit differs from the stored body"
        );
    }
}

#[test]
fn numeric_names_sort_as_numbers_before_strings() {
    use iris_agentic_dev_core::tools::storage_map::tree::order_key;
    let mut names = vec!["10", "2", "1", "11", "a", "A", "01", "-1", "0", "1.5"];
    names.sort_by_key(|a| order_key(Some(a)));
    assert_eq!(
        names,
        vec!["-1", "0", "1", "1.5", "2", "10", "11", "01", "A", "a"]
    );
}

#[test]
fn every_malformed_body_is_refused_quickly_with_its_reason() {
    use iris_agentic_dev_core::tools::storage_map::tree::parse_body;
    let dir = fixture_dir().join("malformed");
    let mut seen = 0;
    for e in std::fs::read_dir(&dir).expect("malformed dir") {
        let p = e.expect("entry").path();
        let text = std::fs::read_to_string(&p).expect("read");
        let (first, body) = text.split_once('\n').unwrap_or((&text, ""));
        let expect = first
            .strip_prefix("# expect:")
            .unwrap_or_else(|| panic!("{}: first line must be '# expect: …'", p.display()))
            .trim()
            .to_lowercase();
        let t0 = std::time::Instant::now();
        let err = parse_body(body).expect_err(&format!("{} must be refused", p.display()));
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(100),
            "{}: took {:?}",
            p.display(),
            t0.elapsed()
        );
        let msg = format!("{} {}", err.reason, err.context).to_lowercase();
        assert!(
            msg.contains(&expect),
            "{}: expected '{expect}' in '{msg}'",
            p.display()
        );
        seen += 1;
    }
    assert!(seen >= 8, "expected the 8 malformed files, found {seen}");
}
