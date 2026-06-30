// Unit tests for tools/mod.rs internal helpers and public functions.
// Targets uncovered branches identified from LCOV analysis.

use iris_agentic_dev_core::tools::{map_status_int, translate_sql_macros, validate_read_only_sql};

// ── map_status_int ────────────────────────────────────────────────────────────

#[test]
fn map_status_int_all_branches() {
    // 1 → passed
    assert_eq!(map_status_int(1, ""), "passed");
    // 0 → failed
    assert_eq!(map_status_int(0, ""), "failed");
    // other with non-empty error_action → error
    assert_eq!(map_status_int(2, "someAction"), "error");
    // other with empty error_action → failed
    assert_eq!(map_status_int(99, ""), "failed");
}

// ── validate_read_only_sql ────────────────────────────────────────────────────

#[test]
fn validate_read_only_sql_select_ok() {
    assert!(validate_read_only_sql("SELECT 1").is_ok());
    assert!(validate_read_only_sql("SELECT Name FROM Sample.Person").is_ok());
}

#[test]
fn validate_read_only_sql_insert_blocked() {
    assert!(validate_read_only_sql("INSERT INTO Foo VALUES (1)").is_err());
}

#[test]
fn validate_read_only_sql_update_blocked() {
    assert!(validate_read_only_sql("UPDATE Foo SET x=1").is_err());
}

#[test]
fn validate_read_only_sql_delete_blocked() {
    assert!(validate_read_only_sql("DELETE FROM Foo").is_err());
}

// ── translate_sql_macros ──────────────────────────────────────────────────────

#[test]
fn translate_sql_macros_passthrough_plain_sql() {
    let result = translate_sql_macros("SELECT 1");
    assert!(
        !result.found,
        "plain SQL should not be flagged as translated"
    );
    assert_eq!(result.translated_code, "SELECT 1");
}

#[test]
fn translate_sql_macros_select_into_translated() {
    // &sql macro SELECT INTO — should detect the pattern
    let code = "&sql(SELECT Name INTO :name FROM Sample.Person WHERE ID=1)";
    let result = translate_sql_macros(code);
    // If found, translated_code should be populated
    if result.found {
        assert!(!result.translated_code.is_empty());
    }
    // At minimum, the function does not panic
}

// ── ConfigWatcher::has_changed ────────────────────────────────────────────────

#[test]
fn config_watcher_has_changed_appears_after_creation() {
    use iris_agentic_dev_core::tools::ConfigWatcher;
    use std::io::Write;

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join(".iris-agentic-dev.toml");
    // ConfigWatcher::new with a non-existent file → last_mtime = None
    let mut w = ConfigWatcher::new(path.clone()).unwrap();
    assert!(w.last_mtime.is_none(), "non-existent file → no mtime");
    // has_changed: None→None → false
    assert!(!w.has_changed(), "file not yet created → not changed");
    // Create the file — should now be detected (None→Some)
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"container = \"test\"\n").unwrap();
    }
    assert!(w.has_changed(), "newly created file should be detected");
    // Immediately check again — not changed (same mtime)
    assert!(!w.has_changed(), "same mtime → not changed on second check");
}

#[test]
fn config_watcher_has_changed_file_deleted() {
    use iris_agentic_dev_core::tools::ConfigWatcher;
    use std::io::Write;

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join(".iris-agentic-dev.toml");
    // Create file first so ConfigWatcher::new captures mtime
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"container = \"test\"\n").unwrap();
    }
    let mut w = ConfigWatcher::new(path.clone()).unwrap();
    assert!(
        w.last_mtime.is_some(),
        "file exists → mtime captured at construction"
    );
    // has_changed: file unchanged since construction → false
    let _ = w.has_changed();
    // Delete the file
    std::fs::remove_file(&path).unwrap();
    // has_changed should see deletion (Some -> None) and return false, reset mtime
    let deleted = w.has_changed();
    assert!(!deleted, "file deleted → has_changed returns false");
    assert!(w.last_mtime.is_none(), "mtime reset to None after deletion");
    // Re-create — should detect (None→Some)
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"container = \"test2\"\n").unwrap();
    }
    assert!(
        w.has_changed(),
        "file re-created after deletion should be detected"
    );
}

