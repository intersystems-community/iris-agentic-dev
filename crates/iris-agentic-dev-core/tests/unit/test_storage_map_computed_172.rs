//! What Benjamin De Boe's iFind case showed (2026-11-26): a level the map computes by code is not
//! a subscript of the global, `compare` must say when it checked nothing, `propose` must say what
//! it did not infer, and the compiler's own `StreamLocation` is noise unless a stream property
//! uses it. Each test names the known answer, not the shape of the answer.

use iris_agentic_dev_core::tools::storage_map::diagram::markdown;
use iris_agentic_dev_core::tools::storage_map::model::{
    derived_facts, from_tree, mark_compiler_added, StorageModel,
};
use iris_agentic_dev_core::tools::storage_map::shape::{
    compare_map, compare_notes, draft, not_inferred, Format, Pieces, ShapeReport,
};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};

fn model_from(name: &str, text: &str) -> StorageModel {
    let span = split_class(text).unwrap().remove(0);
    let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
    let mut m = from_tree(&format!("IadSm172.{name}"), &span.name, &nodes);
    m.derived = derived_facts(&m);
    m
}

fn fixture_text(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    std::fs::read_to_string(p).unwrap()
}

fn facts_at<'a>(m: &'a StorageModel, path: &str) -> Vec<&'a str> {
    m.derived
        .iter()
        .filter(|d| d.path == path)
        .map(|d| d.fact.as_str())
        .collect()
}

// ── computed levels ────────────────────────────────────────────────────────

#[test]
fn computed_level_the_bitmap_chunk_third_subscript_is_flagged_and_the_others_are_not() {
    let m = model_from("Bits", &fixture_text("Bits"));
    let third = facts_at(&m, "/maps/Chunks/subscripts/3");
    assert_eq!(third.len(), 1, "{:?}", m.derived);
    for want in [
        "computed by code",
        "AccessType Other",
        "NextCode",
        "may not exist in the global",
        "$BIT",
        "check the map against real data",
    ] {
        assert!(
            third[0].contains(want),
            "missing {want:?} in {:?}",
            third[0]
        );
    }
    assert!(facts_at(&m, "/maps/Chunks/subscripts/1").is_empty());
}

#[test]
fn computed_level_an_access_variable_with_code_is_an_info_line_on_its_level() {
    let m = model_from("Bits", &fixture_text("Bits"));
    let second = facts_at(&m, "/maps/Chunks/subscripts/2");
    assert_eq!(second.len(), 1, "{:?}", m.derived);
    assert!(
        second[0].contains("access variable 2D2") && second[0].contains("sets it by code"),
        "{:?}",
        second[0]
    );
    // An access variable alone does not say the level is invented.
    assert!(!second[0].contains("may not exist"), "{:?}", second[0]);
}

#[test]
fn computed_level_an_invalid_condition_alone_is_a_filter_not_an_invented_level() {
    let m = model_from("Route", &fixture_text("Route"));
    let filters: Vec<_> = m
        .derived
        .iter()
        .filter(|d| d.fact.contains("filters its keys by code"))
        .collect();
    assert!(
        !filters.is_empty(),
        "Route has an Invalidcondition: {:?}",
        m.derived
    );
    // The level is a real subscript, so it must not be called invented.
    assert!(filters
        .iter()
        .all(|d| !d.fact.contains("may not exist") && !d.fact.contains("computed by code")));
}

#[test]
fn computed_level_a_plain_map_gets_no_computed_warning() {
    let m = model_from("Member", &fixture_text("Member"));
    assert!(
        m.derived
            .iter()
            .all(|d| !d.fact.contains("computed by code")),
        "{:?}",
        m.derived
    );
}

#[test]
fn computed_level_shows_in_the_markdown_a_person_reads() {
    let md = markdown(&model_from("Bits", &fixture_text("Bits")), None);
    assert!(md.contains("computed by code"), "{md}");
    assert!(md.contains("may not exist in the global"), "{md}");
}

// ── StreamLocation ─────────────────────────────────────────────────────────

#[test]
fn stream_location_is_compiler_added_when_the_class_has_no_stream_property() {
    let text = fixture_text("Bits");
    let mut m = model_from("Bits", &text);
    mark_compiler_added(&mut m, &text);
    assert_eq!(m.compiler_added, vec!["StreamLocation".to_string()]);
    // read keeps the element, the Markdown leaves it out.
    assert!(m.storage.keywords.contains_key("StreamLocation"));
    let md = markdown(&m, None);
    assert!(!md.contains("StreamLocation"), "{md}");
    assert!(!md.contains("IadSm172BitsS"), "{md}");
}

