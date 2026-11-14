//! T078: which `iris_doc` puts go through the storage-map safeguards (FR-031): a class whose
//! Storage block holds an SQLMap, of any storage type. Class XML is told apart, because a map
//! in class XML is refused (FR-012a).

use iris_agentic_dev_core::tools::storage_map::write::{content_has_sql_map, is_class_xml};

fn fixture(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

const XML_EXPORT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Export generator=\"IRIS\" version=\"26\">\n<Class name=\"IadSm172.Xml\">\n\
<Super>%Persistent</Super>\n<Storage name=\"SQLStorage\">\n<Type>%Storage.SQL</Type>\n\
<SQLMap name=\"Master\">\n<Global>^IadSm172Xml</Global>\n<Type>data</Type>\n</SQLMap>\n\
</Storage>\n</Class>\n</Export>\n";

#[test]
fn test_storage_map_doc_gate_172_fixtures_with_maps_are_caught() {
    for name in [
        "Member",
        "MemberByCity",
        "Branch",
        "Copy",
        "Genre",
        "Loan",
        "LoanEvent",
        "LoanList",
        "Notes",
        "Shelf",
        "BookMulti",
        "ActiveLoan",
        "LoanUnion",
        "MemberD11",
    ] {
        let t = fixture(name);
        assert!(content_has_sql_map(&t), "{name}");
        assert!(!is_class_xml(&t), "{name}");
    }
    assert!(
        !content_has_sql_map(&fixture("DefaultBook")),
        "default storage has no SQLMap"
    );
}

#[test]
fn test_storage_map_doc_gate_172_class_xml_with_a_map() {
    assert!(is_class_xml(XML_EXPORT));
    assert!(content_has_sql_map(XML_EXPORT));
    let no_map = XML_EXPORT.replace(
        "<SQLMap name=\"Master\">\n<Global>^IadSm172Xml</Global>\n<Type>data</Type>\n</SQLMap>\n",
        "",
    );
    assert!(!content_has_sql_map(&no_map));
}

#[test]
fn test_storage_map_doc_gate_172_other_text() {
    // A routine, or a class whose only mention of SQLMap is in a comment outside Storage.
    assert!(!content_has_sql_map("ROUTINE X\n Write \"<SQLMap>\"\n"));
    let commented = "Class X.Y Extends %Persistent\n{\n\n/// Uses <SQLMap name=\"M\"> elsewhere\nProperty A As %String;\n\n}\n";
    assert!(!content_has_sql_map(commented));
    // A Storage block iad cannot parse but that names an SQLMap still goes through the
    // safeguards, where the parse failure is reported (fail closed).
    let broken = fixture("Member").replacen("</SQLMap>", "", 1);
    assert!(content_has_sql_map(&broken));
}

#[test]
fn test_storage_map_doc_gate_172_crlf_and_indentation_still_hand_off() {
    use iris_agentic_dev_core::tools::storage_map::write::stored_form;
    let member = fixture("Member");
    // Windows line endings: IRIS stores the lines, so the hand-off judges them as IRIS will.
    let crlf = member.replace('\n', "\r\n");
    assert!(
        content_has_sql_map(&crlf),
        "CRLF content skipped the hand-off"
    );
    assert_eq!(stored_form(&crlf), stored_form(&member));
    assert!(!stored_form(&crlf).contains('\r'));
    // An indented map line, and an indented Storage header the block reader does not take: both
    // go through the safeguards, where a block iad cannot read is refused.
    let indented_map = member.replace("<SQLMap name=\"Master\">", "  <SQLMap name=\"Master\">");
    assert!(content_has_sql_map(&indented_map));
    let indented_header = member.replace("\nStorage SQLStorage\n", "\n  Storage SQLStorage\n");
    assert!(content_has_sql_map(&indented_header));
}
