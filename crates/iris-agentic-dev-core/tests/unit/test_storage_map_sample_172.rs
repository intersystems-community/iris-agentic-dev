//! T082: the sampler's ObjectScript holds no caller text beyond a validated global name
//! (FR-030b), and its output reads back into a shape report (data-model.md "Shape report").

use iris_agentic_dev_core::tools::storage_map::globals::GlobalRef;
use iris_agentic_dev_core::tools::storage_map::sample::{build_code, parse};

fn global(name: &str) -> GlobalRef {
    GlobalRef {
        name: name.to_string(),
        extended: None,
        subscripts_prefix: None,
    }
}

#[test]
fn test_storage_map_sample_172_code_names_only_the_global() {
    let code = build_code(&global("^IadSm172Member"), 200, 5, false).expect("valid name");
    assert_eq!(code.matches("IadSm172Member").count(), 1, "{code}");
    assert!(code.contains("\"^IadSm172Member\""), "{code}");
    assert!(
        code.contains("max=200") && code.contains("secs=5") && code.contains("vals=0"),
        "{code}"
    );
    let with_values = build_code(&global("^IadSm172Member"), 10, 2, true).unwrap();
    assert!(with_values.contains("vals=1"), "{with_values}");
}

#[test]
fn test_storage_map_sample_172_bad_names_are_refused_before_any_code() {
    for bad in ["^X)", "^X\"", "^X(1)", "X\nKill ^Y", "^", "^1X", "^X..Y"] {
        assert!(build_code(&global(bad), 200, 5, false).is_err(), "{bad:?}");
    }
    let mut ext = global("^X");
    ext.extended = Some("USER".into());
    assert!(
        build_code(&ext, 200, 5, false).is_err(),
        "extended reference"
    );
    let mut sub = global("^X");
    sub.subscripts_prefix = Some("(1)".into());
    assert!(build_code(&sub, 200, 5, false).is_err(), "subscript prefix");
    // `X` without the caret is the same global.
    assert!(build_code(&global("IadSm172Member"), 200, 5, false).is_ok());
}

/// Lines the sampler writes for `Member`'s seed: the root holds the ID counter `3`, and rows 1 to
/// 3 hold `name^city^age`.
const MEMBER_LINES: &str = r#"{"d":0,"s":11,"f":"plain","p":1}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"done":1,"c":4,"capped":0}
"#;

#[test]
fn test_storage_map_sample_172_parse_reads_the_seeded_shape() {
    let r = parse(MEMBER_LINES).expect("parse");
    assert_eq!(r.nodes_scanned, 4);
    assert!(!r.capped);
    assert_eq!(r.depths.get("0"), Some(&1));
    assert_eq!(r.depths.get("1"), Some(&3));
    assert_eq!(r.data_states["1"].get("1"), Some(&3));
    assert_eq!(r.data_states["0"].get("11"), Some(&1));
    let row = r
        .formats
        .iter()
        .find(|f| f.depth == 1)
        .expect("a depth 1 format");
    assert_eq!(row.format, "delimited");
    assert_eq!(row.delimiter.as_deref(), Some("^"));
    assert_eq!((row.pieces.min, row.pieces.max), (3, 3));
    assert!(row.node.is_none());
    assert!(r.values.is_none());
}

#[test]
fn test_storage_map_sample_172_parse_reads_sub_nodes_and_lists() {
    let lines = r#"{"d":1,"s":11,"f":"$LIST","p":2}
{"d":2,"s":1,"n":"contact","f":"delimited","dl":124,"p":2}
{"d":1,"s":10}
{"d":2,"s":1,"n":"contact","f":"delimited","dl":124,"p":3}
{"done":1,"c":4,"capped":1}
"#;
    let r = parse(lines).unwrap();
    assert!(r.capped);
    assert_eq!(r.data_states["1"].get("11"), Some(&1));
    assert_eq!(r.data_states["1"].get("10"), Some(&1));
    assert_eq!(r.sub_nodes["2"], vec!["contact".to_string()]);
    let contact = r
        .formats
        .iter()
        .find(|f| f.node.as_deref() == Some("\"contact\""))
        .expect("the contact node");
    assert_eq!(contact.delimiter.as_deref(), Some("|"));
    assert_eq!((contact.pieces.min, contact.pieces.max), (2, 3));
    assert!(r
        .formats
        .iter()
        .any(|f| f.depth == 1 && f.format == "$LIST"));
}

#[test]
fn test_storage_map_sample_172_parse_refuses_incomplete_output() {
    assert!(parse("{\"d\":1,\"s\":1}\n").is_err(), "no done line");
    assert!(parse("<UNDEFINED>x+3^Y\n").is_err(), "an IRIS error");
}
