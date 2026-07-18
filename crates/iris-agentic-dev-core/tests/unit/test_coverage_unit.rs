//! Unit tests for iris_coverage tool — no IRIS connection required.

use iris_agentic_dev_core::tools::coverage::{
    build_coverage_check_code, build_coverage_run_code, build_package_expand_code,
    build_routine_name, parse_check_output, parse_coverage_output, parse_package_expand_output,
    strip_routine_suffix,
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

// ── cobertura_path ────────────────────────────────────────────────────────────

#[test]
fn coverage_params_cobertura_path_optional() {
    use iris_agentic_dev_core::tools::coverage::IrisCoverageParams;
    let v: Result<IrisCoverageParams, _> = serde_json::from_value(serde_json::json!({
        "mode": "run",
        "classes": ["MyApp.MyClass"],
        "test_path": "MyApp.Tests",
        "cobertura_path": "/tmp/coverage.xml"
    }));
    assert!(v.is_ok());
    let p = v.unwrap();
    assert_eq!(p.cobertura_path.as_deref(), Some("/tmp/coverage.xml"));
}

#[test]
fn coverage_params_cobertura_path_absent_is_none() {
    use iris_agentic_dev_core::tools::coverage::IrisCoverageParams;
    let v: Result<IrisCoverageParams, _> = serde_json::from_value(serde_json::json!({
        "mode": "check"
    }));
    let p = v.unwrap();
    assert!(p.cobertura_path.is_none());
}

// ── parse_package_expand_output ───────────────────────────────────────────────

#[test]
fn package_expand_parses_class_list() {
    let output = "MyApp.ClassA\nMyApp.ClassB\nMyApp.ClassC\nDONE|3\n";
    let result = parse_package_expand_output(output);
    assert!(result.is_ok());
    let classes = result.unwrap();
    assert_eq!(
        classes,
        vec!["MyApp.ClassA", "MyApp.ClassB", "MyApp.ClassC"]
    );
}

#[test]
fn package_expand_empty_output_returns_error() {
    let result = parse_package_expand_output("");
    assert!(result.is_err());
}

#[test]
fn package_expand_error_line_returns_err() {
    let output = "ERROR|SQL_ERROR|SQL prepare failed\n";
    let result = parse_package_expand_output(output);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err["error_code"], "SQL_ERROR");
}

#[test]
fn package_expand_empty_package_returns_empty_vec() {
    let output = "DONE|0\n";
    let result = parse_package_expand_output(output);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

// ── pipe-delimited coverage output parsing ────────────────────────────────────

#[test]
fn parse_coverage_output_pipe_delimited() {
    // With sentinel
    let output = "COVERAGE_DATA_START\nMyApp.ClassA|MyApp.ClassA.1|45|60\nTOTAL|45|60\n";
    let v = parse_coverage_output(output);
    assert_eq!(v["success"], true);
    assert_eq!(v["hits"], 45);
    assert_eq!(v["total"], 60);
    let classes = v["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["class"], "MyApp.ClassA");
    assert_eq!(classes[0]["hit"], 45);
    assert_eq!(classes[0]["total"], 60);
    let pct = classes[0]["pct"].as_f64().unwrap();
    assert!((pct - 75.0).abs() < 0.2, "pct {pct} should be ~75");
}

#[test]
fn parse_coverage_output_with_runtest_preamble() {
    // RunTest stdout mixed in before sentinel — must be ignored
    let output = "  TestFoo begins ...\n  TestFoo PASSED\nAll PASSEDCOVERAGE_DATA_START\nMyApp.ClassA|MyApp.ClassA.1|45|60\nTOTAL|45|60\n";
    let v = parse_coverage_output(output);
    assert_eq!(v["success"], true);
    assert_eq!(v["hits"], 45);
    // class name must not include RunTest preamble
    let class = v["classes"][0]["class"].as_str().unwrap_or("");
    assert_eq!(
        class, "MyApp.ClassA",
        "class name must not include RunTest output: {v}"
    );
}

#[test]
fn parse_coverage_output_error_line() {
    let output = "ERROR|MONITOR_IN_USE|monitor already running\n";
    let v = parse_coverage_output(output);
    assert_eq!(v["success"], false);
    assert_eq!(v["error_code"], "MONITOR_IN_USE");
}

#[test]
fn parse_coverage_output_multiple_classes() {
    let output =
        "COVERAGE_DATA_START\nMyApp.A|MyApp.A.1|10|20\nMyApp.B|MyApp.B.1|15|15\nTOTAL|25|35\n";
    let v = parse_coverage_output(output);
    assert_eq!(v["success"], true);
    assert_eq!(v["hits"], 25);
    assert_eq!(v["total"], 35);
    assert_eq!(v["classes"].as_array().unwrap().len(), 2);
}

// ── parse_check_output pipe-delimited ────────────────────────────────────────

#[test]
fn check_output_pipe_ok_returns_ready() {
    let v = parse_check_output("OK|ready\n");
    assert_eq!(v["ok"], true);
    assert_eq!(v["bbsiz_state"], "ready");
}

#[test]
fn check_output_pipe_bbsiz_not_configured() {
    let v = parse_check_output("BBSIZ_NOT_CONFIGURED|Start() failed — increase gmheap");
    assert_eq!(v["error_code"], "BBSIZ_NOT_CONFIGURED");
    assert!(
        v["fix"].as_str().is_some(),
        "should include fix instructions"
    );
}

// ── build_package_expand_code ─────────────────────────────────────────────────

#[test]
fn package_expand_code_uses_dot_prefix_not_percent() {
    // Regression: old code used "Package.%" as prefix, which IRIS %STARTSWITH interprets
    // as a literal percent sign — no classes match. Correct prefix is "Package.".
    let code = build_package_expand_code("MyApp", "USER");
    assert!(
        code.contains("MyApp."),
        "SQL should use 'MyApp.' as prefix: {code}"
    );
    assert!(
        !code.contains("MyApp.%"),
        "SQL must NOT use 'MyApp.%' (literal percent) as prefix: {code}"
    );
}

#[test]
fn package_expand_code_uses_startswith() {
    let code = build_package_expand_code("MyApp", "USER");
    assert!(code.contains("%STARTSWITH"), "must use %STARTSWITH: {code}");
    assert!(code.contains("Abstract"), "must filter Abstract=0: {code}");
}

#[test]
fn package_expand_code_sets_namespace() {
    let code = build_package_expand_code("MyApp", "MYNAMESPACE");
    assert!(code.contains("MYNAMESPACE"), "must set namespace: {code}");
}
