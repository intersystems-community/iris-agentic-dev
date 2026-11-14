//! FR-031 / FR-030c: an `iris_doc` put of a class with an SQL map is audited like an
//! `iris_storage_map` write. `write::doc_write_record` builds the record from the text IRIS held
//! before and the text it holds now.

use iris_agentic_dev_core::tools::storage_map::model::version_hex;
use iris_agentic_dev_core::tools::storage_map::tree::{
    emit_body, normalize, parse_body, split_class,
};
use iris_agentic_dev_core::tools::storage_map::write::doc_write_record;

fn member() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes/Member.cls");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `golden/Member.read.json`'s `storage.version`.
const MEMBER_READ_VERSION: &str = "7d4752f10786ed02";

/// The storage version `read` reports for `text`.
fn version(text: &str) -> String {
    let span = split_class(text).unwrap()[0].clone();
    let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
    version_hex(&emit_body(&normalize(nodes)))
}

#[test]
fn a_changed_piece_is_recorded_with_both_versions_and_no_code_change() {
    let before = member();
    let after = before.replacen("<Piece>3</Piece>", "<Piece>4</Piece>", 1);
    let r = doc_write_record("IadSm172.Member", Some(&before), &after);
    assert_eq!(r["class"], "IadSm172.Member");
    assert_eq!(r["map"], serde_json::Value::Null);
    // The version `read` reports for Member, pinned in its golden file: the record names the same
    // version the agent saw, not one this test computes.
    assert_eq!(r["version_before"], MEMBER_READ_VERSION);
    assert_eq!(r["version_after"], version(&after));
    assert_ne!(r["version_before"], r["version_after"]);
    assert_eq!(r["changed_paths"], serde_json::json!(["/maps/Master"]));
    assert_eq!(r["code_changed"], false);
}

#[test]
fn a_new_class_has_no_version_before_and_its_map_code_is_new() {
    let after = member();
    let r = doc_write_record("IadSm172.Member", None, &after);
    assert_eq!(r["version_before"], "");
    assert_eq!(r["version_after"], MEMBER_READ_VERSION);
    assert_eq!(r["changed_paths"], serde_json::json!(["/maps/Master"]));
    assert_eq!(r["code_changed"], true);
}
