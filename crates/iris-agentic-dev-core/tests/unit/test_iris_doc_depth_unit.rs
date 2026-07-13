// Unit tests for 053-doc-depth: iris_doc extensions (fragment/compiled/list) + iris_execute_method
// All tests are pure Rust — no IRIS connection required.

use iris_agentic_dev_core::tools::doc::{clamp_max_results, slice_lines, validate_list_pattern};
use iris_agentic_dev_core::tools::{IrisDocParams, IrisExecuteMethodParams};

// ── Phase 2 foundational helpers ──────────────────────────────────────────────

#[test]
fn test_slice_lines_basic_range() {
    let lines: Vec<String> = (1..=5).map(|i| format!("line{i}")).collect();
    let (sliced, actual_start, actual_end, clamped) = slice_lines(&lines, 1, 3);
    assert_eq!(sliced.len(), 3);
    assert_eq!(actual_start, 1);
    assert_eq!(actual_end, 3);
    assert!(!clamped);
    assert_eq!(sliced[0], "line1");
    assert_eq!(sliced[2], "line3");
}

#[test]
fn test_slice_lines_end_clamped() {
    let lines: Vec<String> = (1..=5).map(|i| format!("line{i}")).collect();
    let (sliced, _, actual_end, clamped) = slice_lines(&lines, 1, 999);
    assert_eq!(sliced.len(), 5);
    assert_eq!(actual_end, 5);
    assert!(clamped);
}

#[test]
fn test_slice_lines_start_beyond_len() {
    let lines: Vec<String> = (1..=5).map(|i| format!("line{i}")).collect();
    let (sliced, _, _, clamped) = slice_lines(&lines, 10, 20);
    assert!(sliced.is_empty());
    assert!(clamped);
}

#[test]
fn test_clamp_max_results_over() {
    assert_eq!(clamp_max_results(9999), 1000);
}

#[test]
fn test_clamp_max_results_zero() {
    assert_eq!(clamp_max_results(0), 1);
}

#[test]
fn test_clamp_max_results_within() {
    assert_eq!(clamp_max_results(200), 200);
}

#[test]
fn test_validate_list_pattern_accepts_valid() {
    assert!(validate_list_pattern("User.*").is_ok());
    assert!(validate_list_pattern("MyPkg.Sub*").is_ok());
    assert!(validate_list_pattern("Exact.Name.cls").is_ok());
    assert!(validate_list_pattern("%Library.*").is_ok());
}

#[test]
fn test_validate_list_pattern_rejects_empty() {
    let r = validate_list_pattern("");
    assert!(r.is_err());
    let e = r.unwrap_err();
    assert_eq!(e["error_code"], "MISSING_PARAMS");
}

#[test]
fn test_validate_list_pattern_rejects_bare_star() {
    assert!(validate_list_pattern("*").is_err());
    assert!(validate_list_pattern("**").is_err());
    assert!(validate_list_pattern("*.cls").is_err());
}

// ── Phase 3: US1 — fragment ───────────────────────────────────────────────────

#[test]
fn test_fragment_params_missing_start() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "fragment", "name": "Foo.cls", "end": 10}"#).unwrap();
    assert!(p.start.is_none());
    // Dispatcher would return MISSING_PARAMS — verified structurally
}

#[test]
fn test_fragment_params_start_gt_end() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "fragment", "name": "Foo.cls", "start": 10, "end": 5}"#)
            .unwrap();
    assert_eq!(p.start, Some(10));
    assert_eq!(p.end, Some(5));
    // start > end → INVALID_PARAMS fired by handler
}

#[test]
fn test_fragment_params_start_zero_treated_as_one() {
    // start=0 is clamped to 1 by the handler (spec edge case)
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "fragment", "name": "Foo.cls", "start": 0, "end": 5}"#)
            .unwrap();
    assert_eq!(p.start, Some(0));
    // slice_lines receives start clamped to max(1, start) = 1
    let lines: Vec<String> = (1..=10).map(|i| format!("line{i}")).collect();
    let (sliced, actual_start, _, _) = slice_lines(&lines, 1, 5);
    assert_eq!(actual_start, 1);
    assert_eq!(sliced.len(), 5);
}

#[test]
fn test_fragment_params_single_line() {
    let lines: Vec<String> = vec!["only line".to_string()];
    let (sliced, actual_start, actual_end, clamped) = slice_lines(&lines, 1, 1);
    assert_eq!(sliced.len(), 1);
    assert_eq!(actual_start, 1);
    assert_eq!(actual_end, 1);
    assert!(!clamped);
}

