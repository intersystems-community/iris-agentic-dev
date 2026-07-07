// Tests for dispatch_gate() orchestrator (051-phi-policy-env-gates).
//
// Verifies the 4-gate evaluation order and that each gate fires correctly
// via the unified dispatch_gate() entry point.

use iris_agentic_dev_core::iris::workspace_config::{
    ConnectionPolicy, DataPolicy, McpTemplate, ToolCategory,
};
use iris_agentic_dev_core::policy::gate::dispatch_gate;

fn policy_live() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-prod".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Live),
        data_policy: Some(DataPolicy::Block),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    }
}

fn policy_test() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-staging".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Test),
        data_policy: Some(DataPolicy::Block),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    }
}

fn policy_dev_allow() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-dev".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    }
}

fn policy_dev_block() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-dev".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Block),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    }
}

fn policy_custom_blocklist() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-custom".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Block),
        global_blocklist: vec!["^Secret*".to_string()],
        data_policy_kill_allowlist: vec![],
    }
}

fn no_params() -> serde_json::Value {
    serde_json::json!({})
}

// ── No policy: all tools permitted ───────────────────────────────────────────

#[test]
fn no_policy_always_permits() {
    let r = dispatch_gate("iris_execute", "server", None, &no_params());
    assert!(r.is_ok(), "no policy → all tools permitted");
}

// ── Gate [1]: mcpTemplate env gate ───────────────────────────────────────────

