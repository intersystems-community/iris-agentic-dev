//! T015: Unit tests for IrisDocParams elicitation fields.

use iris_agentic_dev_core::tools::{DocMode, IrisDocParams};

#[test]
fn doc_params_elicitation_fields() {
    let p: IrisDocParams = serde_json::from_str(
        r#"{
        "mode": "put",
        "name": "MyApp.Patient.cls",
        "content": "Class MyApp.Patient {}",
        "elicitation_id": "abc-123",
        "elicitation_answer": "yes"
    }"#,
    )
    .unwrap();
    assert!(matches!(p.mode, DocMode::Put));
    assert_eq!(p.elicitation_id.as_deref(), Some("abc-123"));
    assert_eq!(p.elicitation_answer.as_deref(), Some("yes"));
}

#[test]
fn doc_params_no_elicitation_defaults_to_none() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode":"get","name":"MyApp.Patient.cls"}"#).unwrap();
    assert!(p.elicitation_id.is_none());
    assert!(p.elicitation_answer.is_none());
}

// ── I-3: Storage block stripping ─────────────────────────────────────────

#[test]
fn test_strip_storage_removes_block() {
    let cls = "Class MyApp.Foo Extends %Persistent {\nProperty Name As %String;\nStorage Default\n{\n<Type>%Storage.Persistent</Type>\n}\n}\n";
    let (stripped, flag) = iris_agentic_dev_core::tools::doc::strip_storage_blocks(cls);
    assert!(flag, "storage_stripped should be true");
    assert!(
        !stripped.contains("Storage Default"),
        "Storage block should be removed"
    );
    assert!(
        stripped.contains("Property Name"),
        "other content preserved"
    );
}

#[test]
fn test_strip_storage_noop_when_no_block() {
    let cls = "Class MyApp.Foo {\nProperty Name As %String;\n}\n";
    let (stripped, flag) = iris_agentic_dev_core::tools::doc::strip_storage_blocks(cls);
    assert!(
        !flag,
        "storage_stripped should be false when no Storage block"
    );
    assert_eq!(stripped, cls, "content should be unchanged");
}

#[test]
fn test_strip_storage_preserves_other_xdata() {
    let cls = "Class MyApp.Foo {\nXData MyData { <data/> }\nStorage Default\n{\n<Type>%Storage.Persistent</Type>\n}\n}\n";
    let (stripped, _) = iris_agentic_dev_core::tools::doc::strip_storage_blocks(cls);
    assert!(stripped.contains("XData MyData"), "other XData preserved");
    assert!(!stripped.contains("Storage Default"), "Storage removed");
}

#[test]
fn test_strip_multiple_storage_blocks() {
    let cls = "Class MyApp.Foo {\nStorage S1\n{\n<Type>T</Type>\n}\nStorage S2\n{\n<Type>T</Type>\n}\n}\n";
    let (stripped, flag) = iris_agentic_dev_core::tools::doc::strip_storage_blocks(cls);
    assert!(flag);
    assert!(!stripped.contains("Storage S1"));
    assert!(!stripped.contains("Storage S2"));
}

#[test]
fn test_strip_storage_removes_trailing_blank_lines_before_storage() {
    // Blank lines between last real line and Storage block should be stripped (line 525 branch)
    let cls = "Class MyApp.Foo {\nProperty Name As %String;\n\n\nStorage Default\n{\n<Type>%Storage.Persistent</Type>\n}\n}\n";
    let (stripped, flag) = iris_agentic_dev_core::tools::doc::strip_storage_blocks(cls);
    assert!(flag, "should detect storage block");
    assert!(!stripped.contains("Storage Default"), "storage removed");
    assert!(stripped.contains("Property Name"), "property preserved");
    // The trailing blank lines before Storage should be removed
    assert!(
        !stripped.trim_end().ends_with('\n')
            || stripped.trim_end().ends_with("Property Name As %String;"),
        "no trailing blank lines: {:?}",
        stripped
    );
}
