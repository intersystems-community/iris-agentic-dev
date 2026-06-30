//! Unit tests for info.rs — InfoParams, MacroParams, GenerateParams, TableInfoParams serde.

use iris_agentic_dev_core::tools::info::{
    DebugParams, GenerateParams, InfoParams, MacroParams, TableInfoParams,
};

#[test]
fn info_params_defaults() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"documents"}"#).unwrap();
    assert_eq!(p.what, "documents");
    assert!(p.doc_type.is_none());
    assert_eq!(p.namespace, "USER");
    assert!(!p.inline);
}

#[test]
fn info_params_with_doc_type() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"documents","doc_type":"CLS"}"#).unwrap();
    assert_eq!(p.doc_type.as_deref(), Some("CLS"));
}

#[test]
fn info_params_missing_what_fails() {
    let r: Result<InfoParams, _> = serde_json::from_str(r#"{}"#);
    assert!(r.is_err(), "what is required");
}

#[test]
fn macro_params_defaults() {
    let p: MacroParams = serde_json::from_str(r#"{"action":"list"}"#).unwrap();
    assert_eq!(p.action, "list");
    assert!(p.name.is_none());
    assert!(p.args.is_empty());
    assert_eq!(p.namespace, "USER");
}

#[test]
fn macro_params_with_name_and_args() {
    let p: MacroParams =
        serde_json::from_str(r#"{"action":"expand","name":"MYCLASS","args":["arg1"]}"#).unwrap();
    assert_eq!(p.name.as_deref(), Some("MYCLASS"));
    assert_eq!(p.args.len(), 1);
}

#[test]
fn generate_params_defaults_gen_type_class() {
    let p: GenerateParams = serde_json::from_str(r#"{"description":"a simple class"}"#).unwrap();
    assert_eq!(p.gen_type, "class");
    assert!(p.class_name.is_none());
    assert_eq!(p.namespace, "USER");
}

#[test]
fn generate_params_test_type() {
    let p: GenerateParams = serde_json::from_str(
        r#"{"description":"tests for Foo","gen_type":"test","class_name":"Foo.Bar"}"#,
    )
    .unwrap();
    assert_eq!(p.gen_type, "test");
    assert_eq!(p.class_name.as_deref(), Some("Foo.Bar"));
}

#[test]
fn table_info_params_defaults() {
    let p: TableInfoParams = serde_json::from_str(r#"{"table":"SQLUser.MyTable"}"#).unwrap();
    assert_eq!(p.table, "SQLUser.MyTable");
    assert_eq!(p.namespace, "USER");
    assert!(!p.include_row_count);
}

#[test]
fn table_info_params_with_row_count() {
    let p: TableInfoParams =
        serde_json::from_str(r#"{"table":"App.Orders","include_row_count":true}"#).unwrap();
    assert!(p.include_row_count);
}

#[test]
fn table_info_params_missing_table_fails() {
    let r: Result<TableInfoParams, _> = serde_json::from_str(r#"{}"#);
    assert!(r.is_err(), "table is required");
}

// ── InfoParams edge cases ────────────────────────────────────────────────────

#[test]
fn info_params_with_all_fields() {
    let p: InfoParams = serde_json::from_str(
        r#"{"what":"documents","doc_type":"MAC","name":"Foo","namespace":"SYS","inline":true}"#,
    )
    .unwrap();
    assert_eq!(p.what, "documents");
    assert_eq!(p.doc_type.as_deref(), Some("MAC"));
    assert_eq!(p.name.as_deref(), Some("Foo"));
    assert_eq!(p.namespace, "SYS");
    assert!(p.inline);
}

#[test]
fn info_params_doc_type_all_option() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"documents","doc_type":"ALL"}"#).unwrap();
    assert_eq!(p.doc_type.as_deref(), Some("ALL"));
}

#[test]
fn info_params_what_metadata() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"metadata"}"#).unwrap();
    assert_eq!(p.what, "metadata");
}

#[test]
fn info_params_what_namespace() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"namespace"}"#).unwrap();
    assert_eq!(p.what, "namespace");
}

#[test]
fn info_params_what_jobs() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"jobs"}"#).unwrap();
    assert_eq!(p.what, "jobs");
}

#[test]
fn info_params_what_csp_apps() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"csp_apps"}"#).unwrap();
    assert_eq!(p.what, "csp_apps");
}

#[test]
fn info_params_what_csp_debug() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"csp_debug"}"#).unwrap();
    assert_eq!(p.what, "csp_debug");
}

