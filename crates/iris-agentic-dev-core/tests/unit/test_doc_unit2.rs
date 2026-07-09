//! Unit tests for doc.rs — IrisDocParams serde.

use iris_agentic_dev_core::tools::doc::IrisDocParams;

#[test]
fn doc_params_defaults() {
    let p: IrisDocParams = serde_json::from_str(r#"{"name":"Foo.Bar.cls"}"#).unwrap();
    assert_eq!(p.name.as_deref(), Some("Foo.Bar.cls"));
    assert!(p.mode == "get");
    assert!(!p.compile);
    assert_eq!(p.namespace, "USER");
}

#[test]
fn doc_params_put_mode_lowercase() {
    let p: IrisDocParams = serde_json::from_str(r#"{"name":"x.cls","mode":"put"}"#).unwrap();
    assert!(p.mode == "put");
}

#[test]
fn doc_params_action_alias_delete() {
    let p: IrisDocParams = serde_json::from_str(r#"{"name":"x.cls","action":"delete"}"#).unwrap();
    assert!(p.mode == "delete");
}

#[test]
fn doc_params_compile_flag() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"name":"x.cls","mode":"put","compile":true}"#).unwrap();
    assert!(p.compile);
}

#[test]
fn doc_params_names_list() {
    let p: IrisDocParams = serde_json::from_str(r#"{"names":["a.cls","b.cls"]}"#).unwrap();
    assert_eq!(p.names.len(), 2);
}

#[test]
fn doc_mode_head_lowercase() {
    let p: IrisDocParams = serde_json::from_str(r#"{"name":"x.cls","mode":"head"}"#).unwrap();
    assert!(p.mode == "head");
}
