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
        iris_audit: false,
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
        iris_audit: false,
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
        iris_audit: false,
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
        iris_audit: false,
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
        iris_audit: false,
    }
}

fn no_params() -> serde_json::Value {
    serde_json::json!({})
}

// ── No policy: the strict default, not an absence of gates ───────────────────

#[test]
fn no_policy_permits_ordinary_tools() {
    let r = dispatch_gate("iris_execute", "server", None, &no_params());
    assert!(
        r.is_ok(),
        "no policy must not block a tool the default policy permits"
    );
}

/// A connection with no `[policy.<server>]` section used to skip gates [1]–[4] entirely, which
/// made the bulk-PHI hard block opt-in. `journal_search` would dump journal records — global
/// names and values — on any unconfigured connection.
#[test]
fn no_policy_still_blocks_bulk_phi() {
    let r = dispatch_gate("journal_search", "server", None, &no_params());
    assert!(
        r.is_err(),
        "bulk-PHI hard block must fire on an unconfigured connection"
    );
    assert_eq!(r.unwrap_err()["error_code"], "DATA_POLICY_BLOCKED");
}

/// Same hole on the other side: the system blocklist is documented as non-configurable, but with
/// no policy section gate [3] never ran, so `iris_global` reached `^oddDEF` unimpeded.
#[test]
fn no_policy_still_blocks_system_globals() {
    let params = serde_json::json!({ "global_name": "oddDEF", "action": "set" });
    let r = dispatch_gate("iris_global", "server", None, &params);
    assert!(
        r.is_err(),
        "system blocklist must fire on an unconfigured connection"
    );
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

/// The registered tool is `iris_message_body`. This test named `view_message_body` — a tool that has
/// never existed — and passed, because `BULK_PHI_TOOLS` held the same wrong string. Two artifacts
/// agreeing with each other is not evidence; `every_bulk_phi_tool_is_a_registered_tool` in
/// `test_data_policy_gate.rs` is what makes this checkable.
#[test]
fn gate2_iris_message_body_blocked() {
    let r = dispatch_gate(
        "iris_message_body",
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
        iris_audit: false,
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
        iris_audit: false,
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
        iris_audit: false,
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
        iris_audit: false,
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
    let r = dispatch_gate(
        "iris_execute",
        "iris-dev",
        Some(&policy_dev_allow()),
        &params,
    );
    assert!(r.is_err());
    assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn gate0_blocks_iris_execute_obj_compile() {
    let params = serde_json::json!({
        "namespace": "USER",
        "code": r#"do $system.OBJ.Compile("My.Class","ck")"#,
    });
    let r = dispatch_gate(
        "iris_execute",
        "iris-dev",
        Some(&policy_dev_allow()),
        &params,
    );
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
    let r = dispatch_gate(
        "iris_execute",
        "iris-dev",
        Some(&policy_dev_allow()),
        &params,
    );
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
    assert!(
        r.is_ok(),
        "read-mode %Dictionary introspection must be permitted"
    );
}

/// `iris_execute_method` names a class and a method directly, which reaches everything the
/// ObjectScript gate blocks. It carries no `code` param, so gate [0] used to skip it and
/// `class="%SYSTEM.OBJ", method="Delete"` deleted a class with nothing recorded.
#[test]
fn gate0_blocks_execute_method_code_apis() {
    for (class, method) in &[
        ("%SYSTEM.OBJ", "Delete"),
        ("%SYSTEM.OBJ", "Compile"),
        ("%SYSTEM.OBJ", "Load"),
        ("%RoutineMgr", "Delete"),
        ("%Compiler.UDL.TextServices", "SetTextFromString"),
        ("%Dictionary.ClassDefinition", "%DeleteId"),
    ] {
        let params = serde_json::json!({ "class": class, "method": method, "args": [] });
        let r = dispatch_gate("iris_execute_method", "iris-dev", None, &params);
        assert!(
            r.is_err(),
            "iris_execute_method must not reach {class}.{method}"
        );
        assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
    }
}

#[test]
fn gate0_permits_ordinary_execute_method() {
    for (class, method) in &[
        ("%SYSTEM.Version", "GetVersion"),
        ("%SYS.Journal.System", "GetCurrentFile"),
        ("MyApp.Util", "Format"),
        ("%Dictionary.CompiledClass", "%OpenId"),
    ] {
        let params = serde_json::json!({ "class": class, "method": method, "args": [] });
        let r = dispatch_gate("iris_execute_method", "iris-dev", None, &params);
        assert!(r.is_ok(), "{class}.{method} must stay callable");
    }
}

/// `mode="read"` plus `force=true` skips the read-only SQL validation, and gate [0] only looked
/// at `mode="write"` — so this was the way to send DML at the code dictionary without either
/// check running.
#[test]
fn gate0_blocks_forced_read_mode_dictionary_write() {
    let params = serde_json::json!({
        "namespace": "USER",
        "mode": "read",
        "force": true,
        "query": "DELETE FROM %Dictionary.ClassDefinition WHERE ID='My.Class'",
    });
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(
        r.is_err(),
        "force=true must not be a way past the code-edit gate"
    );
    assert_eq!(r.unwrap_err()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn gate0_permits_forced_write_to_app_table() {
    let params = serde_json::json!({
        "namespace": "USER",
        "mode": "read",
        "force": true,
        "query": "DELETE FROM MyApp.Patient WHERE ID=1",
    });
    let r = dispatch_gate("iris_query", "iris-dev", Some(&policy_dev_allow()), &params);
    assert!(
        r.is_ok(),
        "force=true against an application table is not a code edit"
    );
}

/// Every gate rejection has to look like every other tool error. The declared output shape is
/// `{success: false, error_code, ...}`, and until 1.3.2 no gate set `success` at all — so
/// `test_iris_message_body_no_connection_response_matches_declared_shape` read `Null` there the
/// moment gate [2] started firing. A caller that branches on `success` treats a missing field as
/// "not false" and reads the block as a pass.
#[test]
fn every_gate_rejection_carries_success_false_and_an_error_code() {
    // One case per gate, in dispatch order: [0] code-edit, [1] env template, [2] bulk PHI,
    // [3] system blocklist, [4] PHI name.
    let cases: Vec<(&str, ConnectionPolicy, serde_json::Value, &str)> = vec![
        (
            "iris_execute",
            policy_dev_allow(),
            serde_json::json!({"code": "do ##class(%SYSTEM.OBJ).Delete(\"My.Class\")"}),
            "CODE_EDIT_BLOCKED",
        ),
        (
            "iris_compile",
            policy_live(),
            serde_json::json!({"name": "My.Class.cls"}),
            "ENV_GATE_BLOCKED",
        ),
        (
            "iris_message_body",
            policy_dev_block(),
            serde_json::json!({"message_id": "1"}),
            "DATA_POLICY_BLOCKED",
        ),
        (
            "iris_global",
            policy_dev_allow(),
            serde_json::json!({"global_name": "oddDEF"}),
            "SYSTEM_BLOCKLIST",
        ),
        (
            "iris_global",
            policy_dev_allow(),
            serde_json::json!({"global_name": "PAPMI"}),
            "PHI_GATE_BLOCKED",
        ),
    ];

    for (tool, policy, params, expected_code) in cases {
        let e = dispatch_gate(tool, &policy.server_name.clone(), Some(&policy), &params)
            .expect_err(&format!("{tool} must be blocked with {expected_code}"));
        assert_eq!(
            e["error_code"], expected_code,
            "wrong gate fired for {tool}: {e}"
        );
        assert_eq!(
            e["success"],
            serde_json::Value::Bool(false),
            "{expected_code} must set success: false, got {e}"
        );
        assert!(
            e["message"].is_string() || e["error"].is_string(),
            "{expected_code} must carry human-readable text: {e}"
        );
    }
}

/// `iris_admin` multiplexes: `action="journal_search"` runs the same journal reader as the
/// standalone tool. Gate [2] matched only the dispatcher's name, which is not in `BULK_PHI_TOOLS`,
/// so routing through `iris_admin` skipped the bulk-PHI block entirely.
#[test]
fn gate2_blocks_bulk_phi_reached_through_the_iris_admin_action() {
    let policy = policy_dev_block();
    let e = dispatch_gate(
        "iris_admin",
        &policy.server_name.clone(),
        Some(&policy),
        &serde_json::json!({"action": "journal_search"}),
    )
    .expect_err("iris_admin(action=journal_search) must hit the bulk-PHI block");
    assert_eq!(e["error_code"], "DATA_POLICY_BLOCKED", "{e}");
    assert_eq!(e["success"], serde_json::Value::Bool(false), "{e}");
    // The message has to name the action, not the dispatcher, or the operator cannot tell what
    // was refused.
    assert_eq!(e["tool_name"], "journal_search", "{e}");
}

/// The same route must still work when the configured policy permits it, and every other
/// `iris_admin` action must be unaffected by the action-aware check.
#[test]
fn gate2_permits_iris_admin_actions_that_are_not_bulk_phi() {
    let blocked = policy_dev_block();
    for action in [
        "list_namespaces",
        "list_databases",
        "view_locks",
        "view_processes",
        "create_namespace",
    ] {
        dispatch_gate(
            "iris_admin",
            &blocked.server_name.clone(),
            Some(&blocked),
            &serde_json::json!({"action": action}),
        )
        .unwrap_or_else(|e| panic!("iris_admin(action={action}) must not be gate-blocked: {e}"));
    }

    let allowed = policy_dev_allow();
    dispatch_gate(
        "iris_admin",
        &allowed.server_name.clone(),
        Some(&allowed),
        &serde_json::json!({"action": "journal_search"}),
    )
    .expect("dataPolicy=allow in config must permit journal_search");
}

/// A caller-supplied `dataPolicy` must not unlock the action. This is the `iris_message_body`
/// self-authorization hole on the dispatcher route.
#[test]
fn gate2_ignores_caller_supplied_data_policy_on_the_admin_route() {
    let policy = policy_dev_block();
    let e = dispatch_gate(
        "iris_admin",
        &policy.server_name.clone(),
        Some(&policy),
        &serde_json::json!({"action": "journal_search", "dataPolicy": "allow"}),
    )
    .expect_err("caller-supplied dataPolicy must not unlock a bulk-PHI action");
    assert_eq!(e["error_code"], "DATA_POLICY_BLOCKED", "{e}");
    assert_eq!(
        e["data_policy"], "block",
        "the gate must report the configured policy: {e}"
    );
}
