//! Spec 173, T020: the master `propose` links against, whether drafted from a sample
//! (`MasterSpec::from_draft`) or read from a class (`MasterSpec::from_class`), and the property
//! name check both paths use.

use iris_agentic_dev_core::tools::storage_map::master::{
    is_valid_property_name, MasterSpec, Refusal,
};
use iris_agentic_dev_core::tools::storage_map::model::{from_tree, StorageModel};
use iris_agentic_dev_core::tools::storage_map::sample::parse;
use iris_agentic_dev_core::tools::storage_map::shape::ShapeReport;
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
use std::fs;

/// The sampler's lines for `Member`'s seed: the root holds the counter 3; rows 1-3 hold
/// name^city^age.
const MEMBER: &str = r#"{"d":0,"s":11,"f":"plain","p":1}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"done":1,"c":4,"capped":0}
"#;

/// A row with three fields and a sub-node `contact` with two pieces: five fields in all.
const WITH_SUB_NODE: &str = r#"{"d":1,"s":11,"f":"$LIST","p":2}
{"d":2,"s":1,"n":"contact","f":"delimited","dl":94,"p":2}
{"d":1,"s":11,"f":"$LIST","p":3}
{"d":2,"s":1,"n":"contact","f":"delimited","dl":94,"p":2}
{"done":1,"c":4,"capped":0}
"#;

/// Rows two levels down, so the key is two subscripts long.
const TWO_LEVELS: &str = r#"{"d":1,"s":10}
{"d":2,"s":1,"f":"delimited","dl":94,"p":2}
{"d":2,"s":1,"f":"delimited","dl":94,"p":2}
{"d":2,"s":1,"f":"delimited","dl":94,"p":2}
{"done":1,"c":4,"capped":0}
"#;

fn report(lines: &str, global: &str) -> ShapeReport {
    let mut report = parse(lines).unwrap();
    report.global = global.into();
    report
}

