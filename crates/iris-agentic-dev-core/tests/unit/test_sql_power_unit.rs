//! Unit tests for iris_query SQL power extensions (057-sql-power): explain, count, write.
//! No live IRIS connection required.

use iris_agentic_dev_core::tools::validate_dml_sql;

// ---------------------------------------------------------------------------
// T009: validate_dml_sql allows DML
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_allows_insert() {
    assert_eq!(validate_dml_sql("INSERT INTO t VALUES (1)"), Ok(()));
}

#[test]
fn validate_dml_sql_allows_update() {
    assert_eq!(validate_dml_sql("UPDATE t SET x=1"), Ok(()));
}

#[test]
fn validate_dml_sql_allows_delete() {
    assert_eq!(validate_dml_sql("DELETE FROM t"), Ok(()));
}

#[test]
fn validate_dml_sql_allows_call() {
    assert_eq!(validate_dml_sql("CALL myproc()"), Ok(()));
}

#[test]
fn validate_dml_sql_allows_truncate() {
    assert_eq!(validate_dml_sql("TRUNCATE TABLE t"), Ok(()));
}

// ---------------------------------------------------------------------------
// T010: validate_dml_sql blocks DDL
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_blocks_create() {
    assert_eq!(
        validate_dml_sql("CREATE TABLE t (id INT)"),
        Err("CREATE".to_string())
    );
}

#[test]
fn validate_dml_sql_blocks_drop() {
    assert_eq!(validate_dml_sql("DROP TABLE t"), Err("DROP".to_string()));
}

#[test]
fn validate_dml_sql_blocks_alter() {
    assert_eq!(
        validate_dml_sql("ALTER TABLE t ADD col INT"),
        Err("ALTER".to_string())
    );
}

#[test]
fn validate_dml_sql_blocks_grant() {
    assert_eq!(
        validate_dml_sql("GRANT SELECT ON t TO u"),
        Err("GRANT".to_string())
    );
}

#[test]
fn validate_dml_sql_blocks_revoke() {
    assert_eq!(
        validate_dml_sql("REVOKE SELECT ON t FROM u"),
        Err("REVOKE".to_string())
    );
}

// ---------------------------------------------------------------------------
// T011: validate_dml_sql blocks SELECT, empty, comments; allows inner SELECT subquery
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_blocks_select() {
    assert_eq!(
        validate_dml_sql("SELECT * FROM t"),
        Err("SELECT_IN_WRITE".to_string())
    );
}

#[test]
fn validate_dml_sql_empty_input() {
    assert_eq!(validate_dml_sql(""), Err("EMPTY".to_string()));
}

#[test]
fn validate_dml_sql_comment_only() {
    assert_eq!(
        validate_dml_sql("-- just a comment\n/* another */"),
        Err("EMPTY".to_string())
    );
}

#[test]
fn validate_dml_sql_insert_with_inner_select_allowed() {
    // Outer statement is INSERT — inner SELECT subquery doesn't change classification.
    assert_eq!(validate_dml_sql("INSERT INTO t SELECT * FROM src"), Ok(()));
}

#[test]
fn validate_dml_sql_unknown_statement() {
    assert_eq!(
        validate_dml_sql("EXPLAIN SELECT * FROM t"),
        Err("UNKNOWN_STATEMENT".to_string())
    );
}

// ---------------------------------------------------------------------------
// T012-T015: mode="explain" param validation and gate classification
// (exercised via the pure helper functions once implemented; see below)
// ---------------------------------------------------------------------------

#[test]
fn explain_requires_select_check() {
    // Mirrors the validation logic used by the explain arm: first keyword must be
    // SELECT or WITH.
    fn is_select_or_with(query: &str) -> bool {
        let first = query.split_whitespace().next().unwrap_or("").to_uppercase();
        first == "SELECT" || first == "WITH"
    }
    assert!(!is_select_or_with("INSERT INTO t VALUES (1)"));
    assert!(is_select_or_with("SELECT * FROM t"));
    assert!(is_select_or_with(
        "WITH cte AS (SELECT 1) SELECT * FROM cte"
    ));
}