#[test]
fn info_params_what_modified() {
    let p: InfoParams = serde_json::from_str(r#"{"what":"modified"}"#).unwrap();
    assert_eq!(p.what, "modified");
}

#[test]
fn info_params_what_sa_schema() {
    let p: InfoParams =
        serde_json::from_str(r#"{"what":"sa_schema","name":"SQLUser.Orders"}"#).unwrap();
    assert_eq!(p.what, "sa_schema");
    assert_eq!(p.name.as_deref(), Some("SQLUser.Orders"));
}

// ── MacroParams edge cases ───────────────────────────────────────────────────

#[test]
fn macro_params_action_signature() {
    let p: MacroParams =
        serde_json::from_str(r#"{"action":"signature","name":"MYMACRO"}"#).unwrap();
    assert_eq!(p.action, "signature");
}

#[test]
fn macro_params_action_location() {
    let p: MacroParams = serde_json::from_str(r#"{"action":"location","name":"MYMACRO"}"#).unwrap();
    assert_eq!(p.action, "location");
}

#[test]
fn macro_params_action_definition() {
    let p: MacroParams =
        serde_json::from_str(r#"{"action":"definition","name":"MYMACRO"}"#).unwrap();
    assert_eq!(p.action, "definition");
}

#[test]
fn macro_params_action_expand() {
    let p: MacroParams = serde_json::from_str(r#"{"action":"expand","name":"MYMACRO"}"#).unwrap();
    assert_eq!(p.action, "expand");
}

#[test]
fn macro_params_with_multiple_args() {
    let p: MacroParams =
        serde_json::from_str(r#"{"action":"expand","name":"MYMACRO","args":["a","b","c"]}"#)
            .unwrap();
    assert_eq!(p.args.len(), 3);
    assert_eq!(p.args[0], "a");
}

#[test]
fn macro_params_custom_namespace() {
    let p: MacroParams = serde_json::from_str(r#"{"action":"list","namespace":"SYS"}"#).unwrap();
    assert_eq!(p.namespace, "SYS");
}

// ── DebugParams edge cases ───────────────────────────────────────────────────

#[test]
fn debug_params_action_map_int() {
    let p: DebugParams =
        serde_json::from_str(r#"{"action":"map_int","error_string":"<UNDEFINED>x"}"#).unwrap();
    assert_eq!(p.action, "map_int");
    assert_eq!(p.error_string.as_deref(), Some("<UNDEFINED>x"));
}

#[test]
fn debug_params_action_error_logs() {
    let p: DebugParams = serde_json::from_str(r#"{"action":"error_logs"}"#).unwrap();
    assert_eq!(p.action, "error_logs");
}

#[test]
fn debug_params_action_capture() {
    let p: DebugParams = serde_json::from_str(r#"{"action":"capture"}"#).unwrap();
    assert_eq!(p.action, "capture");
}

#[test]
fn debug_params_action_source_map() {
    let p: DebugParams =
        serde_json::from_str(r#"{"action":"source_map","class_name":"MyApp.Foo"}"#).unwrap();
    assert_eq!(p.action, "source_map");
    assert_eq!(p.class_name.as_deref(), Some("MyApp.Foo"));
}

#[test]
fn debug_params_custom_limit() {
    let p: DebugParams = serde_json::from_str(r#"{"action":"error_logs","limit":100}"#).unwrap();
    assert_eq!(p.limit, 100);
}

#[test]
fn debug_params_custom_namespace() {
    let p: DebugParams =
        serde_json::from_str(r#"{"action":"error_logs","namespace":"TEST"}"#).unwrap();
    assert_eq!(p.namespace, "TEST");
}

// ── GenerateParams edge cases ────────────────────────────────────────────────

#[test]
fn generate_params_with_custom_namespace() {
    let p: GenerateParams =
        serde_json::from_str(r#"{"description":"a class","namespace":"CUSTOM"}"#).unwrap();
    assert_eq!(p.namespace, "CUSTOM");
}

#[test]
fn generate_params_test_with_namespace() {
    let p: GenerateParams = serde_json::from_str(
        r#"{"description":"tests","gen_type":"test","class_name":"Foo.Bar","namespace":"SYS"}"#,
    )
    .unwrap();
    assert_eq!(p.gen_type, "test");
    assert_eq!(p.namespace, "SYS");
}

// ── TableInfoParams edge cases ───────────────────────────────────────────────

#[test]
fn table_info_params_with_dot_notation() {
    let p: TableInfoParams = serde_json::from_str(r#"{"table":"Schema.Table"}"#).unwrap();
    assert_eq!(p.table, "Schema.Table");
}

#[test]
fn table_info_params_custom_namespace() {
    let p: TableInfoParams =
        serde_json::from_str(r#"{"table":"MyTable","namespace":"SYS"}"#).unwrap();
    assert_eq!(p.namespace, "SYS");
}

#[test]
fn table_info_params_with_row_count_false() {
    let p: TableInfoParams =
        serde_json::from_str(r#"{"table":"T1","include_row_count":false}"#).unwrap();
    assert!(!p.include_row_count);
}
