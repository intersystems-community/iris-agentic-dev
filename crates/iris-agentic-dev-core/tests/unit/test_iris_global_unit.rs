//! Unit tests for iris_global tool — no IRIS connection required.

use iris_agentic_dev_core::tools::global::{
    build_global_ref, build_kill_code, build_list_code, build_set_objectscript,
    build_subtree_get_code, clamp_max_nodes, clamp_max_subscripts, normalize_global_name,
    parse_execute_output, validate_subscripts,
};

// ---------------------------------------------------------------------------
// T013: normalize_global_name
// ---------------------------------------------------------------------------

#[test]
fn normalize_strips_caret() {
    assert_eq!(normalize_global_name("^MyApp"), "MyApp");
    assert_eq!(normalize_global_name("MyApp"), "MyApp");
    assert_eq!(normalize_global_name("^%SYS"), "%SYS");
    assert_eq!(normalize_global_name("^"), "");
    assert_eq!(normalize_global_name(""), "");
}

// ---------------------------------------------------------------------------
// T014: validate_subscripts allowlist
// ---------------------------------------------------------------------------

#[test]
fn validate_subscripts_accepts_valid() {
    let ok = validate_subscripts(&[
        "a".into(),
        "b_1".into(),
        "hello world".into(),
        "foo.bar".into(),
        "x:y".into(),
        "my-key".into(),
        "UPPER123".into(),
    ]);
    assert!(ok.is_ok(), "expected Ok but got {:?}", ok);
}