#[test]
fn stream_location_stays_when_a_property_is_a_stream() {
    let text = fixture_text("Bits").replace(
        "Property Word As %Integer;",
        "Property Word As %Integer;\n\nProperty Body As %Stream.GlobalCharacter;",
    );
    let mut m = model_from("Bits", &text);
    mark_compiler_added(&mut m, &text);
    assert!(m.compiler_added.is_empty(), "{:?}", m.compiler_added);
    assert!(markdown(&m, None).contains("StreamLocation"));
}

#[test]
fn stream_location_stays_when_the_class_extends_something_that_may_carry_streams() {
    let text = fixture_text("Bits").replace(
        "Extends %Persistent",
        "Extends (%Persistent, IadSm172.Base)",
    );
    let mut m = model_from("Bits", &text);
    mark_compiler_added(&mut m, &text);
    assert!(m.compiler_added.is_empty(), "{:?}", m.compiler_added);
}

#[test]
fn stream_location_json_has_no_compiler_added_key_when_nothing_is_marked() {
    let m = model_from("Member", &fixture_text("Member"));
    let j = serde_json::to_value(&m).unwrap();
    assert!(j.get("compiler_added").is_none(), "{j}");
}

// ── compare says when it checked nothing ───────────────────────────────────

fn report(nodes: u32, depth: u32, pieces: u32) -> ShapeReport {
    let mut r = ShapeReport {
        global: "^IadSm172Bits".into(),
        nodes_scanned: nodes,
        ..Default::default()
    };
    if nodes > 0 {
        r.depths.insert(depth.to_string(), nodes);
        r.formats.push(Format {
            depth,
            node: None,
            format: "plain".into(),
            delimiter: None,
            pieces: Pieces {
                min: pieces,
                max: pieces,
            },
        });
    }
    r
}

fn bits_map() -> iris_agentic_dev_core::tools::storage_map::model::SqlMap {
    model_from("Bits", &fixture_text("Bits"))
        .storage
        .maps
        .remove(0)
}

#[test]
fn compare_notes_no_data_means_not_verified_and_names_the_global() {
    let map = bits_map();
    let r = report(0, 0, 0);
    assert!(compare_map(&map, &r).iter().all(|m| !m.seen.is_empty()));
    let n = compare_notes(&map, &r);
    assert!(!n.verified);
    assert!(
        n.notes
            .iter()
            .any(|s| s.contains("no data found at ^IadSm172Bits, the map was not checked")),
        "{:?}",
        n.notes
    );
}

#[test]
fn compare_notes_with_data_is_verified_and_has_no_depth_note_without_a_computed_level() {
    let m = model_from("Member", &fixture_text("Member"))
        .storage
        .maps
        .remove(0);
    let n = compare_notes(&m, &report(5, 1, 3));
    assert!(n.verified);
    assert!(n.notes.is_empty(), "{:?}", n.notes);
}

#[test]
fn compare_notes_a_computed_level_with_data_two_deep_names_both_depths() {
    // The map declares three levels; the data holds its bit strings at depth 2.
    let n = compare_notes(&bits_map(), &report(7, 2, 0));
    assert!(n.verified);
    let note = n
        .notes
        .iter()
        .find(|s| s.contains("declares 3 levels"))
        .unwrap_or_else(|| panic!("{:?}", n.notes));
    assert!(note.contains("data sits at depth 2"), "{note}");
    assert!(note.contains("level 3"), "{note}");
}

// ── propose says what it did not infer ─────────────────────────────────────

#[test]
fn propose_not_inferred_lists_what_the_draft_leaves_to_the_caller() {
    let list = not_inferred();
    for want in [
        "datatypes",
        "collations",
        "index maps",
        "conditions",
        "computed subscripts",
        "ID counter",
    ] {
        assert!(
            list.iter().any(|s| s.contains(want)),
            "nothing about {want:?}: {list:?}"
        );
    }
}

#[test]
fn propose_draft_still_has_its_known_depth_and_pieces() {
    let mut r = report(3, 1, 3);
    r.formats[0].format = "delimited".into();
    r.formats[0].delimiter = Some("^".into());
    let (d, guesses) = draft(&r);
    assert_eq!(d["subscripts"].as_array().unwrap().len(), 1);
    assert_eq!(d["data"].as_object().unwrap().len(), 3);
    assert!(guesses.iter().any(|g| g.ends_with("/subscripts")));
}
