//! FR-012, FR-012a: `write::judge_stored` judges what IRIS stored against what iad sent. One
//! judgment for both writers (`iris_storage_map` edit/replace and the `iris_doc` hand-off): the
//! stored tree when it is what was sent, else the answer that names what went wrong.

use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class, Node};
use iris_agentic_dev_core::tools::storage_map::write::judge_stored;
use serde_json::{json, Value};

fn member() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes/Member.cls");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn nodes_of(text: &str) -> Vec<Node> {
    let span = split_class(text)
        .expect("split")
        .into_iter()
        .next()
        .expect("a Storage block");
    parse_body(&text[span.body_start..span.body_end]).expect("parse")
}

/// `nodes` as the one Storage block `Member` has.
fn one(nodes: &[Node]) -> Vec<(String, Vec<Node>)> {
    vec![("SQLStorage".to_string(), nodes.to_vec())]
}

fn no_gate(_: &[Node]) -> Option<Value> {
    None
}

const DOC: &str = "IadSm172.Member.cls";

#[test]
fn what_was_sent_comes_back_as_the_stored_tree() {
    let text = member();
    let sent = nodes_of(&text);
    let got = judge_stored(&one(&sent), Some(&text), DOC, no_gate).expect("the same map");
    assert_eq!(got, one(&sent));
}

#[test]
fn no_text_or_no_storage_block_is_a_writer_mismatch_naming_the_class() {
    let sent = nodes_of(&member());
    for stored in [
        None,
        Some("Class IadSm172.Member Extends %Persistent\n{\n}\n"),
    ] {
        let err = judge_stored(&one(&sent), stored, DOC, no_gate).expect_err("nothing to judge");
        assert_eq!(err["error_code"], "STORAGE_MAP_WRITER_MISMATCH", "{err}");
        assert_eq!(
            err["error"],
            format!("iad cannot read back the Storage block IRIS stored for {DOC}"),
            "{err}"
        );
    }
}

#[test]
fn an_element_iris_did_not_keep_is_dropped_and_listed() {
    let text = member();
    let sent = nodes_of(&text.replacen(
        "<Global>^IadSm172Member</Global>",
        "<Global>^IadSm172Member</Global>\n<Wibble>x</Wibble>",
        1,
    ));
    let err = judge_stored(&one(&sent), Some(&text), DOC, no_gate).expect_err("Wibble is gone");
    assert_eq!(err["error_code"], "STORAGE_MAP_DROPPED", "{err}");
    assert_eq!(err["error"], "IRIS dropped 1 element(s) iad sent", "{err}");
    let dropped = err["dropped"].as_array().expect("a list");
    assert_eq!(dropped.len(), 1, "{err}");
    assert!(dropped[0].as_str().unwrap().ends_with("/Wibble"), "{err}");
}

#[test]
fn a_value_iris_changed_is_a_writer_mismatch_with_the_difference() {
    let text = member();
    let sent = nodes_of(&text.replacen("<Piece>3</Piece>", "<Piece>4</Piece>", 1));
    let err = judge_stored(&one(&sent), Some(&text), DOC, no_gate).expect_err("piece differs");
    assert_eq!(err["error_code"], "STORAGE_MAP_WRITER_MISMATCH", "{err}");
    assert_eq!(
        err["error"],
        "IRIS stored a different map than iad sent (1 difference(s)); this is an iad bug",
        "{err}"
    );
    let d = err["differences"][0].as_str().unwrap();
    assert!(d.ends_with("sent \"4\", IRIS has \"3\""), "{err}");
}

#[test]
fn the_gates_judge_the_stored_tree_and_their_refusal_is_the_answer() {
    let text = member();
    let sent = nodes_of(&text);
    let mut seen = 0;
    let refusal = json!({"success": false, "error_code": "CODE_EDIT_BLOCKED"});
    let err = judge_stored(&one(&sent), Some(&text), DOC, |stored: &[Node]| {
        seen = stored.len();
        Some(refusal.clone())
    })
    .expect_err("the gate refuses");
    assert_eq!(err, refusal);
    assert_eq!(seen, sent.len(), "the gate saw the stored tree");
}

/// IRIS stores `<Type>%CacheSQLStorage</Type>` as `%Storage.SQL` (measured 2026-11-14), so a class
/// written with the old name is what was sent, not a writer mismatch.
#[test]
fn the_old_storage_type_name_comes_back_renamed_and_that_is_no_difference() {
    let text = member();
    let sent = nodes_of(&text.replacen(
        "<Type>%Storage.SQL</Type>",
        "<Type>%CacheSQLStorage</Type>",
        1,
    ));
    let got = judge_stored(&one(&sent), Some(&text), DOC, no_gate).expect("the rename is IRIS's");
    assert_eq!(got, one(&nodes_of(&text)));
}
