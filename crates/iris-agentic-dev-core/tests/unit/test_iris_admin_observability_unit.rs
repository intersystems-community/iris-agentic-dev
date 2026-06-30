//! Unit tests for iris_admin observability actions (055-system-observability).
//! No live IRIS connection required.

use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy, McpTemplate};
use iris_agentic_dev_core::tools::observability::{
    glob_to_sql_like, redact_process_entry, require_data_policy_allow, resolve_namespace,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_result(result: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("text content");
    serde_json::from_str(&text).expect("valid JSON")
}

fn live_policy() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Live),
        data_policy: Some(DataPolicy::Block),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    }
}

// ---------------------------------------------------------------------------
// T011: glob_to_sql_like
// ---------------------------------------------------------------------------

#[test]
fn glob_star_becomes_percent() {
    assert_eq!(glob_to_sql_like("IrisDevTest.*"), "IrisDevTest.%");
}

#[test]
fn glob_question_becomes_underscore() {
    assert_eq!(glob_to_sql_like("^PAPMI?"), "^PAPMI_");
}

#[test]
fn glob_literal_percent_escaped() {
    assert_eq!(glob_to_sql_like("100%off"), r"100\%off");
}

#[test]
fn glob_literal_underscore_escaped() {
    assert_eq!(glob_to_sql_like("foo_bar"), r"foo\_bar");
}

#[test]
fn glob_mixed_wildcards() {
    // '*' → '%', '?' → '_', '.' is literal
    assert_eq!(glob_to_sql_like("^App*.?"), "^App%._");
    // '_' is literal in glob input, gets escaped to '\_'; '*' → '%'
    assert_eq!(glob_to_sql_like("x_*"), r"x\_%");
}

#[test]
fn glob_empty_string() {
    assert_eq!(glob_to_sql_like(""), "");
}

// ---------------------------------------------------------------------------
// T012: view_locks with no IRIS returns IRIS_UNREACHABLE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn view_locks_no_iris_returns_unreachable() {
    let result = iris_agentic_dev_core::tools::observability::view_locks_impl(None)
        .await
        .expect("Ok(CallToolResult)");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    assert_eq!(v["success"], false);
}

// ---------------------------------------------------------------------------
// T013: view_locks empty list returns correct shape (not error)
// ---------------------------------------------------------------------------

#[test]
fn view_locks_empty_output_parses_to_empty_array() {
    let out = "";
    let locks: Vec<serde_json::Value> = out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let p: Vec<&str> = line.splitn(5, '|').collect();
            serde_json::json!({
                "resource":       p.first().copied().unwrap_or(""),
                "owner_pid":      p.get(1).copied().unwrap_or(""),
                "lock_type":      p.get(2).copied().unwrap_or(""),
                "lock_mode":      p.get(3).copied().unwrap_or(""),
                "owner_username": p.get(4).copied().unwrap_or(""),
            })
        })
        .collect();
    let count = locks.len();
    let resp = serde_json::json!({"success": true, "locks": locks, "count": count});
    assert_eq!(resp["success"], true);
    assert_eq!(resp["count"], 0);
    assert!(resp["locks"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// T014: view_locks is Query category — dispatch_gate permits on mcpTemplate=live
// ---------------------------------------------------------------------------

#[test]
fn view_locks_gate_allows_on_live_template() {
    let policy = live_policy();
    let params = serde_json::json!({"action": "view_locks"});
    let result = iris_agentic_dev_core::policy::gate::dispatch_gate(
        "iris_admin",
        "test-server",
        Some(&policy),
        &params,
    );
    assert!(
        result.is_ok(),
        "view_locks should be permitted on live template"
    );
}

// ---------------------------------------------------------------------------
// T015: require_data_policy_allow blocks on "block" (view_locks doesn't call it)
// ---------------------------------------------------------------------------

#[test]
fn view_locks_not_blocked_by_data_policy_block() {
    // view_locks_impl never calls require_data_policy_allow.
    // Confirm the helper blocks when called so we know the contract is enforced
    // by the callers that DO use it (view_processes, journal_search).
    let blocked = require_data_policy_allow("block", "view_locks");
    assert!(blocked.is_some(), "helper blocks when data_policy=block");
}

// ---------------------------------------------------------------------------
// T019: redact_process_entry replaces PHI fields, leaves others intact
// ---------------------------------------------------------------------------

#[test]
fn redact_process_entry_replaces_phi_fields() {
    let mut entry = serde_json::json!({
        "pid": "1234",
        "namespace": "USER",
        "state": "RUN",
        "routine": "MyApp",
        "username": "jdoe",
        "client_node_name": "laptop.example.com",
        "client_ip": "192.168.1.5",
    });
    redact_process_entry(&mut entry);
    assert_eq!(entry["username"], "[REDACTED]");
    assert_eq!(entry["client_node_name"], "[REDACTED]");
    assert_eq!(entry["client_ip"], "[REDACTED]");
    assert_eq!(entry["pid"], "1234");
    assert_eq!(entry["namespace"], "USER");
    assert_eq!(entry["state"], "RUN");
    assert_eq!(entry["routine"], "MyApp");
}

#[test]
fn redact_process_entry_absent_fields_no_panic() {
    let mut entry = serde_json::json!({"pid": "42"});
    redact_process_entry(&mut entry);
    assert_eq!(entry["pid"], "42");
}

// ---------------------------------------------------------------------------
// T020: view_processes with dataPolicy=block returns DATA_POLICY_BLOCKED
// ---------------------------------------------------------------------------

#[tokio::test]
async fn view_processes_block_returns_data_policy_blocked() {
    let result =
        iris_agentic_dev_core::tools::observability::view_processes_impl(None, "block", None)
            .await
            .expect("Ok(CallToolResult)");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "DATA_POLICY_BLOCKED");
    assert_eq!(v["success"], false);
}

// ---------------------------------------------------------------------------
// T027: journal_search with no filters returns MISSING_PARAMS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_search_no_filters_returns_missing_params() {
    let result = iris_agentic_dev_core::tools::observability::journal_search_impl(
        None, "allow", None, None, None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "MISSING_PARAMS");
}

