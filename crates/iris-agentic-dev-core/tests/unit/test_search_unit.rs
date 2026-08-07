// Unit tests for search.rs — SearchParams deserialization (no IRIS needed).

use iris_agentic_dev_core::tools::search::SearchParams;

#[test]
fn test_search_params_minimal() {
    let p: SearchParams = serde_json::from_str(r#"{"query": "test"}"#).unwrap();
    assert_eq!(p.query, "test");
    // Omitted namespace stays None; resolution falls back to the connection namespace.
    assert_eq!(p.namespace, None);
    // bool fields default to false
    assert!(!p.regex);
    assert!(!p.case_sensitive);
    // optional fields default to empty/None
    assert!(p.category.is_none());
    assert!(p.documents.is_empty());
}

#[test]
fn test_search_params_full() {
    let p: SearchParams = serde_json::from_str(
        r#"{
            "query": "Director",
            "namespace": "USER",
            "regex": true,
            "case_sensitive": false,
            "category": "CLS",
            "documents": ["HS.FHIR.*.cls"]
        }"#,
    )
    .unwrap();
    assert_eq!(p.query, "Director");
    assert_eq!(p.namespace.as_deref(), Some("USER"));
    assert!(p.regex);
    assert!(!p.case_sensitive);
    assert_eq!(p.category.as_deref(), Some("CLS"));
    assert_eq!(p.documents, vec!["HS.FHIR.*.cls"]);
}

#[test]
fn test_search_params_case_sensitive_flag() {
    let p: SearchParams =
        serde_json::from_str(r#"{"query": "findMe", "case_sensitive": true}"#).unwrap();
    assert!(p.case_sensitive);
    assert!(!p.regex);
}

#[test]
fn test_search_params_custom_namespace() {
    let p: SearchParams =
        serde_json::from_str(r#"{"query": "foo", "namespace": "IRISAPP"}"#).unwrap();
    assert_eq!(p.namespace.as_deref(), Some("IRISAPP"));
}

#[test]
fn test_search_params_multiple_documents() {
    let p: SearchParams =
        serde_json::from_str(r#"{"query": "util", "documents": ["Pkg.A.*.cls", "Pkg.B.*.mac"]}"#)
            .unwrap();
    assert_eq!(p.documents.len(), 2);
    assert_eq!(p.documents[0], "Pkg.A.*.cls");
    assert_eq!(p.documents[1], "Pkg.B.*.mac");
}

#[test]
fn test_search_params_missing_query_fails() {
    let result: Result<SearchParams, _> = serde_json::from_str(r#"{"namespace": "USER"}"#);
    assert!(
        result.is_err(),
        "query is required — should fail without it"
    );
}
