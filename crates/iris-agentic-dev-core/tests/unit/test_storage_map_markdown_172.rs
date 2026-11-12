//! `diagram::markdown`: a class's storage as one Markdown document a person can read, and a
//! cheap first step toward a GUI. For each map: the global layout, a subscript table, a field
//! table, the row ID, the facts `read` derives, and the map's Mermaid block.

use iris_agentic_dev_core::tools::storage_map::diagram::markdown;
use iris_agentic_dev_core::tools::storage_map::model::{derived_facts, from_tree, StorageModel};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};

fn model(name: &str, text: &str) -> StorageModel {
    let span = split_class(text).unwrap().remove(0);
    let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
    let mut m = from_tree(&format!("IadSm172.{name}"), &span.name, &nodes);
    m.derived = derived_facts(&m);
    m
}

fn fixture(name: &str) -> StorageModel {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    model(name, &std::fs::read_to_string(&p).unwrap())
}

/// The cells of the table row whose first cell is `first`, trimmed.
fn row<'a>(md: &'a str, first: &str) -> Vec<&'a str> {
    md.lines()
        .find(|l| l.starts_with('|') && l.split('|').nth(1).map(str::trim) == Some(first))
        .unwrap_or_else(|| panic!("no row starting {first:?} in:\n{md}"))
        .trim_matches('|')
        .split(" | ")
        .map(str::trim)
        .collect()
}

#[test]
fn test_storage_map_markdown_172_member_reads_as_its_layout() {
    let md = markdown(&fixture("Member"), None);
    assert!(md.starts_with("# Storage of IadSm172.Member\n"), "{md}");
    assert!(md.contains("## Map `Master`"), "{md}");
    assert!(
        md.contains("`^IadSm172Member({ID})`"),
        "global layout:\n{md}"
    );
    // Fields: Age is piece 3 of the row node, delimited by "^".
    assert_eq!(
        row(&md, "`Age`"),
        vec!["`Age`", "row node", "`\"^\"`", "`3`", "delimited", "yes"]
    );
    assert_eq!(row(&md, "`Name`")[3], "`1`");
    // Subscript level 1 is the ID.
    assert_eq!(row(&md, "1")[1], "`{ID}`");
    assert!(
        md.contains("`$i(^IadSm172Member)`"),
        "the ID expression:\n{md}"
    );
    assert!(md.contains("```mermaid\n"), "{md}");
}

#[test]
fn test_storage_map_markdown_172_says_list_when_there_is_no_delimiter() {
    let md = markdown(&fixture("LoanList"), None);
    assert_eq!(row(&md, "`MemberId`")[4], "$LIST", "{md}");
    assert!(
        md.contains("no delimiter: node read as $LIST"),
        "derived fact:\n{md}"
    );
}

#[test]
fn test_storage_map_markdown_172_multi_line_code_goes_in_a_block() {
    let md = markdown(&fixture("Shelf"), None);
    assert!(
        md.contains("```objectscript\n s {L1}=$o(^IadSm172Shelf({L1}))\n"),
        "{md}"
    );
    assert!(md.contains("\tq:{L1}=\"\""), "the tab survives:\n{md}");
}

#[test]
fn test_storage_map_markdown_172_a_pipe_in_code_does_not_break_the_table() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes/Member.cls");
    let text = std::fs::read_to_string(p).unwrap().replacen(
        "<Piece>3</Piece>",
        "<Piece>3</Piece>\n<RetrievalCode>s {*}=$s({L1}=\"\"|1:0)</RetrievalCode>",
        1,
    );
    let md = markdown(&model("Member", &text), None);
    let age = row(&md, "`Age`");
    assert_eq!(age.len(), 6, "the pipe split the row: {age:?}\n{md}");
    assert!(md.contains("\\|"), "{md}");
}

#[test]
fn test_storage_map_markdown_172_default_storage_lists_its_slots() {
    let md = markdown(&fixture("DefaultBook"), None);
    assert!(md.contains("default storage"), "{md}");
    assert!(md.contains("`^IadSm172.DefaultBookD`"), "{md}");
    assert_eq!(row(&md, "2")[1], "`Title`", "{md}");
    assert_eq!(row(&md, "4")[1], "`ISBN`", "{md}");
}

#[test]
fn test_storage_map_markdown_172_map_filter_keeps_one_map() {
    let md = markdown(&fixture("MemberByCity"), Some("CityIdx"));
    assert!(md.contains("## Map `CityIdx`"), "{md}");
    assert!(!md.contains("## Map `Master`"), "{md}");
    assert!(md.contains("`^IadSm172MemberByCityI({City},{ID})`"), "{md}");
}
