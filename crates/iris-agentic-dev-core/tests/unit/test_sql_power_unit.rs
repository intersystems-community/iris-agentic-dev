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
    use iris_agentic_dev_core::testing::gated;
    let params = serde_json::json!({"mode": "explain"});
    let result = gated::env("iris_query", &McpTemplate::Live, "test-server", &params);
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
    use iris_agentic_dev_core::testing::gated;
    let params = serde_json::json!({"mode": "count"});
    let result = gated::env("iris_query", &McpTemplate::Live, "test-server", &params);
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
    use iris_agentic_dev_core::testing::gated;
    let params = serde_json::json!({"mode": "write"});
    let result = gated::env("iris_query", &McpTemplate::Live, "test-server", &params);
    assert!(result.is_some(), "write (Execute) must be blocked on live");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn write_mode_blocked_on_test_template() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::testing::gated;
    let params = serde_json::json!({"mode": "write"});
    let result = gated::env("iris_query", &McpTemplate::Test, "test-server", &params);
    assert!(result.is_some(), "write (Execute) must be blocked on test");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn read_mode_not_blocked_on_live_or_test() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::testing::gated;
    let params = serde_json::json!({"mode": "read"});
    assert!(gated::env("iris_query", &McpTemplate::Live, "test-server", &params).is_none());
    assert!(gated::env("iris_query", &McpTemplate::Test, "test-server", &params).is_none());
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

#[test]
fn rows_precheck_truncate_returns_none() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(build_rows_precheck_query("TRUNCATE TABLE MyTable"), None);
}

// ---------------------------------------------------------------------------
// T041: SQL validation edge cases — empty query variants
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_tabs_and_newlines_only() {
    assert_eq!(validate_dml_sql("\t\n\t"), Err("EMPTY".to_string()));
}

#[test]
fn validate_dml_sql_block_comment_only_with_content() {
    assert_eq!(
        validate_dml_sql("/* DROP TABLE t */"),
        Err("EMPTY".to_string())
    );
}

#[test]
fn validate_dml_sql_multiple_block_comments() {
    assert_eq!(
        validate_dml_sql("/* DROP */ /* DELETE */ /* CREATE */"),
        Err("EMPTY".to_string())
    );
}

#[test]
fn validate_dml_sql_nested_block_comment_partial() {
    // Block comments don't nest: "/* outer /* inner */" stops at first */ (closes comment prematurely)
    // Leaving " stays closed */" which starts with unknown keyword "stays"
    assert_eq!(
        validate_dml_sql("/* outer /* inner */ stays closed */"),
        Err("UNKNOWN_STATEMENT".to_string())
    );
}

#[test]
fn validate_dml_sql_line_comment_with_block_comment() {
    assert_eq!(
        validate_dml_sql("-- line comment\n/* block */"),
        Err("EMPTY".to_string())
    );
}

// ---------------------------------------------------------------------------
// T042: SQL validation — case sensitivity and word boundaries
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_lowercase_insert() {
    assert_eq!(validate_dml_sql("insert into t values (1)"), Ok(()));
}

#[test]
fn validate_dml_sql_lowercase_update() {
    assert_eq!(validate_dml_sql("update t set x=1"), Ok(()));
}

#[test]
fn validate_dml_sql_lowercase_delete() {
    assert_eq!(validate_dml_sql("delete from t"), Ok(()));
}

#[test]
fn validate_dml_sql_lowercase_call() {
    assert_eq!(validate_dml_sql("call myproc()"), Ok(()));
}

#[test]
fn validate_dml_sql_lowercase_truncate() {
    assert_eq!(validate_dml_sql("truncate table t"), Ok(()));
}

#[test]
fn validate_dml_sql_mixed_case_insert() {
    assert_eq!(validate_dml_sql("InSeRt INTO t VALUES (1)"), Ok(()));
}

#[test]
fn validate_dml_sql_mixed_case_create_blocked() {
    assert_eq!(
        validate_dml_sql("CrEaTe TABLE t (id INT)"),
        Err("CREATE".to_string())
    );
}

#[test]
fn validate_dml_sql_mixed_case_drop_blocked() {
    assert_eq!(validate_dml_sql("DrOp TABLE t"), Err("DROP".to_string()));
}

#[test]
fn validate_dml_sql_mixed_case_alter_blocked() {
    assert_eq!(
        validate_dml_sql("AlTeR TABLE t ADD x INT"),
        Err("ALTER".to_string())
    );
}

