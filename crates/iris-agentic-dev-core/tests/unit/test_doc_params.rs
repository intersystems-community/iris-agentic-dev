//! T015: Unit tests for IrisDocParams elicitation fields.

use iris_agentic_dev_core::tools::IrisDocParams;

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
    assert_eq!(p.mode, "put");
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

// ── Lenient numeric deserialization (string-or-int) ──────────────────────
// LLMs frequently serialize numeric args as strings ("214"); a hard serde type
// error rejects the whole tool call at the JSON-RPC layer and drives a retry loop.

#[test]
fn doc_params_line_as_string_coerces_to_int() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode":"insert","name":"F.cls","line":"214","expected":"x"}"#)
            .unwrap();
    assert_eq!(p.line, Some(214));
}

#[test]
fn doc_params_start_end_as_strings_coerce() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode":"fragment","name":"F.cls","start":"1","end":"5"}"#)
            .unwrap();
    assert_eq!(p.start, Some(1));
    assert_eq!(p.end, Some(5));
}

#[test]
fn doc_params_max_results_as_string_coerces() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode":"list","pattern":"User.*","max_results":"50"}"#).unwrap();
    assert_eq!(p.max_results, Some(50));
}

#[test]
fn doc_params_line_as_int_still_works() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode":"insert","name":"F.cls","line":214,"expected":"x"}"#)
            .unwrap();
    assert_eq!(p.line, Some(214));
}

#[test]
fn doc_params_numeric_fields_missing_are_none() {
    let p: IrisDocParams = serde_json::from_str(r#"{"mode":"get","name":"F.cls"}"#).unwrap();
    assert!(p.line.is_none());
    assert!(p.start.is_none());
    assert!(p.end.is_none());
    assert!(p.max_results.is_none());
}

#[test]
fn doc_params_empty_string_int_is_none() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode":"insert","name":"F.cls","line":""}"#).unwrap();
    assert!(p.line.is_none());
}

#[test]
fn doc_params_non_numeric_string_errors() {
    // A genuinely non-numeric value stays an error (documents the coercion boundary).
    let r: Result<IrisDocParams, _> =
        serde_json::from_str(r#"{"mode":"insert","name":"F.cls","line":"abc"}"#);
    assert!(r.is_err());
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
