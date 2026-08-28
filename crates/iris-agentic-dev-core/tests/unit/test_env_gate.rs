// Tests for environment template gate (051-phi-policy-env-gates, US1).
//
// Verifies:
// - mcpTemplate=live blocks Execute, Compile, SourceControl
// - mcpTemplate=live permits Query, Search, Docs, Debug, Skill, KB
// - mcpTemplate=test blocks Execute, Compile; permits SourceControl, Query
// - mcpTemplate=dev permits all categories
// - Unknown tools (no category) are permitted (not blocked)
// - Error JSON shape: error_code, template, blocked_category, server_name, message, remediation

use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
use iris_agentic_dev_core::policy::env_gate::check_env_gate;

// ── mcpTemplate=live ─────────────────────────────────────────────────────────

#[test]
fn live_blocks_iris_execute() {
    let r = check_env_gate(
        "iris_execute",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_some(), "live must block iris_execute");
    let j = r.unwrap();
    assert_eq!(j["error_code"], "ENV_GATE_BLOCKED");
    assert_eq!(j["template"], "live");
    assert_eq!(j["blocked_category"], "execute");
    assert_eq!(j["server_name"], "iris-prod");
}

#[test]
fn live_blocks_iris_compile() {
    let r = check_env_gate(
        "iris_compile",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_some(), "live must block iris_compile");
    assert_eq!(r.unwrap()["blocked_category"], "compile");
}

#[test]
fn live_blocks_iris_source_control() {
    let r = check_env_gate(
        "iris_source_control",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_some(), "live must block iris_source_control");
    assert_eq!(r.unwrap()["blocked_category"], "source_control");
}

#[test]
fn live_permits_iris_query() {
    let r = check_env_gate(
        "iris_query",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "live must permit iris_query");
}

#[test]
fn live_permits_iris_search() {
    let r = check_env_gate(
        "iris_search",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "live must permit iris_search");
}

#[test]
fn live_permits_docs_introspect() {
    let r = check_env_gate(
        "docs_introspect",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "live must permit docs_introspect");
}

#[test]
fn live_permits_debug_capture_packet() {
    let r = check_env_gate(
        "debug_capture_packet",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "live must permit debug tools");
}

#[test]
fn live_permits_skill_list() {
    let r = check_env_gate(
        "skill_list",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "live must permit skill tools");
}

#[test]
fn live_permits_kb_recall() {
    let r = check_env_gate(
        "kb_recall",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "live must permit KB tools");
}

// ── mcpTemplate=test ─────────────────────────────────────────────────────────

#[test]
fn test_blocks_iris_execute() {
    let r = check_env_gate(
        "iris_execute",
        &McpTemplate::Test,
        "iris-staging",
        &serde_json::Value::Null,
    );
    assert!(r.is_some(), "test must block iris_execute");
    assert_eq!(r.unwrap()["template"], "test");
}

#[test]
fn test_blocks_iris_compile() {
    let r = check_env_gate(
        "iris_compile",
        &McpTemplate::Test,
        "iris-staging",
        &serde_json::Value::Null,
    );
    assert!(r.is_some(), "test must block iris_compile");
}

#[test]
fn test_permits_iris_source_control() {
    let r = check_env_gate(
        "iris_source_control",
        &McpTemplate::Test,
        "iris-staging",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "test must permit iris_source_control");
}

#[test]
fn test_permits_iris_query() {
    let r = check_env_gate(
        "iris_query",
        &McpTemplate::Test,
        "iris-staging",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "test must permit iris_query");
}

// ── mcpTemplate=dev ───────────────────────────────────────────────────────────

#[test]
fn dev_permits_all_tools() {
    for tool in &[
        "iris_compile",
        "iris_execute",
        "iris_query",
        "iris_source_control",
        "docs_introspect",
        "debug_capture_packet",
        "skill_list",
        "kb_recall",
    ] {
        let r = check_env_gate(tool, &McpTemplate::Dev, "local", &serde_json::Value::Null);
        assert!(r.is_none(), "dev must permit {tool}");
    }
}

// ── unknown tool (no category mapping) ───────────────────────────────────────

#[test]
fn unknown_tool_not_blocked_in_live() {
    let r = check_env_gate(
        "nonexistent_tool_xyz",
        &McpTemplate::Live,
        "iris-prod",
        &serde_json::Value::Null,
    );
    assert!(r.is_none(), "unknown tool: no category → not blocked");
}

// ── Error JSON shape ──────────────────────────────────────────────────────────

#[test]
fn error_json_includes_all_required_fields() {
    let r = check_env_gate(
        "iris_execute",
        &McpTemplate::Live,
        "prod-server",
        &serde_json::Value::Null,
    )
    .unwrap();
    for field in &[
        "error_code",
        "env_gate_blocked",
        "server_name",
        "template",
        "blocked_category",
        "message",
        "remediation",
    ] {
        assert!(
            r.get(field).is_some(),
            "missing field '{field}' in ENV_GATE_BLOCKED error"
        );
    }
    assert_eq!(r["env_gate_blocked"], true);
}

#[test]
fn error_json_server_name_matches_input() {
    let r = check_env_gate(
        "iris_compile",
        &McpTemplate::Live,
        "custom-server",
        &serde_json::Value::Null,
    )
    .unwrap();
    assert_eq!(r["server_name"], "custom-server");
}

// ── T062: dispatch_gate latency < 1ms avg (SC-005) ────────────────────────────

#[test]
fn test_gate_latency_under_1ms() {
    use iris_agentic_dev_core::iris::workspace_config::{
        ConnectionPolicy, DataPolicy, McpTemplate,
    };
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "perf-server".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
        iris_audit: false,
    };
    let params = serde_json::json!({});
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = dispatch_gate("iris_query", "perf-server", Some(&policy), &params);
    }
    let elapsed_ms = start.elapsed().as_millis();
    assert!(
        elapsed_ms < 1000,
        "1000 dispatch_gate calls took {elapsed_ms}ms — must be < 1000ms (1ms avg per SC-005)"
    );
}