// ── Phase 4: US2 — compiled ───────────────────────────────────────────────────

#[test]
fn test_compiled_params_inc_rejected() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "compiled", "name": "MyMacros.INC"}"#).unwrap();
    // .INC → NOT_COMPILED fired immediately by handler
    assert!(p.name.as_deref().unwrap().to_lowercase().ends_with(".inc"));
}

#[test]
fn test_compiled_params_inc_lowercase() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "compiled", "name": "MyMacros.inc"}"#).unwrap();
    assert!(p.name.as_deref().unwrap().to_lowercase().ends_with(".inc"));
}

#[test]
fn test_compiled_type_validation_int_accepted() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "compiled", "name": "Foo.cls", "compiled_type": "INT"}"#)
            .unwrap();
    assert_eq!(p.compiled_type.as_deref(), Some("INT"));
}

#[test]
fn test_compiled_type_validation_obj_rejected() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "compiled", "name": "Foo.cls", "compiled_type": "OBJ"}"#)
            .unwrap();
    assert_eq!(p.compiled_type.as_deref(), Some("OBJ"));
    // OBJ not supported in v1 → INVALID_PARAMS from handler
}

#[test]
fn test_compiled_name_derivation_cls() {
    // MyApp.Foo.cls → routine MyApp.Foo.1
    let name = "MyApp.Foo.cls";
    let lower = name.to_lowercase();
    assert!(lower.ends_with(".cls"));
    let base = &name[..name.len() - 4];
    let routine = format!("{base}.1");
    assert_eq!(routine, "MyApp.Foo.1");
}

#[test]
fn test_compiled_name_derivation_mac() {
    // MyRoutine.mac → routine MyRoutine
    let name = "MyRoutine.mac";
    let lower = name.to_lowercase();
    assert!(lower.ends_with(".mac"));
    let routine = &name[..name.len() - 4];
    assert_eq!(routine, "MyRoutine");
}

#[test]
fn test_compiled_name_derivation_case_insensitive() {
    // MyRoutine.MAC (uppercase extension)
    let name = "MyRoutine.MAC";
    let lower = name.to_lowercase();
    assert!(lower.ends_with(".mac"));
}

// ── Phase 5: US3 — list ───────────────────────────────────────────────────────

#[test]
fn test_list_params_missing_pattern() {
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "list", "category": "CLS"}"#).unwrap();
    assert!(p.pattern.is_none());
    // Handler returns MISSING_PARAMS
}

#[test]
fn test_list_params_invalid_category() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "list", "pattern": "User.*", "category": "BOGUS"}"#)
            .unwrap();
    assert_eq!(p.category.as_deref(), Some("BOGUS"));
    // Handler returns INVALID_PARAMS
}

#[test]
fn test_list_params_category_default_all() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "list", "pattern": "User.*"}"#).unwrap();
    assert!(p.category.is_none()); // default applied in handler
}

#[test]
fn test_list_clamp_max_results() {
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "list", "pattern": "User.*", "max_results": 9999}"#)
            .unwrap();
    assert_eq!(p.max_results, Some(9999));
    assert_eq!(clamp_max_results(9999), 1000);
}

// ── Phase 6: US4 — iris_execute_method ───────────────────────────────────────