fn member_report() -> ShapeReport {
    report(MEMBER, "^IadSm172Member")
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn refusal(r: Result<MasterSpec, Refusal>) -> (String, String) {
    let e = r.expect_err("must be refused");
    (e.code.to_string(), e.message)
}

// ---------------------------------------------------------------------------
// The property name check
// ---------------------------------------------------------------------------

#[test]
fn test_master_173_property_name_accepts_letters_digits_and_a_leading_percent() {
    for ok in ["Name", "a", "Field1", "%Id", "A1b2", "X"] {
        assert!(is_valid_property_name(ok), "{ok} should be valid");
    }
    assert!(
        is_valid_property_name(&"a".repeat(128)),
        "128 characters is the limit"
    );
}

#[test]
fn test_master_173_property_name_refuses_everything_else() {
    for bad in [
        "", "1Name", "_x", "a b", "a-b", "a.b", "A;B", "Name\n", "x\"y", "a$b", "%", "%1", "é",
    ] {
        assert!(!is_valid_property_name(bad), "{bad:?} should be refused");
    }
    assert!(
        !is_valid_property_name(&"a".repeat(129)),
        "129 characters is one too many"
    );
}

// ---------------------------------------------------------------------------
// from_draft
// ---------------------------------------------------------------------------

#[test]
fn test_master_173_draft_without_names_uses_the_placeholders() {
    let spec = MasterSpec::from_draft(&member_report(), &[]).unwrap();
    assert_eq!(spec.global, "^IadSm172Member");
    assert_eq!(spec.class, None);
    assert_eq!(spec.key_names, names(&["ID"]));
    assert_eq!(spec.key_count(), 1);
    assert_eq!(spec.subscripts, names(&["{ID}"]));
    let got: Vec<&str> = spec.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(got, ["Field1", "Field2", "Field3"]);
    let pieces: Vec<Option<&str>> = spec.fields.iter().map(|f| f.piece.as_deref()).collect();
    assert_eq!(pieces, [Some("1"), Some("2"), Some("3")]);
    assert!(spec
        .fields
        .iter()
        .all(|f| f.delimiter.as_deref() == Some("\"^\"")));
    assert!(spec.fields.iter().all(|f| f.guessed));
}

#[test]
fn test_master_173_draft_takes_the_callers_names_by_position() {
    let spec = MasterSpec::from_draft(&member_report(), &names(&["Name", "City", "Age"])).unwrap();
    let got: Vec<(&str, &str)> = spec
        .fields
        .iter()
        .map(|f| (f.placeholder.as_str(), f.name.as_str()))
        .collect();
    assert_eq!(
        got,
        [("Field1", "Name"), ("Field2", "City"), ("Field3", "Age")]
    );
}

#[test]
fn test_master_173_draft_counts_sub_node_fields_after_the_row_fields() {
    let report = report(WITH_SUB_NODE, "^X");
    let spec = MasterSpec::from_draft(&report, &[]).unwrap();
    let got: Vec<&str> = spec.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(got, ["Field1", "Field2", "Field3", "Contact1", "Contact2"]);
    let c = &spec.fields[3];
    assert_eq!(c.node.as_deref(), Some("\"contact\""));
    assert_eq!(c.piece.as_deref(), Some("1"));
    // Field 4 by position is the first contact piece.
    let spec = MasterSpec::from_draft(&report, &names(&["A", "B", "C", "D", "E"])).unwrap();
    assert_eq!(spec.fields[3].name, "D");
    assert_eq!(spec.fields[3].placeholder, "Contact1");
}

#[test]
fn test_master_173_draft_with_two_levels_has_a_two_part_key() {
    let spec = MasterSpec::from_draft(&report(TWO_LEVELS, "^X"), &[]).unwrap();
    assert_eq!(spec.key_count(), 2);
    assert_eq!(spec.key_names, names(&["Key1", "ID"]));
    assert_eq!(spec.subscripts, names(&["{Key1}", "{ID}"]));
}

#[test]
fn test_master_173_draft_refuses_a_short_list_naming_the_count_found() {
    let (code, msg) = refusal(MasterSpec::from_draft(
        &member_report(),
        &names(&["Name", "City"]),
    ));
    assert_eq!(code, "INVALID_PARAMS");
    assert!(msg.contains("field_names"), "{msg}");
    assert!(
        msg.contains("3 fields"),
        "the message names the 3 fields the sample found: {msg}"
    );
    assert!(msg.contains("2 name"), "and the 2 names given: {msg}");
}

#[test]
fn test_master_173_draft_refuses_a_duplicate_ignoring_case() {
    let (code, msg) = refusal(MasterSpec::from_draft(
        &member_report(),
        &names(&["Name", "City", "name"]),
    ));
    assert_eq!(code, "INVALID_PARAMS");
    assert!(msg.to_lowercase().contains("duplicate"), "{msg}");
    assert!(msg.contains("name"), "{msg}");
}

#[test]
fn test_master_173_draft_refuses_a_name_that_collides_with_a_key_name() {
    let (code, _) = refusal(MasterSpec::from_draft(
        &member_report(),
        &names(&["Name", "City", "ID"]),
    ));
    assert_eq!(code, "INVALID_PARAMS");
}

#[test]
fn test_master_173_draft_refuses_an_invalid_name() {
    for bad in ["1st", "a b", "X) Kill ^Y", "", "%"] {
        let (code, msg) = refusal(MasterSpec::from_draft(
            &member_report(),
            &names(&["Name", "City", bad]),
        ));
        assert_eq!(code, "INVALID_PARAMS", "{bad:?}");
        assert!(msg.contains("property name"), "{bad:?}: {msg}");
    }
}

#[test]
fn test_master_173_draft_ignores_names_past_the_field_count() {
    let spec = MasterSpec::from_draft(&member_report(), &names(&["Name", "City", "Age", "Extra"]))
        .unwrap();
    assert_eq!(spec.fields.len(), 3);
}

// ---------------------------------------------------------------------------
// from_class
// ---------------------------------------------------------------------------

fn model_of_fixture(name: &str) -> StorageModel {
    let path = format!("tests/fixtures/storage172/classes/{name}.cls");
    let text = fs::read_to_string(&path).expect("fixture not found");
    model_of_text(&format!("IadSm172.{name}"), &text)
}

fn model_of_text(class: &str, text: &str) -> StorageModel {
    let spans = split_class(text).expect("split failed");
    let span = &spans[0];
    let nodes = parse_body(&text[span.body_start..span.body_end]).expect("parse failed");
    from_tree(class, &span.name, &nodes)
}

/// A class whose Storage block holds `maps` (SQLMap elements) and the SQL storage type.
fn class_with_maps(maps: &str, storage_type: &str) -> String {
    format!(
        "Class X.Y Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\nStorage SQLStorage\n{{\n{maps}<Type>{storage_type}</Type>\n}}\n\n}}\n"
    )
}

#[test]
fn test_master_173_class_copy_has_a_three_part_key() {
    let spec = MasterSpec::from_class(&model_of_fixture("Copy")).unwrap();
    assert_eq!(spec.class.as_deref(), Some("IadSm172.Copy"));
    assert_eq!(spec.global, "^IadSm172Copy");
    assert_eq!(spec.map_name, "Master");
    assert_eq!(spec.key_count(), 3);
    assert_eq!(spec.key_names, names(&["BookId", "Branch", "CopyNo"]));
    assert_eq!(
        spec.subscripts,
        names(&["{BookId}", "{Branch}", "{CopyNo}"])
    );
    assert_eq!(spec.row_id.len(), 3);
    assert_eq!(spec.row_id[1].field.as_deref(), Some("Branch"));
    let got: Vec<&str> = spec.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(got, ["Shelf"]);
    assert!(
        spec.fields.iter().all(|f| !f.guessed),
        "a class's names are its own"
    );
}

#[test]
fn test_master_173_class_member_has_a_one_part_key_and_three_fields() {
    let spec = MasterSpec::from_class(&model_of_fixture("Member")).unwrap();
    assert_eq!(spec.global, "^IadSm172Member");
    assert_eq!(spec.key_count(), 1);
    assert_eq!(spec.key_names, names(&["ID"]));
    assert!(spec.row_id.is_empty());
    let got: Vec<(&str, Option<&str>)> = spec
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.piece.as_deref()))
        .collect();
    assert_eq!(
        got,
        [("Age", Some("3")), ("City", Some("2")), ("Name", Some("1"))]
    );
    assert!(spec
        .fields
        .iter()
        .all(|f| f.delimiter.as_deref() == Some("\"^\"")));
}