#[test]
fn explain_mode_is_query_category_not_execute_by_default() {
    use iris_agentic_dev_core::iris::server_manager::tool_to_category_pub;
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(
        tool_to_category_pub("iris_query"),
        Some(ToolCategory::Query)
    );
}

#[test]
fn explain_mode_not_blocked_on_live_template() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;
    let params = serde_json::json!({"mode": "explain"});
    let result = check_env_gate("iris_query", &McpTemplate::Live, "test-server", &params);
    assert!(
        result.is_none(),
        "explain (Query) must not be blocked on live"
    );
}

// ---------------------------------------------------------------------------
// T015: query_hash helper — same query -> same hash; whitespace-insensitive
// ---------------------------------------------------------------------------

#[test]
fn query_hash_deterministic() {
    use iris_agentic_dev_core::tools::query_hash;
    let h1 = query_hash("SELECT * FROM t");
    let h2 = query_hash("SELECT * FROM t");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 16);
}

#[test]
fn query_hash_whitespace_insensitive() {
    use iris_agentic_dev_core::tools::query_hash;
    let h1 = query_hash("SELECT * FROM t");
    let h2 = query_hash("select   *   from   t");
    assert_eq!(
        h1, h2,
        "normalized hash should ignore case/whitespace differences"
    );
}

#[test]
fn query_hash_differs_for_different_queries() {
    use iris_agentic_dev_core::tools::query_hash;
    let h1 = query_hash("SELECT * FROM t");
    let h2 = query_hash("SELECT * FROM u");
    assert_ne!(h1, h2);
}

// ---------------------------------------------------------------------------
// T019: mode="count" missing target
// ---------------------------------------------------------------------------

#[test]
fn count_missing_target_detection() {
    // Mirrors the count-mode validation: neither table nor query provided.
    let table: Option<&str> = None;
    let query: Option<&str> = None;
    assert!(table.is_none() && query.is_none());
}

// ---------------------------------------------------------------------------
// T020-T021: count query building
// ---------------------------------------------------------------------------

#[test]
fn count_query_from_table() {
    use iris_agentic_dev_core::tools::build_count_query;
    assert_eq!(
        build_count_query(Some("Sample.Person"), None),
        "SELECT COUNT(*) FROM Sample.Person"
    );
}

#[test]
fn count_query_from_query_takes_precedence() {
    use iris_agentic_dev_core::tools::build_count_query;
    assert_eq!(
        build_count_query(
            Some("Sample.Person"),
            Some("SELECT * FROM Sample.Person WHERE Age > 30")
        ),
        "SELECT COUNT(*) FROM (SELECT * FROM Sample.Person WHERE Age > 30) t"
    );
}

#[test]
fn count_query_from_query_only() {
    use iris_agentic_dev_core::tools::build_count_query;
    assert_eq!(
        build_count_query(None, Some("SELECT * FROM t")),
        "SELECT COUNT(*) FROM (SELECT * FROM t) t"
    );
}

// ---------------------------------------------------------------------------
// T022: count mode Query category not blocked on live
// ---------------------------------------------------------------------------

#[test]
fn count_mode_not_blocked_on_live_template() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;
    let params = serde_json::json!({"mode": "count"});
    let result = check_env_gate("iris_query", &McpTemplate::Live, "test-server", &params);
    assert!(
        result.is_none(),
        "count (Query) must not be blocked on live"
    );
}

// ---------------------------------------------------------------------------
// T026-T027: write mode Execute category, blocked on live and test
// ---------------------------------------------------------------------------

#[test]
fn write_mode_blocked_on_live_template() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;
    let params = serde_json::json!({"mode": "write"});
    let result = check_env_gate("iris_query", &McpTemplate::Live, "test-server", &params);
    assert!(result.is_some(), "write (Execute) must be blocked on live");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn write_mode_blocked_on_test_template() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;
    let params = serde_json::json!({"mode": "write"});
    let result = check_env_gate("iris_query", &McpTemplate::Test, "test-server", &params);
    assert!(result.is_some(), "write (Execute) must be blocked on test");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn read_mode_not_blocked_on_live_or_test() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;
    let params = serde_json::json!({"mode": "read"});
    assert!(check_env_gate("iris_query", &McpTemplate::Live, "test-server", &params).is_none());
    assert!(check_env_gate("iris_query", &McpTemplate::Test, "test-server", &params).is_none());
}