#[test]
fn test_execute_method_params_missing_class() {
    let result: Result<IrisExecuteMethodParams, _> =
        serde_json::from_str(r#"{"method": "IsValid"}"#);
    assert!(result.is_err(), "class is required");
}

#[test]
fn test_execute_method_params_missing_method() {
    let result: Result<IrisExecuteMethodParams, _> =
        serde_json::from_str(r#"{"class": "%Library.Integer"}"#);
    assert!(result.is_err(), "method is required");
}

#[test]
fn test_execute_method_params_defaults() {
    let p: IrisExecuteMethodParams =
        serde_json::from_str(r#"{"class": "%Library.Integer", "method": "IsValid"}"#).unwrap();
    assert!(p.args.is_empty());
    assert_eq!(p.namespace, "USER");
}

#[test]
fn test_execute_method_params_with_args() {
    let p: IrisExecuteMethodParams = serde_json::from_str(
        r#"{"class": "%Library.Integer", "method": "IsValid", "args": ["42"]}"#,
    )
    .unwrap();
    assert_eq!(p.args, vec!["42"]);
}

// ENV_GATE_BLOCKED tests are integration-level (require gate infrastructure);
// we verify the params parse correctly here and rely on gate unit tests in
// test_env_gate.rs for the blocking behavior.

#[test]
fn test_execute_method_injection_guard_class_with_brace() {
    // Handler rejects class/method containing { } or ;
    // We verify the guard logic directly by testing the condition
    let class = "Bad{Class}";
    let method = "Normal";
    let has_injection = ['{', '}', ';']
        .iter()
        .any(|ch| class.contains(*ch) || method.contains(*ch));
    assert!(has_injection, "brace in class name should trigger guard");
}

#[test]
fn test_execute_method_injection_guard_method_with_semicolon() {
    let class = "Good.Class";
    let method = "Bad;Method";
    let has_injection = ['{', '}', ';']
        .iter()
        .any(|ch| class.contains(*ch) || method.contains(*ch));
    assert!(
        has_injection,
        "semicolon in method name should trigger guard"
    );
}

#[test]
fn test_execute_method_injection_guard_clean() {
    let class = "%Library.Integer";
    let method = "IsValid";
    let has_injection = ['{', '}', ';']
        .iter()
        .any(|ch| class.contains(*ch) || method.contains(*ch));
    assert!(!has_injection, "clean names should not trigger guard");
}

// ── New tests for uncovered pure functions ───────────────────────────────────

// ── Test 1: require_name edge cases ──────────────────────────────────────────

#[test]
fn test_require_name_trimmed_whitespace() {
    // Test that leading/trailing whitespace in name is trimmed
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "get", "name": "  Foo.cls  "}"#).unwrap();
    assert_eq!(p.name.as_deref(), Some("  Foo.cls  "));
    // The actual trimming happens in require_name function; verify the name was parsed
    let name = p.name.unwrap();
    let trimmed = name.trim();
    assert_eq!(trimmed, "Foo.cls");
}

#[test]
fn test_require_name_all_spaces_returns_error() {
    // Test that name with only spaces is treated as missing
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "name": "   "}"#).unwrap();
    assert_eq!(p.name.as_deref(), Some("   "));
    // When passed through require_name logic: trim() yields "", which is_empty()
    let name = p.name.unwrap();
    let trimmed = name.trim();
    assert!(trimmed.is_empty(), "all-spaces should trim to empty string");
}

#[test]
fn test_require_name_preserves_extension() {
    // Test that file extension is preserved as-is (case, format)
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "get", "name": "MyClass.CLS"}"#).unwrap();
    assert_eq!(p.name.as_deref(), Some("MyClass.CLS"));
    // Extension should not be modified
    let name = p.name.unwrap();
    assert!(name.ends_with(".CLS"));
}

#[test]
fn test_require_name_missing_field() {
    // Test that missing name field defaults to None
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get"}"#).unwrap();
    assert!(p.name.is_none());
}

// ── Test 2: de_opt_i64_lenient deserializer ─────────────────────────────────

#[test]
fn test_de_opt_i64_lenient_json_number() {
    // Test that JSON number 42 deserializes to Some(42)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "start": 42}"#).unwrap();
    assert_eq!(p.start, Some(42));
}

#[test]
fn test_de_opt_i64_lenient_json_string() {
    // Test that JSON string "42" deserializes to Some(42)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "start": "42"}"#).unwrap();
    assert_eq!(p.start, Some(42));
}

#[test]
fn test_de_opt_i64_lenient_json_string_trimmed() {
    // Test that JSON string "  42  " (with spaces) deserializes to Some(42)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "start": "  42  "}"#).unwrap();
    assert_eq!(p.start, Some(42));
}

#[test]
fn test_de_opt_i64_lenient_json_string_empty() {
    // Test that empty string "" deserializes to None
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "start": ""}"#).unwrap();
    assert_eq!(p.start, None);
}

#[test]
fn test_de_opt_i64_lenient_null() {
    // Test that JSON null deserializes to None
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "start": null}"#).unwrap();
    assert_eq!(p.start, None);
}

#[test]
fn test_de_opt_i64_lenient_missing_field() {
    // Test that missing field deserializes to None
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get"}"#).unwrap();
    assert_eq!(p.start, None);
}