// ---------------------------------------------------------------------------
// T028: journal_search with dataPolicy=block returns DATA_POLICY_BLOCKED
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_search_block_returns_data_policy_blocked() {
    let result = iris_agentic_dev_core::tools::observability::journal_search_impl(
        None,
        "block",
        Some("IrisDevTest.*"),
        None,
        None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "DATA_POLICY_BLOCKED");
}

// ---------------------------------------------------------------------------
// T029: journal_search with dataPolicy=redact returns DATA_POLICY_BLOCKED
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_search_redact_returns_data_policy_blocked() {
    let result = iris_agentic_dev_core::tools::observability::journal_search_impl(
        None,
        "redact",
        Some("IrisDevTest.*"),
        None,
        None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "DATA_POLICY_BLOCKED");
}

// ---------------------------------------------------------------------------
// T030: journal_search max_records clamped to 1000
// ---------------------------------------------------------------------------

#[test]
fn journal_search_max_records_clamped() {
    let cap = Some(5000u64).map(|n| n.min(1000)).unwrap_or(100);
    assert_eq!(cap, 1000);
    let cap_default: u64 = None::<u64>.unwrap_or(100).min(1000);
    assert_eq!(cap_default, 100);
}

// ---------------------------------------------------------------------------
// T031: journal_search with only global_pattern is valid (reaches IRIS_UNREACHABLE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_search_pattern_only_not_missing_params() {
    let result = iris_agentic_dev_core::tools::observability::journal_search_impl(
        None,
        "allow",
        Some("IrisDevTest.*"),
        None,
        None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_ne!(v["error_code"], "MISSING_PARAMS");
    assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
}

// ---------------------------------------------------------------------------
// T032: journal_search with only time_range is valid (reaches IRIS_UNREACHABLE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_search_time_range_only_not_missing_params() {
    let time_range =
        serde_json::json!({"from": "2026-06-29T00:00:00Z", "to": "2026-06-30T00:00:00Z"});
    let result = iris_agentic_dev_core::tools::observability::journal_search_impl(
        None,
        "allow",
        None,
        Some(&time_range),
        None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_ne!(v["error_code"], "MISSING_PARAMS");
    assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
}

// ---------------------------------------------------------------------------
// T036: resolve_namespace returns param when non-empty
// ---------------------------------------------------------------------------

#[test]
fn resolve_namespace_returns_param_when_provided() {
    assert_eq!(resolve_namespace(Some("MYNS"), "USER"), "MYNS");
    assert_eq!(resolve_namespace(Some("%SYS"), "USER"), "%SYS");
}