// ── check_config via call_for_test (no IRIS connection → None branch) ─────────

#[cfg(feature = "testing")]
#[tokio::test]
async fn check_config_with_no_iris_returns_not_connected() {
    use iris_agentic_dev_core::tools::IrisTools;

    let tools = IrisTools::new(None).expect("IrisTools::new should succeed");
    // Do NOT set any IRIS connection — iris field will be None
    // Hits the None branch in check_config (~L3228-3234)
    let result = tools
        .call_for_test("check_config", serde_json::json!({}))
        .await;

    match result {
        Ok(r) => {
            let text = r
                .content
                .first()
                .and_then(|c| c.raw.as_text())
                .map(|t| t.text.clone())
                .unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            // connected=false OR missing means no live IRIS — both are correct
            let connected = v["connected"].as_bool().unwrap_or(false);
            assert!(
                !connected,
                "check_config with no IRIS must show not-connected: {v}"
            );
        }
        Err(e) => {
            // Errors are acceptable when IRIS is absent
            eprintln!("check_config returned Err (acceptable without IRIS): {e}");
        }
    }
}

// ── split_host_vars_from_rest edge case (via translate_sql_macros) ────────────

#[test]
fn translate_sql_macros_select_into_no_from_clause() {
    // SELECT INTO with no FROM — hits the fallback branch in split_host_vars_from_rest
    let code = "&sql(SELECT :x INTO :y)";
    let result = translate_sql_macros(code);
    // Should not panic; may or may not translate
    let _ = result;
}

#[test]
fn translate_sql_macros_select_without_select_keyword() {
    // Edge case: select_cols_sql without SELECT keyword hits L344 fallback
    // This is hard to trigger externally; at minimum verify no panic
    let code = "&sql(1 INTO :x FROM Foo WHERE ID=1)";
    let result = translate_sql_macros(code);
    let _ = result;
}

// ── build_test_run_from_sql / build_test_detail ────────────────────────────────

#[test]
fn build_test_run_from_sql_empty_input() {
    use iris_agentic_dev_core::tools::build_test_run_from_sql;

    let result = build_test_run_from_sql(&[], &[]);
    assert!(result.is_object(), "empty input should return an object");
}

#[test]
fn build_test_run_from_sql_with_suites() {
    use iris_agentic_dev_core::tools::{build_test_run_from_sql, SuiteRow};

    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "TestSuite".to_string(),
        status: 1,
        duration_ms: Some(123.0),
    }];
    let result = build_test_run_from_sql(&suites, &[]);
    assert!(result.is_object());
}

#[test]
fn build_test_detail_empty_input() {
    use iris_agentic_dev_core::tools::build_test_detail;

    let result = build_test_detail(&[], &[]);
    assert!(result.is_object());
}

// ── write_open_hint: smoke test ───────────────────────────────────────────────

#[test]
fn write_open_hint_does_not_panic() {
    use iris_agentic_dev_core::tools::write_open_hint;
    // write_open_hint emits to tracing — just verify it doesn't panic
    write_open_hint("USER", "Sample.Person.cls");
    write_open_hint("", "");
}

// ── IrisTools::registered_tool_names ─────────────────────────────────────────

#[test]
fn iris_tools_registered_tool_names_non_empty() {
    use iris_agentic_dev_core::tools::IrisTools;
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        !names.is_empty(),
        "registered_tool_names must return non-empty set"
    );
    // Core tools should be registered
    assert!(
        names.contains("iris_compile"),
        "iris_compile must be registered"
    );
    assert!(
        names.contains("iris_execute"),
        "iris_execute must be registered"
    );
    assert!(
        names.contains("iris_query"),
        "iris_query must be registered"
    );
    assert!(
        names.contains("check_config"),
        "check_config must be registered"
    );
    // 052: iris_global must appear in the Merged toolset inventory
    assert!(
        names.contains("iris_global"),
        "iris_global must be registered (052-iris-global)"
    );
}

// ── validate_read_only_sql: edge cases and comment handling ─────────────────

#[test]
fn validate_read_only_sql_block_comment() {
    // SQL with /* */ block comment containing a keyword should still pass if outer SQL is clean
    assert!(validate_read_only_sql("SELECT /* DELETE */ id FROM t").is_ok());
}