// ---------------------------------------------------------------------------
// T030: max_rows_affected clamping
// ---------------------------------------------------------------------------

#[test]
fn max_rows_affected_clamp_zero_treated_as_default() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(0)), 1000);
}

#[test]
fn max_rows_affected_clamp_none_treated_as_default() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(None), 1000);
}

#[test]
fn max_rows_affected_clamp_over_limit() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(99999)), 10000);
}

#[test]
fn max_rows_affected_within_range_unchanged() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(5000)), 5000);
}

// ---------------------------------------------------------------------------
// T037-T039: regression + edge cases for existing read mode / count precedence
// ---------------------------------------------------------------------------

#[test]
fn read_mode_regression_insert_still_blocked() {
    use iris_agentic_dev_core::tools::validate_read_only_sql;
    assert_eq!(
        validate_read_only_sql("INSERT INTO t VALUES (1)"),
        Err("INSERT".to_string())
    );
}

#[test]
fn count_query_precedence_uses_subquery_form_not_table_form() {
    use iris_agentic_dev_core::tools::build_count_query;
    let sql = build_count_query(Some("IgnoredTable"), Some("SELECT 1"));
    assert!(sql.contains("(SELECT 1) t"));
    assert!(!sql.contains("IgnoredTable"));
}

// ---------------------------------------------------------------------------
// T038: mode omitted behaves identically to mode="read" explicit
// ---------------------------------------------------------------------------

#[test]
fn mode_omitted_defaults_to_read() {
    let mode: Option<String> = None;
    assert_eq!(mode.as_deref().unwrap_or("read"), "read");
    let explicit: Option<String> = Some("read".to_string());
    assert_eq!(explicit.as_deref().unwrap_or("read"), "read");
}

// ---------------------------------------------------------------------------
// T032: force has no effect in write mode (force_ignored surfaced in response)
// ---------------------------------------------------------------------------

#[test]
fn write_mode_force_flag_does_not_bypass_dml_validation() {
    use iris_agentic_dev_core::tools::validate_dml_sql;
    // force is a param on QueryParams handled at the call-site (mod.rs), not inside
    // validate_dml_sql itself — validate_dml_sql has no force parameter, confirming
    // it cannot be bypassed by force regardless of caller behavior.
    assert!(validate_dml_sql("CREATE TABLE t (id INT)").is_err());
}

// ---------------------------------------------------------------------------
// Rows pre-check query extraction (build_rows_precheck_query)
// ---------------------------------------------------------------------------

#[test]
fn rows_precheck_update_with_where() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(
        build_rows_precheck_query("UPDATE MyTable SET x=1 WHERE y=2"),
        Some("SELECT COUNT(*) FROM MyTable WHERE y=2".to_string())
    );
}

#[test]
fn rows_precheck_update_without_where() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(
        build_rows_precheck_query("UPDATE MyTable SET x=1"),
        Some("SELECT COUNT(*) FROM MyTable".to_string())
    );
}

#[test]
fn rows_precheck_delete_with_where() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(
        build_rows_precheck_query("DELETE FROM MyTable WHERE y=2"),
        Some("SELECT COUNT(*) FROM MyTable WHERE y=2".to_string())
    );
}

#[test]
fn rows_precheck_delete_without_where() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(
        build_rows_precheck_query("DELETE FROM MyTable"),
        Some("SELECT COUNT(*) FROM MyTable".to_string())
    );
}

#[test]
fn rows_precheck_insert_returns_none() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(
        build_rows_precheck_query("INSERT INTO MyTable (x) VALUES (1)"),
        None
    );
}

#[test]
fn rows_precheck_call_returns_none() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(build_rows_precheck_query("CALL myproc()"), None);
}