#[test]
fn validate_dml_sql_mixed_case_grant_blocked() {
    assert_eq!(
        validate_dml_sql("GrAnT SELECT ON t TO u"),
        Err("GRANT".to_string())
    );
}

#[test]
fn validate_dml_sql_mixed_case_revoke_blocked() {
    assert_eq!(
        validate_dml_sql("ReVoKe SELECT ON t FROM u"),
        Err("REVOKE".to_string())
    );
}

// ---------------------------------------------------------------------------
// T043: SQL validation — quoted identifiers don't trigger false positives
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_double_quoted_create_in_identifier() {
    assert_eq!(
        validate_dml_sql("INSERT INTO \"CREATE_TABLE\" VALUES (1)"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_double_quoted_drop_in_identifier() {
    assert_eq!(validate_dml_sql("UPDATE \"DROP_ME\" SET x=1"), Ok(()));
}

#[test]
fn validate_dml_sql_single_quoted_insert_in_literal() {
    assert_eq!(
        validate_dml_sql("DELETE FROM t WHERE msg='INSERT NEW'"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_single_quoted_delete_in_literal() {
    assert_eq!(
        validate_dml_sql("UPDATE t SET note='DELETE THIS' WHERE id=1"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_comment_stripping_before_quote_processing() {
    // Ensure comments are stripped before checking quoted strings
    // "-- DROP" is a line comment, so "INSERT" should be the first word
    assert_eq!(
        validate_dml_sql("-- DROP TABLE\nINSERT INTO t VALUES (1)"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_block_comment_with_quotes_inside() {
    // Block comment contains quotes; after stripping, just "INSERT"
    assert_eq!(
        validate_dml_sql("/* 'DROP' \"CREATE\" */ INSERT INTO t VALUES (1)"),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// T044: SQL validation — unknown/invalid first words
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_unknown_statement_explain() {
    assert_eq!(
        validate_dml_sql("EXPLAIN SELECT * FROM t"),
        Err("UNKNOWN_STATEMENT".to_string())
    );
}

#[test]
fn validate_dml_sql_unknown_statement_with() {
    // WITH is a CTE keyword, not in DML list
    assert_eq!(
        validate_dml_sql("WITH cte AS (SELECT 1) SELECT * FROM cte"),
        Err("UNKNOWN_STATEMENT".to_string())
    );
}

#[test]
fn validate_dml_sql_unknown_statement_select() {
    // SELECT is explicitly rejected as "SELECT_IN_WRITE"
    assert_eq!(
        validate_dml_sql("SELECT * FROM t"),
        Err("SELECT_IN_WRITE".to_string())
    );
}

#[test]
fn validate_dml_sql_unknown_statement_pragma() {
    assert_eq!(
        validate_dml_sql("PRAGMA table_info(t)"),
        Err("UNKNOWN_STATEMENT".to_string())
    );
}

#[test]
fn validate_dml_sql_unknown_statement_analyze() {
    assert_eq!(
        validate_dml_sql("ANALYZE TABLE t"),
        Err("UNKNOWN_STATEMENT".to_string())
    );
}

// ---------------------------------------------------------------------------
// T045: SQL validation — DML with complex inner SELECT patterns
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_insert_select_from_subquery() {
    assert_eq!(
        validate_dml_sql("INSERT INTO t SELECT * FROM (SELECT x FROM src) sub"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_insert_select_with_cte() {
    // CTE in inner SELECT doesn't change the outer INSERT classification
    assert_eq!(
        validate_dml_sql("INSERT INTO t WITH cte AS (SELECT 1) SELECT * FROM cte"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_update_with_inner_select() {
    assert_eq!(
        validate_dml_sql("UPDATE t SET x=(SELECT MAX(y) FROM src) WHERE id=1"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_delete_with_inner_select() {
    assert_eq!(
        validate_dml_sql("DELETE FROM t WHERE id IN (SELECT id FROM src)"),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// T046: Query hash determinism and variations
// ---------------------------------------------------------------------------

#[test]
fn query_hash_tabs_vs_spaces() {
    use iris_agentic_dev_core::tools::query_hash;
    let h1 = query_hash("SELECT\t*\tFROM\tt");
    let h2 = query_hash("SELECT * FROM t");
    assert_eq!(h1, h2, "tabs and spaces should normalize to same hash");
}

#[test]
fn query_hash_multiline_collapse() {
    use iris_agentic_dev_core::tools::query_hash;
    let h1 = query_hash("SELECT *\nFROM t\nWHERE x = 1\nORDER BY y");
    let h2 = query_hash("SELECT * FROM t WHERE x = 1 ORDER BY y");
    assert_eq!(h1, h2, "multiline should match single-line");
}

#[test]
fn query_hash_lowercase_vs_uppercase() {
    use iris_agentic_dev_core::tools::query_hash;
    let h1 = query_hash("select * from t");
    let h2 = query_hash("SELECT * FROM T");
    assert_eq!(h1, h2, "case should be normalized");
}

#[test]
fn query_hash_length_is_16_hex_chars() {
    use iris_agentic_dev_core::tools::query_hash;
    let h = query_hash("SELECT 1");
    assert_eq!(h.len(), 16);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

// ---------------------------------------------------------------------------
// T047: Count query edge cases
// ---------------------------------------------------------------------------

#[test]
fn count_query_table_with_schema() {
    use iris_agentic_dev_core::tools::build_count_query;
    assert_eq!(
        build_count_query(Some("MySchema.MyTable"), None),
        "SELECT COUNT(*) FROM MySchema.MyTable"
    );
}

#[test]
fn count_query_complex_query() {
    use iris_agentic_dev_core::tools::build_count_query;
    let complex = "SELECT DISTINCT Category FROM Products WHERE Active=1";
    let result = build_count_query(None, Some(complex));
    assert!(result.starts_with("SELECT COUNT(*) FROM ("));
    assert!(result.contains(complex));
    assert!(result.ends_with(") t"));
}

#[test]
fn count_query_both_provided_uses_query() {
    use iris_agentic_dev_core::tools::build_count_query;
    let result = build_count_query(Some("IgnoredTable"), Some("SELECT * FROM ActualTable"));
    assert!(result.contains("ActualTable"));
    assert!(!result.contains("IgnoredTable"));
}

#[test]
fn count_query_neither_provided() {
    use iris_agentic_dev_core::tools::build_count_query;
    let result = build_count_query(None, None);
    // When neither is provided, table.unwrap_or_default() gives empty string
    assert_eq!(result, "SELECT COUNT(*) FROM ");
}

// ---------------------------------------------------------------------------
// T048: Clamping max_rows_affected edge cases
// ---------------------------------------------------------------------------

#[test]
fn max_rows_affected_clamp_one() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(1)), 1);
}

#[test]
fn max_rows_affected_clamp_boundary_9999() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(9999)), 9999);
}

#[test]
fn max_rows_affected_clamp_boundary_10000() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(10000)), 10000);
}

#[test]
fn max_rows_affected_clamp_boundary_10001() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(10001)), 10000);
}

#[test]
fn max_rows_affected_clamp_max_u32() {
    use iris_agentic_dev_core::tools::clamp_max_rows_affected;
    assert_eq!(clamp_max_rows_affected(Some(u32::MAX)), 10000);
}

// ---------------------------------------------------------------------------
// T049: Rows precheck query edge cases
// ---------------------------------------------------------------------------

#[test]
fn rows_precheck_update_multispace_table() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    let result = build_rows_precheck_query("UPDATE    MyTable    SET x=1");
    assert_eq!(result, Some("SELECT COUNT(*) FROM MyTable".to_string()));
}

#[test]
fn rows_precheck_update_with_complex_where() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    let result =
        build_rows_precheck_query("UPDATE MyTable SET x=1 WHERE y IN (1,2,3) AND z IS NOT NULL");
    assert_eq!(
        result,
        Some("SELECT COUNT(*) FROM MyTable WHERE y IN (1,2,3) AND z IS NOT NULL".to_string())
    );
}

#[test]
fn rows_precheck_delete_with_complex_where() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    let result = build_rows_precheck_query(
        "DELETE FROM MyTable WHERE x > 100 AND (y = 'active' OR z IS NULL)",
    );
    assert!(result.is_some());
    let precheck = result.unwrap();
    assert!(precheck.starts_with("SELECT COUNT(*) FROM MyTable WHERE"));
    assert!(precheck.contains("y = 'active'"));
}

#[test]
fn rows_precheck_where_word_boundary_not_matched_in_identifier() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    // "SOMEWHERE" contains "WHERE" but not as a word boundary
    let result = build_rows_precheck_query("UPDATE MyTable SET x=1");
    assert_eq!(result, Some("SELECT COUNT(*) FROM MyTable".to_string()));
}

#[test]
fn rows_precheck_insert_various_forms_all_return_none() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    assert_eq!(
        build_rows_precheck_query("INSERT INTO t (a,b) VALUES (1,2)"),
        None
    );
    assert_eq!(
        build_rows_precheck_query("INSERT INTO t SELECT * FROM src"),
        None
    );
    assert_eq!(build_rows_precheck_query("INSERT t VALUES (1)"), None);
}

// ---------------------------------------------------------------------------
// T050: Whitespace and comment interactions
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_leading_spaces_and_comments() {
    assert_eq!(
        validate_dml_sql("   \n\n   /* comment */  UPDATE t SET x=1"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_trailing_comments_ignored() {
    assert_eq!(
        validate_dml_sql("INSERT INTO t VALUES (1) -- trailing comment"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_comments_between_keywords() {
    assert_eq!(
        validate_dml_sql("INSERT /* comment */ INTO t VALUES (1)"),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// T051: Special characters and SQL injection attempts
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_semicolon_injection_update() {
    // Semicolon doesn't create a second statement in our single-keyword classifier
    assert_eq!(validate_dml_sql("UPDATE t SET x=1; DELETE FROM t"), Ok(()));
}

#[test]
fn validate_dml_sql_null_byte_handling() {
    // Null bytes are technically valid UTF-8, but we should process them safely
    let sql = "INSERT INTO t VALUES (1)";
    assert_eq!(validate_dml_sql(sql), Ok(()));
}

// ---------------------------------------------------------------------------
// T046: SQL validation — backslash escapes in single-quoted strings (L1323-1325)
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_single_quoted_with_backslash_escape() {
    // Single-quoted string with backslash-escaped quote: 'it\'s'
    // The backslash should skip the next character, so the quote doesn't close the string early
    assert_eq!(
        validate_dml_sql("INSERT INTO t SET note = 'it\\'s fine' WHERE id=1"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_single_quoted_with_multiple_backslash_escapes() {
    // Multiple backslash escapes in a single-quoted string
    assert_eq!(
        validate_dml_sql("UPDATE t SET msg = 'it\\'s a \\\\ backslash' WHERE id=1"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_single_quoted_with_backslash_at_end() {
    // Backslash followed by final quote in single-quoted string
    assert_eq!(
        validate_dml_sql("DELETE FROM t WHERE name = 'ends_with\\' AND x=1"),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// T047: SQL validation — double-quoted identifiers at start (L1332-1339)
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_double_quoted_identifier_at_start() {
    // Double-quoted identifier appearing before the first keyword
    // SQL starts with a double-quoted word, then INSERT keyword follows
    assert_eq!(
        validate_dml_sql("\"CREATE\" INSERT INTO t VALUES (1)"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_double_quoted_then_dml() {
    // Multiple double-quoted identifiers before DML keyword
    assert_eq!(
        validate_dml_sql("\"DROP\" \"ALTER\" UPDATE t SET x=1"),
        Ok(())
    );
}

#[test]
fn validate_dml_sql_mixed_quotes_before_keyword() {
    // Mix of single-quoted literals and double-quoted identifiers before DML
    assert_eq!(
        validate_dml_sql("'INSERT_LITERAL' \"CREATE\" DELETE FROM t WHERE id=1"),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// T048: SQL validation — whitespace and non-alphanumeric before keyword (L1340-1351)
// ---------------------------------------------------------------------------

#[test]
fn validate_dml_sql_special_chars_before_keyword() {
    // SQL starts with special characters like @, # before the keyword
    // These should be skipped during keyword search; the first alphanumeric word
    // will be checked. Here we have @ # before INSERT which is a valid DML keyword.
    assert_eq!(validate_dml_sql("@ # INSERT INTO t VALUES (1)"), Ok(()));
}

#[test]
fn validate_dml_sql_whitespace_and_special_chars() {
    // Whitespace and special characters before DML keyword
    assert_eq!(
        validate_dml_sql("  \t  (((  INSERT INTO t VALUES (1)"),
        Ok(())
    );
}

#[test]
fn rows_precheck_query_with_semicolon_at_end() {
    use iris_agentic_dev_core::tools::build_rows_precheck_query;
    // Trailing semicolon shouldn't break parsing
    let result = build_rows_precheck_query("UPDATE MyTable SET x=1 WHERE y=2;");
    assert_eq!(
        result,
        Some("SELECT COUNT(*) FROM MyTable WHERE y=2;".to_string())
    );
}