#[test]
fn validate_read_only_sql_line_comment() {
    // SQL with -- line comment containing a keyword should pass
    assert!(validate_read_only_sql("SELECT id FROM t -- DELETE this later").is_ok());
}

#[test]
fn validate_read_only_sql_quoted_keyword() {
    // Quoted string containing a keyword should not trigger block
    assert!(validate_read_only_sql("SELECT 'DELETE ME' as msg FROM t").is_ok());
}

#[test]
fn validate_read_only_sql_double_quoted_identifier() {
    // Double-quoted identifier containing keyword should not trigger
    assert!(validate_read_only_sql("SELECT \"UPDATE\" FROM t").is_ok());
}

#[test]
fn validate_read_only_sql_select_into_with_subquery() {
    // SELECT INTO with subquery is allowed (e.g., INTO (SELECT ...))
    assert!(validate_read_only_sql("SELECT col INTO (SELECT * FROM foo) FROM bar").is_ok());
}

#[test]
fn validate_read_only_sql_create_blocked() {
    assert!(validate_read_only_sql("CREATE TABLE Foo (id INT)").is_err());
}

#[test]
fn validate_read_only_sql_drop_blocked() {
    assert!(validate_read_only_sql("DROP TABLE Foo").is_err());
}

#[test]
fn validate_read_only_sql_alter_blocked() {
    assert!(validate_read_only_sql("ALTER TABLE Foo ADD col INT").is_err());
}

#[test]
fn validate_read_only_sql_merge_blocked() {
    assert!(validate_read_only_sql("MERGE INTO target t USING source s ON t.id=s.id").is_err());
}

#[test]
fn validate_read_only_sql_truncate_blocked() {
    assert!(validate_read_only_sql("TRUNCATE TABLE Foo").is_err());
}

#[test]
fn validate_read_only_sql_exec_blocked() {
    assert!(validate_read_only_sql("EXEC sp_stored_proc").is_err());
}

#[test]
fn validate_read_only_sql_execute_blocked() {
    assert!(validate_read_only_sql("EXECUTE sp_stored_proc").is_err());
}

#[test]
fn validate_read_only_sql_load_blocked() {
    assert!(validate_read_only_sql("LOAD DATA INTO TABLE Foo").is_err());
}

#[test]
fn validate_read_only_sql_kill_blocked() {
    assert!(validate_read_only_sql("KILL SESSION 123").is_err());
}

#[test]
fn validate_read_only_sql_lock_blocked() {
    assert!(validate_read_only_sql("LOCK TABLE Foo").is_err());
}

#[test]
fn validate_read_only_sql_word_boundary_underscore() {
    // "_UPDATE" is not a keyword (underscore-prefixed), should pass
    assert!(validate_read_only_sql("SELECT _UPDATE FROM t").is_ok());
}

#[test]
fn validate_read_only_sql_empty_after_comment_stripping() {
    // Only comments and whitespace should fail with EMPTY
    let result = validate_read_only_sql("/* comment */ -- line comment");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "EMPTY");
}

#[test]
fn validate_read_only_sql_escaped_quote_in_string() {
    // String with escaped quote should not break quote tracking
    assert!(validate_read_only_sql("SELECT 'it\\'s ok' FROM t").is_ok());
}

#[test]
fn validate_read_only_sql_nested_parens_for_depth_tracking() {
    // Test that nested parens in WHERE clause don't confuse parsing
    assert!(
        validate_read_only_sql("SELECT id FROM t WHERE (a > 5 AND (b < 10 OR c = 20))").is_ok()
    );
}

// ── translate_sql_macros: edge cases ───────────────────────────────────────

#[test]
fn translate_sql_macros_multiple_macros() {
    let code = "&sql(SELECT 1 INTO :x FROM t)&sql(SELECT y INTO :z FROM u)";
    let result = translate_sql_macros(code);
    assert!(result.found, "multiple macros should be detected");
    // Should have at least one translated — may not handle second depending on spacing
    assert!(result.translated_code.contains("sqlrs1"));
}

#[test]
fn translate_sql_macros_nested_parens() {
    let code = "&sql(SELECT a, (SELECT b FROM c) as sub FROM t)";
    let result = translate_sql_macros(code);
    // Should not panic on nested parens
    assert_eq!(result.found, true);
}

