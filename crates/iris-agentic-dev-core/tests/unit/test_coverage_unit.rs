//! Unit tests for iris_coverage tool — no IRIS connection required.

use iris_agentic_dev_core::tools::coverage::{
    build_coverage_check_code, build_coverage_run_code, build_routine_name, parse_check_output,
    parse_coverage_output, strip_routine_suffix,
};

// ── build_routine_name ────────────────────────────────────────────────────────

#[test]
fn routine_name_appends_dot_one() {
    assert_eq!(build_routine_name("MyApp.MyClass"), "MyApp.MyClass.1");
}

#[test]
fn routine_name_deep_package() {
    assert_eq!(
        build_routine_name("MyApp.Deep.Sub.Class"),
        "MyApp.Deep.Sub.Class.1"
    );
}

#[test]
fn routine_name_single_segment() {
    assert_eq!(build_routine_name("Foo"), "Foo.1");
}

// ── strip_routine_suffix ──────────────────────────────────────────────────────

#[test]
fn strip_suffix_removes_dot_one() {
    assert_eq!(strip_routine_suffix("MyApp.MyClass.1"), "MyApp.MyClass");
}

#[test]
fn strip_suffix_deep() {
    assert_eq!(
        strip_routine_suffix("MyApp.Deep.Class.1"),
        "MyApp.Deep.Class"
    );
}

#[test]
fn strip_suffix_no_suffix_unchanged() {
    assert_eq!(strip_routine_suffix("MyApp.MyClass"), "MyApp.MyClass");
}

// ── build_coverage_run_code ───────────────────────────────────────────────────

#[test]
fn run_code_contains_start_and_stop() {
    let code = build_coverage_run_code(&["MyApp.ClassA.1".to_string()], "MyApp.Tests", "USER");
    assert!(code.contains("LineByLine"), "missing LineByLine: {code}");
    assert!(code.contains("Start"), "missing Start: {code}");
    assert!(code.contains("Stop"), "missing Stop: {code}");
    assert!(code.contains("RunTest"), "missing RunTest: {code}");
    assert!(code.contains("/noload"), "missing /noload: {code}");
    assert!(code.contains("MyApp.Tests"), "missing test path: {code}");
}

#[test]
fn run_code_contains_routine_names() {
    let code = build_coverage_run_code(
        &["MyApp.ClassA.1".to_string(), "MyApp.ClassB.1".to_string()],
        "MyApp.Tests",
        "USER",
    );
    assert!(code.contains("MyApp.ClassA.1"), "missing ClassA: {code}");
    assert!(code.contains("MyApp.ClassB.1"), "missing ClassB: {code}");
}

#[test]
fn run_code_namespace_is_set() {
    let code = build_coverage_run_code(&["Foo.1".to_string()], "Foo.Tests", "MYNAMESPACE");
    assert!(code.contains("MYNAMESPACE"), "missing namespace: {code}");
}

// ── build_coverage_check_code ─────────────────────────────────────────────────

#[test]
fn check_code_calls_linebyline_start() {
    let code = build_coverage_check_code("USER");
    assert!(code.contains("LineByLine"), "missing LineByLine: {code}");
    assert!(code.contains("Start"), "missing Start: {code}");
    assert!(code.contains("Stop"), "missing Stop: {code}");
    assert!(code.contains("OK|ready"), "missing OK|ready: {code}");
    assert!(
        code.contains("BBSIZ_NOT_CONFIGURED"),
        "missing BBSIZ_NOT_CONFIGURED: {code}"
    );
}

// ── parse_coverage_output ─────────────────────────────────────────────────────

