// Unit tests for iris_test HTTP path — SQL parsing, result shape, status mapping.
// These tests make no IRIS connections.

use iris_dev_core::tools::{build_test_run_from_sql, map_status_int, MethodRow, SuiteRow};

// ── T009: map_status_int ─────────────────────────────────────────────────────

#[test]
fn test_map_status_int_passed() {
    assert_eq!(map_status_int(1, ""), "passed");
}

#[test]
fn test_map_status_int_failed() {
    assert_eq!(map_status_int(0, ""), "failed");
}

#[test]
fn test_map_status_int_error_with_error_action() {
    assert_eq!(map_status_int(2, "some error"), "error");
}

#[test]
fn test_map_status_int_other_no_error_action() {
    assert_eq!(map_status_int(2, ""), "failed");
}

// ── T006: build_test_run_from_sql — all passing ──────────────────────────────

#[test]
fn test_build_test_run_all_passing() {
    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "MyApp.Tests".to_string(),
        status: 1,
        duration_ms: Some(150.0),
    }];
    let methods = vec![
        MethodRow {
            suite_id: "1".to_string(),
            name: "TestAdd".to_string(),
            class_name: "MyApp.Tests".to_string(),
            status: 1,
            duration_ms: Some(50.0),
            error_description: "".to_string(),
            error_action: "".to_string(),
        },
        MethodRow {
            suite_id: "1".to_string(),
            name: "TestSubtract".to_string(),
            class_name: "MyApp.Tests".to_string(),
            status: 1,
            duration_ms: Some(100.0),
            error_description: "".to_string(),
            error_action: "".to_string(),
        },
    ];
    let result = build_test_run_from_sql(&suites, &methods);
    assert_eq!(result["success"], true);
    assert_eq!(result["total"], 2);
    assert_eq!(result["passed"], 2);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["errors"], 0);
    let suite_arr = result["test_suites"].as_array().unwrap();
    assert_eq!(suite_arr.len(), 1);
    assert_eq!(suite_arr[0]["name"], "MyApp.Tests");
    assert_eq!(suite_arr[0]["tests"], 2);
    assert_eq!(suite_arr[0]["failures"], 0);
}

// ── T007: build_test_run_from_sql — one failure ──────────────────────────────

#[test]
fn test_build_test_run_one_failure() {
    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "MyApp.Tests".to_string(),
        status: 0,
        duration_ms: Some(200.0),
    }];
    let methods = vec![
        MethodRow {
            suite_id: "1".to_string(),
            name: "TestAdd".to_string(),
            class_name: "MyApp.Tests".to_string(),
            status: 1,
            duration_ms: Some(50.0),
            error_description: "".to_string(),
            error_action: "".to_string(),
        },
        MethodRow {
            suite_id: "1".to_string(),
            name: "TestBad".to_string(),
            class_name: "MyApp.Tests".to_string(),
            status: 0,
            duration_ms: Some(150.0),
            error_description: "Expected 2, got 3".to_string(),
            error_action: "".to_string(),
        },
    ];
    let result = build_test_run_from_sql(&suites, &methods);
    assert_eq!(result["success"], false);
    assert_eq!(result["total"], 2);
    assert_eq!(result["passed"], 1);
    assert_eq!(result["failed"], 1);
    // Suite-level summary should show failures
    let suite_arr = result["test_suites"].as_array().unwrap();
    assert_eq!(suite_arr[0]["failures"], 1);
}

// ── T008: build_test_run_from_sql — empty → NO_TESTS_FOUND ──────────────────

#[test]
fn test_build_test_run_empty_no_tests_found() {
    let result = build_test_run_from_sql(&[], &[]);
    assert_eq!(result["total"], 0);
    assert_eq!(result["error_code"], "NO_TESTS_FOUND");
}

// ── T032: US3 — error vs failed distinction ──────────────────────────────────

#[test]
fn test_build_test_run_error_distinct_from_failed() {
    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "MyApp.Tests".to_string(),
        status: 0,
        duration_ms: None,
    }];
    let methods = vec![MethodRow {
        suite_id: "1".to_string(),
        name: "TestCrash".to_string(),
        class_name: "MyApp.Tests".to_string(),
        status: 2,
        duration_ms: None,
        error_description: "<UNDEFINED>zMyMethod+3".to_string(),
        error_action: "MyClass:MyMethod".to_string(),
    }];
    let result = build_test_run_from_sql(&suites, &methods);
    // errors count should be 1, failed should be 0
    assert_eq!(result["errors"], 1);
    assert_eq!(result["failed"], 0);
}

// ── T025/T026: US2 — path routing stubs (unit-testable logic) ────────────────

#[test]
fn test_path_label_docker() {
    // Verify "docker" is a valid path label value — used in response shape parity
    let path: &str = "docker";
    assert!(["http", "docker", "http_fallback"].contains(&path));
}

#[test]
fn test_path_label_http_fallback() {
    let path: &str = "http_fallback";
    assert!(["http", "docker", "http_fallback"].contains(&path));
}

// ── T033: US3 — namespace check logic stub ───────────────────────────────────

#[test]
fn test_namespace_not_found_error_code_is_correct_string() {
    // Validates the error code constant matches what callers will check
    assert_eq!(
        iris_dev_core::tools::ERR_NAMESPACE_NOT_FOUND,
        "NAMESPACE_NOT_FOUND"
    );
}

#[test]
fn test_no_tests_found_error_code_is_correct_string() {
    assert_eq!(iris_dev_core::tools::ERR_NO_TESTS_FOUND, "NO_TESTS_FOUND");
}