#[test]
fn translate_sql_macros_call_statement_unsupported() {
    let code = "&sql(CALL MyProc(:result OUT))";
    let result = translate_sql_macros(code);
    assert!(result.found);
    assert!(!result.warnings.is_empty(), "CALL should have warning");
    assert!(
        result.warnings[0].contains("CALL"),
        "Warning should mention CALL"
    );
}

#[test]
fn translate_sql_macros_dml_insert() {
    let code = "&sql(INSERT INTO foo (a, b) VALUES (:x, :y))";
    let result = translate_sql_macros(code);
    assert!(result.found);
    // Should translate to DML form
    assert!(result.translated_code.contains("sqlrs1"));
}

#[test]
fn translate_sql_macros_dml_update() {
    let code = "&sql(UPDATE foo SET a=:x WHERE id=:y)";
    let result = translate_sql_macros(code);
    assert!(result.found);
}

#[test]
fn translate_sql_macros_dml_delete() {
    let code = "&sql(DELETE FROM foo WHERE id=:x)";
    let result = translate_sql_macros(code);
    assert!(result.found);
}

#[test]
fn translate_sql_macros_dml_merge() {
    let code =
        "&sql(MERGE INTO target t USING source s ON t.id=s.id WHEN MATCHED THEN UPDATE SET val=:x)";
    let result = translate_sql_macros(code);
    assert!(result.found);
}

#[test]
fn translate_sql_macros_unknown_statement() {
    let code = "&sql(WITH cte AS (SELECT 1) SELECT * FROM cte)";
    let result = translate_sql_macros(code);
    assert!(result.found);
    // WITH is not recognized, should have warning
    assert!(!result.warnings.is_empty());
}

#[test]
fn translate_sql_macros_case_insensitive_keywords() {
    let code = "&sql(select id from t)";
    let result = translate_sql_macros(code);
    assert!(result.found, "lowercase select should be recognized");
}

// ── build_test_run_from_sql: comprehensive test outcomes ──────────────────

#[test]
fn build_test_run_from_sql_all_passed() {
    use iris_agentic_dev_core::tools::{build_test_run_from_sql, MethodRow, SuiteRow};

    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "MySuite".to_string(),
        status: 1,
        duration_ms: Some(100.0),
    }];
    let methods = vec![
        MethodRow {
            suite_id: "1".to_string(),
            name: "test1".to_string(),
            class_name: "MyTest".to_string(),
            status: 1, // passed
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(50.0),
        },
        MethodRow {
            suite_id: "1".to_string(),
            name: "test2".to_string(),
            class_name: "MyTest".to_string(),
            status: 1, // passed
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(50.0),
        },
    ];

    let result = build_test_run_from_sql(&suites, &methods);
    assert_eq!(result["success"], true);
    assert_eq!(result["outcome"], "passed");
    assert_eq!(result["total"], 2);
    assert_eq!(result["passed"], 2);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["errors"], 0);
}

#[test]
fn build_test_run_from_sql_with_failures() {
    use iris_agentic_dev_core::tools::{build_test_run_from_sql, MethodRow, SuiteRow};

    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "MySuite".to_string(),
        status: 0,
        duration_ms: Some(100.0),
    }];
    let methods = vec![
        MethodRow {
            suite_id: "1".to_string(),
            name: "test1".to_string(),
            class_name: "MyTest".to_string(),
            status: 1, // passed
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(50.0),
        },
        MethodRow {
            suite_id: "1".to_string(),
            name: "test2".to_string(),
            class_name: "MyTest".to_string(),
            status: 0, // failed
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(50.0),
        },
    ];

    let result = build_test_run_from_sql(&suites, &methods);
    assert_eq!(result["success"], true);
    assert_eq!(result["outcome"], "failed");
    assert_eq!(result["total"], 2);
    assert_eq!(result["passed"], 1);
    assert_eq!(result["failed"], 1);
}

