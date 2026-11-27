//! Spec 173, T073 to T075 (Benjamin De Boe's feedback on the pre-release, 2026-11-26), pure logic.
//! Each test names the known answer, built from the fixtures' own text.

use iris_agentic_dev_core::tools::storage_map::model::{
    derived_facts, from_tree, mark_compiler_added, StorageModel,
};
use iris_agentic_dev_core::tools::storage_map::notices::{
    computed_warnings, no_data_fields, read_storage, with_warning_lines,
};
use iris_agentic_dev_core::tools::storage_map::shape::ShapeReport;
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
use serde_json::json;

fn model_from(name: &str, text: &str) -> StorageModel {
    let span = split_class(text).unwrap().remove(0);
    let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
    let mut m = from_tree(&format!("IadSm172.{name}"), &span.name, &nodes);
    m.derived = derived_facts(&m);
    mark_compiler_added(&mut m, text);
    m
}

fn fixture_text(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    std::fs::read_to_string(p).unwrap()
}

// ── T073: StreamLocation is left out of what `read` returns ────────────────

#[test]
fn read_storage_leaves_out_a_compiler_added_stream_location_and_keeps_the_rest() {
    let mut m = model_from("Bits", &fixture_text("Bits"));
    assert_eq!(m.compiler_added, vec!["StreamLocation".to_string()]);
    m.storage
        .keywords
        .insert("SqlRowIdName".into(), "ID".into());
    let shown = read_storage(&m);
    let keywords = shown["keywords"].as_object().unwrap();
    assert!(!keywords.contains_key("StreamLocation"), "{shown}");
    assert_eq!(keywords.get("SqlRowIdName"), Some(&json!("ID")), "{shown}");
    assert_eq!(shown["maps"][0]["name"], "Chunks", "{shown}");
    assert_eq!(shown["version"], json!(m.storage.version), "{shown}");
    // A presentation filter only: the model that `edit` and `replace` write from is untouched.
    assert!(m.storage.keywords.contains_key("StreamLocation"));
}

#[test]
fn read_storage_keeps_the_element_when_a_property_is_a_stream() {
    let text = fixture_text("Bits").replace(
        "Property Word As %Integer;",
        "Property Word As %Integer;\n\nProperty Body As %Stream.GlobalCharacter;",
    );
    let m = model_from("Bits", &text);
    assert!(m.compiler_added.is_empty());
    let shown = read_storage(&m);
    assert_eq!(
        shown["keywords"]["StreamLocation"], "^IadSm172BitsS",
        "{shown}"
    );
    // Nothing marked: the storage JSON as it was.
    assert_eq!(shown, json!(m.storage));
}

// ── T074: one warning per computed level and per access variable ───────────

#[test]
fn computed_warnings_name_the_level_the_signs_and_the_way_to_confirm_for_the_bitmap_map() {
    let m = model_from("Bits", &fixture_text("Bits"));
    let w = computed_warnings(&m);
    assert_eq!(w.len(), 2, "{w:?}");
    let level = w.iter().find(|l| l.contains("level 3")).unwrap();
    for want in [
        "map Chunks",
        "AccessType Other",
        "NextCode",
        "iad does not evaluate",
        "may not be a subscript of the global",
        "$BIT",
        "`compare`",
        "samples the real global",
    ] {
        assert!(level.contains(want), "{want:?} missing from {level}");
    }
    let var = w.iter().find(|l| l.contains("access variable")).unwrap();
    for want in [
        "map Chunks",
        "level 2",
        "2D2",
        "set by code",
        "iad does not evaluate",
        "`compare`",
    ] {
        assert!(var.contains(want), "{want:?} missing from {var}");
    }
}

#[test]
fn computed_warnings_call_an_invalidcondition_alone_a_filter_not_a_computed_level() {
    let m = model_from("Route", &fixture_text("Route"));
    let w = computed_warnings(&m);
    assert_eq!(w.len(), 2, "{w:?}");
    let filter = w.iter().find(|l| l.contains("level 2")).unwrap();
    assert!(filter.contains("Invalidcondition"), "{filter}");
    assert!(filter.contains("filter"), "{filter}");
    assert!(!filter.contains("may not be a subscript"), "{filter}");
    assert!(w
        .iter()
        .any(|l| l.contains("level 1") && l.contains("access variable")));
}

#[test]
fn computed_warnings_of_a_plain_map_are_empty() {
    assert!(computed_warnings(&model_from("Member", &fixture_text("Member"))).is_empty());
}

#[test]
fn with_warning_lines_puts_one_visible_line_each_above_the_document() {
    let w = vec![
        "map A, level 3: x".to_string(),
        "map A, level 2: y".to_string(),
    ];
    let md = with_warning_lines("# Class\n\nbody\n", &w);
    let lines: Vec<&str> = md.lines().collect();
    assert_eq!(lines[0], "**Warning:** map A, level 3: x");
    assert_eq!(lines[1], "**Warning:** map A, level 2: y");
    assert!(md.ends_with("# Class\n\nbody\n"), "{md}");
}

#[test]
fn with_warning_lines_without_warnings_is_the_document_unchanged() {
    assert_eq!(with_warning_lines("# Class\n", &[]), "# Class\n");
}

// ── T075: no data is said ───────────────────────────────────────────────────

#[test]
fn no_data_fields_say_the_global_was_read_and_held_nothing() {
    let empty = ShapeReport {
        global: "^IadSm173NoData".into(),
        nodes_scanned: 0,
        ..Default::default()
    };
    let f = no_data_fields(&empty, "USER").unwrap();
    assert_eq!(f["no_data"], true);
    assert_eq!(f["sampled_nodes"], 0);
    assert_eq!(
        f["message"],
        "looked at ^IadSm173NoData in USER: it holds no data, so nothing here is checked against data"
    );
}

#[test]
fn no_data_fields_are_absent_when_nodes_were_sampled() {
    let full = ShapeReport {
        global: "^IadSm173NoData".into(),
        nodes_scanned: 3,
        ..Default::default()
    };
    assert!(no_data_fields(&full, "USER").is_none());
}