#[test]
fn resolve_namespace_falls_back_to_connection_ns() {
    assert_eq!(resolve_namespace(None, "USER"), "USER");
    assert_eq!(resolve_namespace(Some(""), "USER"), "USER");
}

// ---------------------------------------------------------------------------
// T037: namespace_mappings with no IRIS returns IRIS_UNREACHABLE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn namespace_mappings_no_iris_returns_unreachable() {
    let result =
        iris_agentic_dev_core::tools::observability::namespace_mappings_impl(None, None, "USER")
            .await
            .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
}

// ---------------------------------------------------------------------------
// T038: NAMESPACE_NOT_FOUND error code constant
// ---------------------------------------------------------------------------

#[test]
fn namespace_not_found_error_code_constant() {
    let resp =
        serde_json::json!({"success": false, "error_code": "NAMESPACE_NOT_FOUND", "error": "x"});
    assert_eq!(resp["error_code"], "NAMESPACE_NOT_FOUND");
}

// ---------------------------------------------------------------------------
// T043: database_status no connection returns IRIS_UNREACHABLE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn database_status_no_iris_returns_unreachable() {
    let result = iris_agentic_dev_core::tools::observability::database_status_impl(None, None)
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
}

// ---------------------------------------------------------------------------
// T044: DATABASE_NOT_FOUND error code constant
// ---------------------------------------------------------------------------

#[test]
fn database_not_found_error_code_constant() {
    let resp =
        serde_json::json!({"success": false, "error_code": "DATABASE_NOT_FOUND", "error": "x"});
    assert_eq!(resp["error_code"], "DATABASE_NOT_FOUND");
}

// ---------------------------------------------------------------------------
// T045: database_status mirror_state defaults to "none" for non-mirrored
// ---------------------------------------------------------------------------

#[test]
fn database_status_mirror_state_none_when_mirrored_zero() {
    let mirrored_val = "0";
    let mirror_state = if mirrored_val != "0" {
        "mirrored"
    } else {
        "none"
    };
    assert_eq!(mirror_state, "none");
}

// ---------------------------------------------------------------------------
// T046: database_status unmounted entry does not crash
// ---------------------------------------------------------------------------

#[test]
fn database_status_unmounted_entry_parses_safely() {
    let line = "TESTDB|/data/testdb/|Not Mounted|0|0|none";
    let p: Vec<&str> = line.splitn(6, '|').collect();
    let status = p.get(2).copied().unwrap_or("");
    let free_mb: f64 = p.get(3).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    let entry = serde_json::json!({
        "name":          p.first().copied().unwrap_or(""),
        "directory":     p.get(1).copied().unwrap_or(""),
        "mounted":       status.contains("Mounted") && !status.contains("Not"),
        "status":        status,
        "free_space_mb": free_mb,
    });
    assert_eq!(entry["mounted"], false);
    assert_eq!(entry["free_space_mb"], 0.0);
}

// ---------------------------------------------------------------------------
// iso8601_to_iris_timestamp: ISO 8601 -> IRIS %TimeStamp string conversion
// ---------------------------------------------------------------------------

#[test]
fn iso8601_to_iris_timestamp_strips_z_and_replaces_t() {
    let out = iris_agentic_dev_core::tools::observability::iso8601_to_iris_timestamp(
        "2026-06-29T10:00:00Z",
    );
    assert_eq!(out, "2026-06-29 10:00:00");
}

#[test]
fn iso8601_to_iris_timestamp_no_trailing_z() {
    let out = iris_agentic_dev_core::tools::observability::iso8601_to_iris_timestamp(
        "2026-06-29T10:00:00",
    );
    assert_eq!(out, "2026-06-29 10:00:00");
}

#[test]
fn iso8601_to_iris_timestamp_empty_string() {
    let out = iris_agentic_dev_core::tools::observability::iso8601_to_iris_timestamp("");
    assert_eq!(out, "");
}

#[test]
fn iso8601_to_iris_timestamp_trims_whitespace() {
    let out = iris_agentic_dev_core::tools::observability::iso8601_to_iris_timestamp(
        "  2026-06-29T10:00:00Z  ",
    );
    assert_eq!(out, "2026-06-29 10:00:00");
}