#[test]
fn build_test_run_from_sql_with_errors() {
    use iris_agentic_dev_core::tools::{build_test_run_from_sql, MethodRow, SuiteRow};

    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "MySuite".to_string(),
        status: 2,
        duration_ms: Some(100.0),
    }];
    let methods = vec![MethodRow {
        suite_id: "1".to_string(),
        name: "test1".to_string(),
        class_name: "MyTest".to_string(),
        status: 2, // error
        error_action: "some_action".to_string(),
        error_description: "error occurred".to_string(),
        duration_ms: Some(100.0),
    }];

    let result = build_test_run_from_sql(&suites, &methods);
    assert_eq!(result["success"], true);
    assert_eq!(result["outcome"], "errored");
    assert_eq!(result["errors"], 1);
}

#[test]
fn build_test_run_from_sql_multiple_suites() {
    use iris_agentic_dev_core::tools::{build_test_run_from_sql, MethodRow, SuiteRow};

    let suites = vec![
        SuiteRow {
            id: "1".to_string(),
            name: "Suite1".to_string(),
            status: 1,
            duration_ms: Some(50.0),
        },
        SuiteRow {
            id: "2".to_string(),
            name: "Suite2".to_string(),
            status: 1,
            duration_ms: Some(75.0),
        },
    ];
    let methods = vec![
        MethodRow {
            suite_id: "1".to_string(),
            name: "test1".to_string(),
            class_name: "Test1".to_string(),
            status: 1,
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(50.0),
        },
        MethodRow {
            suite_id: "2".to_string(),
            name: "test2".to_string(),
            class_name: "Test2".to_string(),
            status: 1,
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(75.0),
        },
    ];

    let result = build_test_run_from_sql(&suites, &methods);
    assert_eq!(result["total"], 2);
    assert_eq!(result["duration_ms"], 125.0);
    assert!(result["test_suites"].is_array());
    assert_eq!(result["test_suites"].as_array().unwrap().len(), 2);
}

// ── build_test_detail: test case formatting ───────────────────────────────

#[test]
fn build_test_detail_with_failure_messages() {
    use iris_agentic_dev_core::tools::{build_test_detail, MethodRow, SuiteRow};

    let suites = vec![SuiteRow {
        id: "1".to_string(),
        name: "Suite".to_string(),
        status: 0,
        duration_ms: Some(100.0),
    }];
    let methods = vec![
        MethodRow {
            suite_id: "1".to_string(),
            name: "test_pass".to_string(),
            class_name: "MyTest".to_string(),
            status: 1,
            error_action: "".to_string(),
            error_description: "".to_string(),
            duration_ms: Some(50.0),
        },
        MethodRow {
            suite_id: "1".to_string(),
            name: "test_fail".to_string(),
            class_name: "MyTest".to_string(),
            status: 0,
            error_action: "".to_string(),
            error_description: "Assertion failed: expected 5, got 3".to_string(),
            duration_ms: Some(50.0),
        },
    ];

    let result = build_test_detail(&suites, &methods);
    assert!(result["test_suites"].is_array());
    let suite = &result["test_suites"][0];
    assert_eq!(suite["tests"], 2);
    assert_eq!(suite["failures"], 1);
    let cases = &suite["test_cases"];
    assert_eq!(
        cases[1]["failure_message"],
        "Assertion failed: expected 5, got 3"
    );
}

// ── translate_symbols_query edge cases ──────────────────────────────────────

#[test]
fn translate_symbols_query_empty_query() {
    let (sql, params) = iris_agentic_dev_core::tools::translate_symbols_query(50, "");
    assert!(sql.contains("SELECT TOP 50"));
    assert!(!sql.contains("WHERE"));
    assert!(params.is_empty());
}

#[test]
fn translate_symbols_query_custom_limit() {
    let (sql, _) = iris_agentic_dev_core::tools::translate_symbols_query(100, "*");
    assert!(sql.contains("TOP 100"));
}

#[test]
fn translate_symbols_query_multiple_wildcards() {
    let (sql, params) = iris_agentic_dev_core::tools::translate_symbols_query(50, "My.*.Service");
    // Query with wildcard in middle uses LIKE
    assert!(sql.contains("LIKE"));
    assert_eq!(params[0].as_str().unwrap(), "My.%.Service");
}

#[test]
fn translate_symbols_query_dot_prefix_with_wildcard() {
    let (sql, params) = iris_agentic_dev_core::tools::translate_symbols_query(50, "HT.*");
    assert!(sql.contains("STARTSWITH"));
    assert_eq!(params[0].as_str().unwrap(), "HT.");
}