#[test]
fn validate_subscripts_rejects_double_quote() {
    let err = validate_subscripts(&[r#"bad"sub"#.into()]);
    assert!(err.is_err());
    let v = err.unwrap_err();
    assert_eq!(v["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_rejects_caret() {
    let err = validate_subscripts(&["^inject".into()]);
    assert!(err.is_err());
    assert_eq!(err.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_rejects_paren() {
    let err = validate_subscripts(&["a)b".into()]);
    assert!(err.is_err());
    assert_eq!(err.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_empty_list_ok() {
    assert!(validate_subscripts(&[]).is_ok());
}

// ---------------------------------------------------------------------------
// T015: build_global_ref
// ---------------------------------------------------------------------------

#[test]
fn build_global_ref_no_subscripts() {
    assert_eq!(build_global_ref("MyApp", &[]), "^MyApp");
}

#[test]
fn build_global_ref_with_subscripts() {
    assert_eq!(
        build_global_ref("MyApp", &["a".into(), "b".into()]),
        r#"^MyApp("a","b")"#
    );
}

#[test]
fn build_global_ref_single_subscript() {
    assert_eq!(build_global_ref("Foo", &["key1".into()]), r#"^Foo("key1")"#);
}

// ---------------------------------------------------------------------------
// T016: missing global_name returns structured error (via handle_iris_global)
// Tested indirectly: serde deserialization failure returns a parsing error.
// We test that validate_subscripts is callable and parse_execute_output covers errors.
// ---------------------------------------------------------------------------

#[test]
fn parse_execute_output_detects_error_prefix() {
    let result = parse_execute_output("ERROR: <UNDEFINED>x+1^Foo");
    assert!(result.is_err());
    let v = result.unwrap_err();
    assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(v["message"].as_str().unwrap().contains("<UNDEFINED>"));
}

#[test]
fn parse_execute_output_passes_clean() {
    let result = parse_execute_output(r#"{"success":true}"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), r#"{"success":true}"#);
}

// ---------------------------------------------------------------------------
// T017: action=get is Query category — NOT blocked by live template
// Test via check_env_gate directly
// ---------------------------------------------------------------------------

#[test]
fn env_gate_get_permitted_on_live() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;

    let params = serde_json::json!({"action": "get", "global_name": "MyApp"});
    let result = check_env_gate("iris_global", &McpTemplate::Live, "test-server", &params);
    assert!(
        result.is_none(),
        "get should NOT be blocked on live: {:?}",
        result
    );
}

#[test]
fn env_gate_list_permitted_on_live() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;

    let params = serde_json::json!({"action": "list", "global_name": "MyApp"});
    let result = check_env_gate("iris_global", &McpTemplate::Live, "test-server", &params);
    assert!(
        result.is_none(),
        "list should NOT be blocked on live: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// T024/T025: action=set/kill blocked on live and test templates
// ---------------------------------------------------------------------------

#[test]
fn env_gate_set_blocked_on_live() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;

    let params = serde_json::json!({"action": "set", "global_name": "MyApp"});
    let result = check_env_gate("iris_global", &McpTemplate::Live, "test-server", &params);
    assert!(result.is_some(), "set MUST be blocked on live");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn env_gate_kill_blocked_on_live() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;

    let params = serde_json::json!({"action": "kill", "global_name": "MyApp"});
    let result = check_env_gate("iris_global", &McpTemplate::Live, "test-server", &params);
    assert!(result.is_some(), "kill MUST be blocked on live");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

#[test]
fn env_gate_set_blocked_on_test() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;

    let params = serde_json::json!({"action": "set", "global_name": "MyApp"});
    let result = check_env_gate("iris_global", &McpTemplate::Test, "test-server", &params);
    assert!(result.is_some(), "set MUST be blocked on test");
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

// ---------------------------------------------------------------------------
// T018: invalid subscript returns INVALID_SUBSCRIPT
// ---------------------------------------------------------------------------

#[test]
fn invalid_subscript_error_code() {
    let err = validate_subscripts(&[r#"bad"char"#.into()]);
    assert!(err.is_err());
    let v = err.unwrap_err();
    assert_eq!(v["error_code"], "INVALID_SUBSCRIPT");
    assert!(v["subscript"].as_str().unwrap().contains("bad"));
}

// ---------------------------------------------------------------------------
// T023: action=set missing value — tested via INVALID_PARAMS path
// We test the output from the handler indirectly via parse_execute_output and
// validate that the code builder produces correct ObjectScript.
// ---------------------------------------------------------------------------

#[test]
fn build_set_objectscript_correct() {
    let code = build_set_objectscript(r#"^MyApp("a","b")"#, "hello");
    // Direct Set — gref embedded literally, no @indirection
    assert!(
        code.contains(r#"Set ^MyApp("a","b") = "hello""#),
        "code: {code}"
    );
}

#[test]
fn build_set_objectscript_escapes_value_quotes() {
    let code = build_set_objectscript("^Foo", r#"say "hi""#);
    // Embedded " should be doubled for ObjectScript string literal
    assert!(code.contains(r#"say ""hi"""#), "quote not escaped: {code}");
}

// ---------------------------------------------------------------------------
// T040b: IRIS_EXECUTE_ERROR parsing (C2)
// ---------------------------------------------------------------------------

#[test]
fn parse_execute_output_protect_error() {
    let out = "ERROR: <PROTECT> Execute+5^MyClass";
    let result = parse_execute_output(out);
    assert!(result.is_err());
    let v = result.unwrap_err();
    assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(v["message"].as_str().unwrap().contains("<PROTECT>"));
}

#[test]
fn parse_execute_output_whitespace_trimmed() {
    let result = parse_execute_output("  {\"success\":true}  ");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), r#"{"success":true}"#);
}

// ---------------------------------------------------------------------------
// T040c: clamp behavior (C3)
// ---------------------------------------------------------------------------

#[test]
fn clamp_max_nodes_upper() {
    assert_eq!(clamp_max_nodes(9999), 1000);
    assert_eq!(clamp_max_nodes(1000), 1000);
    assert_eq!(clamp_max_nodes(100), 100);
}

#[test]
fn clamp_max_nodes_lower() {
    assert_eq!(clamp_max_nodes(0), 1);
    assert_eq!(clamp_max_nodes(-5), 1);
    assert_eq!(clamp_max_nodes(1), 1);
}

#[test]
fn clamp_max_subscripts_upper() {
    assert_eq!(clamp_max_subscripts(9999), 500);
    assert_eq!(clamp_max_subscripts(500), 500);
    assert_eq!(clamp_max_subscripts(50), 50);
}

#[test]
fn clamp_max_subscripts_lower() {
    assert_eq!(clamp_max_subscripts(0), 1);
    assert_eq!(clamp_max_subscripts(-1), 1);
}

// ---------------------------------------------------------------------------
// Additional: verify ObjectScript code builders produce sensible output
// ---------------------------------------------------------------------------

#[test]
fn build_kill_code_contains_kill() {
    let code = build_kill_code("^IrisDevTest");
    // Direct Kill — gref embedded literally, no @indirection
    assert!(code.contains("Kill ^IrisDevTest"), "code: {code}");
    // Output is plain "ok" — no JSON braces in generator output
    assert!(code.contains("\"ok\""), "code: {code}");
}

#[test]
fn build_list_code_contains_order() {
    let code = build_list_code("^IrisDevTest", 50);
    assert!(code.contains("$Order"), "code: {code}");
    assert!(code.contains("50"), "max not in code: {code}");
}

#[test]
fn build_subtree_get_code_contains_query() {
    let code = build_subtree_get_code("^IrisDevTest", 100);
    assert!(code.contains("$Query"), "code: {code}");
    assert!(code.contains("$ZH"), "timeout guard not in code: {code}");
    assert!(code.contains("100"), "max_nodes not in code: {code}");
}

// ---------------------------------------------------------------------------
// T029/T030: system blocklist gate and PHI gate via dispatch_gate
// ---------------------------------------------------------------------------

#[test]
fn dispatch_gate_system_blocklist_blocks_pct_sys() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow), // allow data policy — blocklist still fires
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let params = serde_json::json!({"action": "get", "global_name": "%SYS"});
    let result = dispatch_gate("iris_global", "test-server", Some(&policy), &params);
    assert!(result.is_err(), "^%SYS must be blocked");
    assert_eq!(result.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

#[test]
fn dispatch_gate_phi_gate_blocks_papmi_without_ack() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let params = serde_json::json!({"action": "get", "global_name": "PAPMI"});
    let result = dispatch_gate("iris_global", "test-server", Some(&policy), &params);
    assert!(result.is_err(), "PAPMI without ack must be blocked");
    assert_eq!(result.unwrap_err()["error_code"], "PHI_GATE_BLOCKED");
}

#[test]
fn dispatch_gate_phi_gate_passes_papmi_with_ack() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let params =
        serde_json::json!({"action": "get", "global_name": "PAPMI", "acknowledgePhi": true});
    let result = dispatch_gate("iris_global", "test-server", Some(&policy), &params);
    assert!(result.is_ok(), "PAPMI with ack must pass: {:?}", result);
}

#[test]
fn dispatch_gate_non_phi_global_passes() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let params = serde_json::json!({"action": "get", "global_name": "MyAppData"});
    let result = dispatch_gate("iris_global", "test-server", Some(&policy), &params);
    assert!(result.is_ok(), "non-PHI global must pass: {:?}", result);
}

// T031: kill on non-blocklisted global passes (no-op in IRIS)
#[test]
fn dispatch_gate_kill_non_blocklisted_passes() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    let params = serde_json::json!({"action": "kill", "global_name": "IrisDevTest"});
    let result = dispatch_gate("iris_global", "test-server", Some(&policy), &params);
    assert!(
        result.is_ok(),
        "kill on IrisDevTest must pass: {:?}",
        result
    );
}