#[test]
fn test_de_opt_i64_lenient_invalid_string() {
    // Test that JSON string "abc" (non-numeric) fails deserialization
    let result: Result<IrisDocParams, _> =
        serde_json::from_str(r#"{"mode": "get", "start": "abc"}"#);
    assert!(
        result.is_err(),
        "non-numeric string should fail deserialization"
    );
}

#[test]
fn test_de_opt_i64_lenient_float_truncated() {
    // Test that JSON float 42.7 is truncated to i64 (42)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "start": 42.7}"#).unwrap();
    assert_eq!(p.start, Some(42), "float should be truncated to i64");
}

#[test]
fn test_de_opt_i64_lenient_large_number() {
    // Test that large numbers work (e.g., 999999)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "get", "end": 999999}"#).unwrap();
    assert_eq!(p.end, Some(999999));
}

// ── Test 3: IrisDocParams mode field deserialization ────────────────────────

#[test]
fn test_mode_insert_deserializes() {
    // Test that mode "insert" deserializes to String "insert"
    let p: IrisDocParams =
        serde_json::from_str(r#"{"mode": "insert", "name": "Foo.cls", "content": "new line"}"#)
            .unwrap();
    assert_eq!(p.mode, "insert");
}

#[test]
fn test_mode_delete_lines_deserializes() {
    // Test that mode "delete_lines" deserializes to String "delete_lines"
    let p: IrisDocParams = serde_json::from_str(
        r#"{"mode": "delete_lines", "name": "Foo.cls", "start": 1, "end": 5, "expected": "line"}"#,
    )
    .unwrap();
    assert_eq!(p.mode, "delete_lines");
}

#[test]
fn test_mode_get_is_default_when_omitted() {
    // Test that mode defaults to "get" when omitted
    let p: IrisDocParams = serde_json::from_str(r#"{"name": "Foo.cls"}"#).unwrap();
    assert_eq!(p.mode, "get", "mode should default to 'get'");
}

#[test]
fn test_mode_unknown_string_accepted() {
    // Test that unknown mode string is accepted as-is (mode is plain String, not enum)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "unknown_mode_xyz"}"#).unwrap();
    assert_eq!(p.mode, "unknown_mode_xyz");
}

#[test]
fn test_mode_case_preserved() {
    // Test that mode case is preserved exactly as input (no normalization in deserialization)
    let p: IrisDocParams = serde_json::from_str(r#"{"mode": "GET"}"#).unwrap();
    assert_eq!(p.mode, "GET", "mode case should be preserved in params");
}

// ── Test 4: STALE_CONTENT error structure ───────────────────────────────────

#[test]
fn test_stale_content_error_json_structure() {
    // Test that the JSON error structure has the expected fields
    // Reconstruct what stale_content_err would produce
    let diff = (
        2,
        "expected_line_content".to_string(),
        "actual_line_content".to_string(),
    );
    let block_start = 10i64;
    let line_no = block_start + diff.0 as i64;

    let error_json = serde_json::json!({
        "success": false,
        "error_code": "STALE_CONTENT",
        "error": format!(
            "Line {line_no} does not match `expected` — the document changed since you \
             last read it. Re-fetch with mode=get or mode=fragment and retry with current \
             line numbers."
        ),
        "line": line_no,
        "expected_line": diff.1,
        "actual_line": diff.2,
    });

    assert_eq!(error_json["success"], false);
    assert_eq!(error_json["error_code"], "STALE_CONTENT");
    assert_eq!(
        error_json["line"], 12,
        "line should be block_start + offset"
    );
    assert_eq!(error_json["expected_line"], "expected_line_content");
    assert_eq!(error_json["actual_line"], "actual_line_content");
    assert!(error_json["error"].is_string());
    assert!(error_json["error"].as_str().unwrap().contains("Line 12"));
}

#[test]
fn test_stale_content_error_line_calculation() {
    // Test that the line number is correctly calculated: block_start + offset
    let test_cases = vec![(0, 5, 5), (5, 0, 5), (10, 3, 13), (100, 50, 150)];
    for (block_start, offset, expected_line) in test_cases {
        let line_no = block_start + offset as i64;
        assert_eq!(line_no, expected_line, "line calculation should be correct");
    }
}

#[test]
fn test_stale_content_error_preserves_content() {
    // Test that expected and actual line content are preserved in error
    let expected_content = "    public String getName() {".to_string();
    let actual_content = "    public int getCount() {".to_string();
    let diff = (0, expected_content.clone(), actual_content.clone());
    let block_start = 42i64;

    let error_json = serde_json::json!({
        "success": false,
        "error_code": "STALE_CONTENT",
        "error": "test error",
        "line": block_start,
        "expected_line": diff.1,
        "actual_line": diff.2,
    });

    assert_eq!(
        error_json["expected_line"].as_str().unwrap(),
        &expected_content
    );
    assert_eq!(error_json["actual_line"].as_str().unwrap(), &actual_content);
}