#[test]
fn parse_valid_coverage_json() {
    let json = r#"{"success":true,"total_pct":75.0,"hits":15,"total":20,"meets_target":false,"target_pct":90.0,"classes":[{"class":"MyApp.MyClass","routine":"MyApp.MyClass.1","hit":15,"total":20,"pct":75.0}]}"#;
    let v = parse_coverage_output(json);
    assert_eq!(v["success"], true);
    assert_eq!(v["hits"], 15);
    assert_eq!(v["total"], 20);
    assert_eq!(v["classes"].as_array().unwrap().len(), 1);
}

#[test]
fn parse_error_json_preserves_error_code() {
    let json = r#"{"error_code":"MONITOR_IN_USE","message":"Somebody else is using the Monitor"}"#;
    let v = parse_coverage_output(json);
    assert_eq!(v["error_code"], "MONITOR_IN_USE");
}

#[test]
fn parse_empty_output_returns_error() {
    let v = parse_coverage_output("");
    assert_eq!(v["success"], false);
    assert!(v["error_code"].as_str().is_some());
}

#[test]
fn parse_whitespace_only_returns_error() {
    let v = parse_coverage_output("   \n  ");
    assert_eq!(v["success"], false);
}

#[test]
fn parse_invalid_json_returns_error() {
    let v = parse_coverage_output("not json at all");
    assert_eq!(v["success"], false);
    assert_eq!(v["error_code"], "PARSE_ERROR");
}

// ── parse_check_output ────────────────────────────────────────────────────────

#[test]
fn check_output_ok_returns_ready() {
    let json = r#"{"ok":true,"bbsiz_state":"ready"}"#;
    let v = parse_check_output(json);
    assert_eq!(v["ok"], true);
    assert_eq!(v["bbsiz_state"], "ready");
}

#[test]
fn check_output_function_error_returns_bbsiz_not_configured() {
    // IRIS writes <FUNCTION> to stdout when $zu(84) is not available
    let output =
        r#"{"error_code":"BBSIZ_NOT_CONFIGURED","message":"$zu(84) subsystem not available"}"#;
    let v = parse_check_output(output);
    assert_eq!(v["error_code"], "BBSIZ_NOT_CONFIGURED");
}

#[test]
fn check_output_empty_returns_error() {
    let v = parse_check_output("");
    assert_eq!(v["success"], false);
}

// ── MISSING_PARAM validation (tested via IrisCoverageParams deserialization) ──

#[test]
fn coverage_params_mode_run_requires_test_path() {
    use iris_agentic_dev_core::tools::coverage::IrisCoverageParams;
    let v: Result<IrisCoverageParams, _> = serde_json::from_value(serde_json::json!({
        "mode": "run",
        "classes": ["MyApp.MyClass"]
        // test_path missing
    }));
    // Deserialization succeeds (test_path is Option); validation happens at handler time
    assert!(v.is_ok());
    let p = v.unwrap();
    assert_eq!(p.test_path, None);
}

#[test]
fn coverage_params_mode_check_no_classes_required() {
    use iris_agentic_dev_core::tools::coverage::IrisCoverageParams;
    let v: Result<IrisCoverageParams, _> = serde_json::from_value(serde_json::json!({
        "mode": "check"
    }));
    assert!(v.is_ok());
}

#[test]
fn coverage_params_deserializes_package() {
    use iris_agentic_dev_core::tools::coverage::IrisCoverageParams;
    let v: Result<IrisCoverageParams, _> = serde_json::from_value(serde_json::json!({
        "mode": "run",
        "package": "MyApp",
        "test_path": "MyApp.Tests"
    }));
    assert!(v.is_ok());
    let p = v.unwrap();
    assert_eq!(p.package.as_deref(), Some("MyApp"));
    assert_eq!(p.classes, None);
}

#[test]
fn coverage_params_target_pct_defaults_to_none() {
    use iris_agentic_dev_core::tools::coverage::IrisCoverageParams;
    let v: Result<IrisCoverageParams, _> = serde_json::from_value(serde_json::json!({
        "mode": "check"
    }));
    let p = v.unwrap();
    assert_eq!(p.target_pct, None);
}
