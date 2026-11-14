//! FR-012a: every Storage block of a class is gated, not only the first. A class may carry more
//! than one Storage definition; IRIS keeps them all, and a second one can name any global.

use iris_agentic_dev_core::tools::storage_map::tree::Node;
use iris_agentic_dev_core::tools::storage_map::write::{gate_blocks, judge_stored};
use serde_json::{json, Value};

fn member() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes/Member.cls");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `Member` with a second Storage block after the first, naming `global`.
fn with_second_block(global: &str) -> String {
    let text = member();
    let end = text.rfind("\n}").expect("the class's closing brace");
    let second = format!(
        "\nStorage Other\n{{\n<SQLMap name=\"Master\">\n<Global>{global}</Global>\n\
         <Subscript name=\"1\">\n<Expression>{{ID}}</Expression>\n</Subscript>\n\
         <Type>data</Type>\n</SQLMap>\n<Type>%Storage.SQL</Type>\n}}\n"
    );
    format!("{}{second}{}", &text[..end], &text[end..])
}

/// A gate that refuses any block naming `^ROUTINE`, and counts the blocks it saw.
fn refuse_routine(seen: &mut usize) -> impl FnMut(&[Node]) -> Option<Value> + '_ {
    move |nodes: &[Node]| {
        *seen += 1;
        let text = format!("{nodes:?}");
        text.contains("^ROUTINE")
            .then(|| json!({"success": false, "error_code": "CODE_EDIT_BLOCKED"}))
    }
}

#[test]
fn every_block_is_gated_and_a_second_blocks_global_is_refused() {
    let mut seen = 0;
    let got = gate_blocks(&with_second_block("^ROUTINE"), refuse_routine(&mut seen));
    assert_eq!(
        got,
        Some(json!({"success": false, "error_code": "CODE_EDIT_BLOCKED"}))
    );
    assert_eq!(seen, 2, "both blocks reached the gate");
}

#[test]
fn a_class_whose_blocks_all_pass_is_let_through() {
    let mut seen = 0;
    let got = gate_blocks(
        &with_second_block("^IadSm172Other"),
        refuse_routine(&mut seen),
    );
    assert_eq!(got, None);
    assert_eq!(seen, 2);
}

#[test]
fn a_second_block_that_does_not_parse_is_refused_not_skipped() {
    let text = with_second_block("^IadSm172Other").replace(
        "<Global>^IadSm172Other</Global>",
        "<Global>^IadSm172Other</Glob>",
    );
    let got = gate_blocks(&text, |_: &[Node]| None).expect("an unparsed block is refused");
    assert_eq!(got["error_code"], "STORAGE_MAP_PARSE_FAILED", "{got}");
    // The class scan stops at the broken element and quotes it.
    assert!(
        got["context"].as_str().unwrap().contains("</Glob>"),
        "the answer shows where: {got}"
    );
}

#[test]
fn the_read_back_gates_every_block_iris_stored() {
    let sent_text = member();
    let span = iris_agentic_dev_core::tools::storage_map::tree::split_class(&sent_text).unwrap()[0]
        .clone();
    let sent = iris_agentic_dev_core::tools::storage_map::tree::parse_body(
        &sent_text[span.body_start..span.body_end],
    )
    .unwrap();
    // IRIS holds a second block the first map's diff never looks at.
    let stored = with_second_block("^ROUTINE");
    let mut seen = 0;
    let sent = vec![(span.name.clone(), sent)];
    let err = judge_stored(
        &sent,
        Some(&stored),
        "IadSm172.Member.cls",
        refuse_routine(&mut seen),
    )
    .expect_err("the stored second block is refused");
    assert_eq!(err["error_code"], "CODE_EDIT_BLOCKED", "{err}");
}

/// `Member` with a second SQL Storage block `Other` whose map reads `^IadSm172Other` with
/// `code` as its subscript expression.
fn with_second_map_block(code: &str) -> String {
    let text = member();
    let end = text.rfind("\n}").expect("the class's closing brace");
    let second = format!(
        "\nStorage Other\n{{\n<SQLMap name=\"Master\">\n<Global>^IadSm172Other</Global>\n\
         <Subscript name=\"1\">\n<Expression>{code}</Expression>\n</Subscript>\n\
         <Type>data</Type>\n</SQLMap>\n<Type>%Storage.SQL</Type>\n}}\n"
    );
    format!("{}{second}{}", &text[..end], &text[end..])
}

#[test]
fn map_blocks_finds_every_block_with_a_map_by_name() {
    use iris_agentic_dev_core::tools::storage_map::write::map_blocks;
    let blocks = map_blocks(&with_second_map_block("{ID}")).expect("parses");
    let names: Vec<&str> = blocks.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["SQLStorage", "Other"]);
    // A default-storage block holds no SQL map and is not one of them.
    let default = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/storage172/classes/DefaultBook.cls"),
    )
    .unwrap();
    assert!(map_blocks(&default).unwrap().is_empty());
}

#[test]
fn map_code_changed_in_a_second_block_is_seen() {
    use iris_agentic_dev_core::tools::storage_map::write::map_code_changed;
    let before = with_second_map_block("{ID}");
    let after = with_second_map_block("{ID}+0");
    assert!(map_code_changed(Some(&before), &after).expect("parses"));
    assert!(!map_code_changed(Some(&before), &before).expect("parses"));
}

#[test]
fn the_read_back_diffs_every_sent_block_against_its_namesake() {
    use iris_agentic_dev_core::tools::storage_map::write::map_blocks;
    let sent_text = with_second_map_block("{ID}");
    let sent = map_blocks(&sent_text).unwrap();
    // IRIS kept block one and changed block two's expression.
    let stored = with_second_map_block("{ID}+1");
    let err = judge_stored(
        &sent,
        Some(&stored),
        "IadSm172.Member.cls",
        |_: &[Node]| None,
    )
    .expect_err("block two differs");
    assert_eq!(err["error_code"], "STORAGE_MAP_WRITER_MISMATCH", "{err}");
    assert!(err["differences"].to_string().contains("Other"), "{err}");
    // And a block IRIS did not keep is dropped.
    let err = judge_stored(
        &sent,
        Some(&member()),
        "IadSm172.Member.cls",
        |_: &[Node]| None,
    )
    .expect_err("block two is gone");
    assert_eq!(err["error_code"], "STORAGE_MAP_DROPPED", "{err}");
}