#[test]
fn test_master_173_class_takes_the_data_map_even_when_an_index_map_comes_first() {
    let spec = MasterSpec::from_class(&model_of_fixture("MemberByCity")).unwrap();
    assert_eq!(spec.map_name, "Master");
    assert_eq!(spec.global, "^IadSm172MemberByCity");
}

#[test]
fn test_master_173_class_refuses_default_storage_with_the_reason_read_gives() {
    let model = model_of_fixture("DefaultBook");
    let (code, msg) = refusal(MasterSpec::from_class(&model));
    assert_eq!(code, "STORAGE_MAP_READ_ONLY");
    assert!(msg.contains("default storage"), "{msg}");
    assert!(msg.contains("IadSm172.DefaultBook"), "{msg}");
}

#[test]
fn test_master_173_class_refuses_a_columnar_map() {
    let maps = "<SQLMap name=\"Master\">\n<Data name=\"A\">\n<Delimiter>\"^\"</Delimiter>\n<Piece>1</Piece>\n</Data>\n<Global>^XY</Global>\n<Subscript name=\"1\">\n<Expression>{ID}</Expression>\n</Subscript>\n<Type>data</Type>\n</SQLMap>\n<SQLMap name=\"Cols\">\n<Global>^XYC</Global>\n<Subscript name=\"1\">\n<Expression>{ID}</Expression>\n</Subscript>\n<Type>columnar</Type>\n</SQLMap>\n";
    let model = model_of_text("X.Y", &class_with_maps(maps, "%Storage.SQL"));
    let (code, msg) = refusal(MasterSpec::from_class(&model));
    assert_eq!(code, "STORAGE_MAP_READ_ONLY");
    assert!(msg.contains("columnar"), "{msg}");
    assert!(msg.contains("Cols"), "the map is named: {msg}");
}

#[test]
fn test_master_173_class_refuses_a_class_with_no_data_master_map() {
    let maps = "<SQLMap name=\"OnlyIdx\">\n<Global>^XIdx</Global>\n<Subscript name=\"1\">\n<Expression>{A}</Expression>\n</Subscript>\n<Type>index</Type>\n</SQLMap>\n";
    let model = model_of_text("X.Y", &class_with_maps(maps, "%Storage.SQL"));
    let (code, msg) = refusal(MasterSpec::from_class(&model));
    assert_eq!(code, "INVALID_PARAMS");
    assert!(msg.contains("no data map"), "{msg}");
    assert!(msg.contains("X.Y"), "{msg}");
}

#[test]
fn test_master_173_class_refuses_other_read_only_storage_with_its_reason() {
    let maps = "<SQLMap name=\"Master\">\n<Global>^XY</Global>\n<Subscript name=\"1\">\n<Expression>{ID}</Expression>\n</Subscript>\n<Type>data</Type>\n</SQLMap>\n";
    let model = model_of_text("X.Y", &class_with_maps(maps, "%Storage.Persistent"));
    assert!(!model.storage.editable);
    let (code, msg) = refusal(MasterSpec::from_class(&model));
    assert_eq!(code, "STORAGE_MAP_READ_ONLY");
    assert!(msg.contains("%Storage.Persistent"), "{msg}");
}