#[test]
fn gate1_live_blocks_iris_execute() {
    let r = dispatch_gate(
        "iris_execute",
        "iris-prod",
        Some(&policy_live()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn gate1_live_blocks_iris_compile() {
    let r = dispatch_gate(
        "iris_compile",
        "iris-prod",
        Some(&policy_live()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn gate1_live_blocks_iris_source_control() {
    let r = dispatch_gate(
        "iris_source_control",
        "iris-prod",
        Some(&policy_live()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn gate1_live_permits_iris_query() {
    let r = dispatch_gate(
        "iris_query",
        "iris-prod",
        Some(&policy_live()),
        &no_params(),
    );
    assert!(r.is_ok(), "live must permit iris_query");
}

#[test]
fn gate1_test_blocks_execute_permits_source_control() {
    let r = dispatch_gate(
        "iris_execute",
        "iris-staging",
        Some(&policy_test()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "ENV_GATE_BLOCKED");

    let r = dispatch_gate(
        "iris_source_control",
        "iris-staging",
        Some(&policy_test()),
        &no_params(),
    );
    assert!(r.is_ok(), "test must permit source_control");
}

#[test]
fn gate1_dev_permits_all_categories() {
    let policy = policy_dev_allow();
    for tool in &[
        "iris_compile",
        "iris_execute",
        "iris_source_control",
        "iris_query",
    ] {
        let r = dispatch_gate(tool, "iris-dev", Some(&policy), &no_params());
        assert!(r.is_ok(), "dev permits {tool}");
    }
}

// ── Gate [2]: bulk-PHI hard-block ─────────────────────────────────────────────

#[test]
fn gate2_journal_search_blocked_when_policy_block() {
    let r = dispatch_gate(
        "journal_search",
        "iris-dev",
        Some(&policy_dev_block()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "DATA_POLICY_BLOCKED");
}

#[test]
fn gate2_journal_search_permitted_when_policy_allow() {
    let r = dispatch_gate(
        "journal_search",
        "iris-dev",
        Some(&policy_dev_allow()),
        &no_params(),
    );
    assert!(r.is_ok(), "journal_search permitted with dataPolicy=allow");
}

#[test]
fn gate2_view_message_body_blocked() {
    let r = dispatch_gate(
        "view_message_body",
        "iris-dev",
        Some(&policy_dev_block()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "DATA_POLICY_BLOCKED");
}

// ── Gate [3]: system global blocklist ────────────────────────────────────────

#[test]
fn gate3_system_global_blocked() {
    let params = serde_json::json!({"global_name": "oddDEF"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

#[test]
fn gate3_percent_sys_blocked() {
    let params = serde_json::json!({"global_name": "%SYS.Security"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

#[test]
fn gate3_custom_blocklist_blocked() {
    let params = serde_json::json!({"global_name": "SecretData"});
    let r = dispatch_gate(
        "iris_query",
        "iris-custom",
        Some(&policy_custom_blocklist()),
        &params,
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

#[test]
fn gate3_camel_case_global_name_param_also_checked() {
    // globalName (camelCase) is also extracted
    let params = serde_json::json!({"globalName": "oddDEF"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

#[test]
fn gate3_app_global_not_blocked() {
    let params = serde_json::json!({"global_name": "MyAppData"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_ok(), "app global must not be blocked");
}

#[test]
fn gate3_kill_action_on_kill_allowlist_permitted() {
    let policy = ConnectionPolicy {
        server_name: "iris-dev".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec!["^TempCache*".to_string()],
        data_policy_kill_allowlist: vec!["^TempCache*".to_string()],
    };
    let params = serde_json::json!({"global_name": "TempCache.Work", "action": "kill"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy), &params);
    assert!(r.is_ok(), "kill op on kill allowlist must be permitted");
}

// ── Gate [4]: PHI name pattern gate ──────────────────────────────────────────

#[test]
fn gate4_phi_global_blocked_without_acknowledge() {
    let params = serde_json::json!({"global_name": "PAPMI"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "PHI_GATE_BLOCKED");
}

#[test]
fn gate4_phi_global_permitted_with_acknowledge() {
    let params = serde_json::json!({"global_name": "PAPMI", "acknowledgePhi": true});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_ok(), "acknowledgePhi=true must bypass PHI gate");
}

#[test]
fn gate4_paadm_blocked() {
    let params = serde_json::json!({"global_name": "PAADM1234"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "PHI_GATE_BLOCKED");
}

// ── Gate ordering: [1] fires before [2], [2] before [3], [3] before [4] ──────

#[test]
fn gate_order_1_before_2() {
    // live blocks execute (gate [1]) even if bulk-PHI would also block
    let r = dispatch_gate(
        "iris_execute",
        "iris-prod",
        Some(&policy_live()),
        &no_params(),
    );
    assert!(r.is_err());
    assert_eq!(
        r.unwrap_err()["error_code"],
        "ENV_GATE_BLOCKED",
        "gate [1] fires before gate [2]"
    );
}

#[test]
fn gate_order_2_before_3() {
    // bulk-PHI tool call with a system global in params — gate [2] fires first
    let params = serde_json::json!({"global_name": "oddDEF"});
    let r = dispatch_gate(
        "journal_search",
        "iris-dev",
        Some(&policy_dev_block()),
        &params,
    );
    assert!(r.is_err());
    assert_eq!(
        r.unwrap_err()["error_code"],
        "DATA_POLICY_BLOCKED",
        "gate [2] fires before gate [3]"
    );
}

#[test]
fn gate_order_3_before_4() {
    // system global that also matches a PHI pattern — gate [3] fires first
    // Ens.MessageHeader is in both SYSTEM_BLOCKLIST and PHI_NAME_PATTERNS
    let params = serde_json::json!({"global_name": "Ens.MessageHeader.1"});
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(
        r.unwrap_err()["error_code"],
        "SYSTEM_BLOCKLIST",
        "gate [3] fires before gate [4]"
    );
}

// ── Default policy values ─────────────────────────────────────────────────────

#[test]
fn default_template_is_dev_all_permitted() {
    // Policy with no mcpTemplate set → defaults to Dev → all categories permitted
    let policy = ConnectionPolicy {
        server_name: "iris-default".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: None,
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let r = dispatch_gate("iris_execute", "iris-default", Some(&policy), &no_params());
    assert!(
        r.is_ok(),
        "missing mcpTemplate defaults to dev, all permitted"
    );
}

#[test]
fn default_data_policy_is_block() {
    // Policy with no dataPolicy → defaults to Block → bulk-PHI blocked
    let policy = ConnectionPolicy {
        server_name: "iris-default".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: None,
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let r = dispatch_gate(
        "journal_search",
        "iris-default",
        Some(&policy),
        &no_params(),
    );
    assert!(r.is_err(), "missing dataPolicy defaults to block");
    assert_eq!(r.unwrap_err()["error_code"], "DATA_POLICY_BLOCKED");
}

// ── Policy allow list interaction ─────────────────────────────────────────────

#[test]
fn dispatch_gate_does_not_check_policy_allow_list() {
    // dispatch_gate only runs the 4 PHI/env gates; policy.allow (category gate) is
    // a separate check (policy_gate) handled by the tool handler, not dispatch_gate
    let policy = ConnectionPolicy {
        server_name: "iris-dev".to_string(),
        allow: Some(vec![ToolCategory::Query]), // compile not in allow list
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    // dispatch_gate permits compile (policy.allow is not its concern)
    let r = dispatch_gate("iris_compile", "iris-dev", Some(&policy), &no_params());
    assert!(
        r.is_ok(),
        "dispatch_gate does not enforce policy.allow list — that is policy_gate's job"
    );
}

// ── Gate [0]: code-edit hard-block (non-configurable) ─────────────────────────

#[test]
fn gate0_blocks_iris_execute_dictionary_edit() {
    let params = serde_json::json!({
        "namespace": "USER",
        "code": r#"set c=##class(%Dictionary.ClassDefinition).%OpenId("My.Class") do c.%Save()"#,
    });
    let r = dispatch_gate("iris_execute", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn gate0_blocks_iris_execute_obj_compile() {
    let params = serde_json::json!({
        "namespace": "USER",
        "code": r#"do $system.OBJ.Compile("My.Class","ck")"#,
    });
    let r = dispatch_gate("iris_execute", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn gate0_fires_even_with_no_policy() {
    // Non-configurable: the code-edit block must fire before the no-policy early return.
    let params = serde_json::json!({
        "namespace": "USER",
        "code": "set ^oddDEF(\"My.Class\")=1",
    });
    let r = dispatch_gate("iris_execute", "server", None, &params);
    assert!(r.is_err(), "code-edit block must fire even with no policy");
    assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn gate0_permits_ordinary_execute() {
    let params = serde_json::json!({ "namespace": "USER", "code": "write $ZVERSION,!" });
    let r = dispatch_gate("iris_execute", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_ok(), "ordinary ObjectScript must not be blocked");
}

#[test]
fn gate0_blocks_iris_query_write_to_dictionary() {
    let params = serde_json::json!({
        "namespace": "USER",
        "mode": "write",
        "query": "UPDATE %Dictionary.MethodDefinition SET Name='x' WHERE parent='My.Class'",
    });
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn gate0_permits_iris_query_read_of_dictionary() {
    // read-mode introspection against %Dictionary must remain allowed.
    let params = serde_json::json!({
        "namespace": "USER",
        "mode": "read",
        "query": "SELECT Name FROM %Dictionary.CompiledClass",
    });
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(r.is_ok(), "read-mode %Dictionary introspection must be permitted");
}
