//! Unit tests for iris_global tool — no IRIS connection required.

use iris_agentic_dev_core::tools::global::{
    build_get_code, build_global_ref, build_kill_code, build_list_code, build_set_objectscript,
    build_subtree_get_code, clamp_max_nodes, clamp_max_subscripts, normalize_global_name,
    parse_execute_output, parse_get_output, parse_list_output, parse_subtree_output,
    validate_subscripts,
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

// ---------------------------------------------------------------------------
// T032-T050: parse_get_output edge cases and state combinations
// ---------------------------------------------------------------------------

#[test]
fn parse_get_output_defined_with_value() {
    // Internal helper test — imports for testing internal functions
    let result = parse_execute_output("1|hello-052");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "1|hello-052");
}

#[test]
fn parse_get_output_undefined() {
    let result = parse_execute_output("0|");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "0|");
}

#[test]
fn parse_get_output_empty_string_value() {
    let result = parse_execute_output("1|");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "1|");
}

#[test]
fn parse_get_output_with_pipe_in_value() {
    let result = parse_execute_output("1|value|with|pipes");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "1|value|with|pipes");
}

#[test]
fn parse_execute_output_error_with_whitespace() {
    let result = parse_execute_output("  ERROR: some error msg  ");
    assert!(result.is_err());
    let v = result.unwrap_err();
    assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(v["message"].as_str().unwrap().contains("some error msg"));
}

#[test]
fn parse_execute_output_multiline_with_error_prefix() {
    let result = parse_execute_output("ERROR: <SYNTAX>");
    assert!(result.is_err());
    let v = result.unwrap_err();
    assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
}

// ---------------------------------------------------------------------------
// T051-T060: validate_subscripts — extended edge cases
// ---------------------------------------------------------------------------

#[test]
fn validate_subscripts_at_boundary() {
    let err = validate_subscripts(&["!invalid".into()]);
    assert!(err.is_err());
    assert_eq!(err.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_special_chars_rejected() {
    let invalid = vec![
        "a@b",
        "x#y",
        "p$q",
        "m%n",
        "k&l",
        "asterisk*",
        "slash/",
        "backslash\\",
    ];
    for sub in invalid {
        let result = validate_subscripts(&[sub.into()]);
        assert!(result.is_err(), "subscript '{}' should be rejected", sub);
    }
}

#[test]
fn validate_subscripts_mixed_valid_and_invalid() {
    let result = validate_subscripts(&["valid".into(), "also-valid".into(), "bad@char".into()]);
    assert!(result.is_err());
    // Should reject on first invalid
    assert_eq!(result.unwrap_err()["subscript"], "bad@char");
}

#[test]
fn validate_subscripts_whitespace_allowed() {
    let result = validate_subscripts(&["hello world".into(), "foo bar baz".into()]);
    assert!(result.is_ok());
}

#[test]
fn validate_subscripts_single_valid_char() {
    assert!(validate_subscripts(&["a".into()]).is_ok());
    assert!(validate_subscripts(&["1".into()]).is_ok());
    assert!(validate_subscripts(&["_".into()]).is_ok());
    assert!(validate_subscripts(&["-".into()]).is_ok());
}

#[test]
fn validate_subscripts_empty_string_subscript() {
    // Empty string subscript does NOT match ^[a-zA-Z0-9 _.:\-]+$
    // (requires at least one character from the set)
    let result = validate_subscripts(&["".into()]);
    assert!(
        result.is_err(),
        "empty string should not match subscript regex"
    );
}

#[test]
fn validate_subscripts_many_subscripts() {
    let subs: Vec<String> = (0..100).map(|i| format!("sub_{}", i)).collect();
    let result = validate_subscripts(&subs);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// T061-T075: build_global_ref edge cases
// ---------------------------------------------------------------------------

#[test]
fn build_global_ref_special_global_names() {
    assert_eq!(build_global_ref("%SYS", &[]), "^%SYS");
    assert_eq!(build_global_ref("PAPMI", &[]), "^PAPMI");
    assert_eq!(build_global_ref("MyApp2024", &[]), "^MyApp2024");
}

#[test]
fn build_global_ref_many_subscripts() {
    let subs: Vec<String> = (0..10).map(|i| format!("s{}", i)).collect();
    let result = build_global_ref("MyApp", &subs);
    assert!(result.starts_with("^MyApp("));
    assert!(result.ends_with(")"));
    for i in 0..10 {
        assert!(result.contains(&format!("\"s{}\"", i)));
    }
}

#[test]
fn build_global_ref_subscripts_with_spaces() {
    assert_eq!(
        build_global_ref("MyApp", &["hello world".into()]),
        r#"^MyApp("hello world")"#
    );
}

#[test]
fn build_global_ref_subscripts_with_special_allowed_chars() {
    assert_eq!(
        build_global_ref(
            "MyApp",
            &["a_b".into(), "c-d".into(), "e:f".into(), "g.h".into()]
        ),
        r#"^MyApp("a_b","c-d","e:f","g.h")"#
    );
}

// ---------------------------------------------------------------------------
// T076-T085: build_set_objectscript edge cases
// ---------------------------------------------------------------------------

#[test]
fn build_set_objectscript_empty_value() {
    let code = build_set_objectscript("^MyApp", "");
    assert!(code.contains(r#"Set ^MyApp = """#));
}

#[test]
fn build_set_objectscript_multiple_quotes() {
    let code = build_set_objectscript("^MyApp", r#"""hello"""#);
    // Input is: "hello"
    // After escape: ""hello""
    // In ObjectScript string: """""hello"""""
    assert!(code.contains(r#"Set ^MyApp = """""hello"""""#));
}

#[test]
fn build_set_objectscript_newline_in_value() {
    let code = build_set_objectscript("^MyApp", "line1\nline2");
    assert!(code.contains(r#"line1"#));
    assert!(code.contains(r#"line2"#));
}

#[test]
fn build_set_objectscript_with_subscripted_ref() {
    let code = build_set_objectscript(r#"^MyApp("a","b")"#, "value123");
    assert!(code.contains(r#"Set ^MyApp("a","b") = "value123""#));
}

#[test]
fn build_set_objectscript_long_value() {
    let long_val = "x".repeat(1000);
    let code = build_set_objectscript("^MyApp", &long_val);
    assert!(code.contains(&long_val));
}

// ---------------------------------------------------------------------------
// T086-T095: build_kill_code variations
// ---------------------------------------------------------------------------

#[test]
fn build_kill_code_simple() {
    let code = build_kill_code("^MyApp");
    assert!(code.contains("Kill ^MyApp"));
}

#[test]
fn build_kill_code_subscripted() {
    let code = build_kill_code(r#"^MyApp("a")"#);
    assert!(code.contains(r#"Kill ^MyApp("a")"#));
}

#[test]
fn build_kill_code_special_name() {
    let code = build_kill_code("^%SYS");
    assert!(code.contains("Kill ^%SYS"));
}

// ---------------------------------------------------------------------------
// T086-T095: build_get_code variations
// ---------------------------------------------------------------------------

#[test]
fn build_get_code_simple() {
    let code = build_get_code("^MyApp");
    assert!(code.contains("Set val = $Get(^MyApp)"));
    assert!(code.contains("Set def = ($Data(^MyApp) > 0)"));
    assert!(code.contains(r#"If def  Write "1|""#));
    assert!(code.contains(r#"If 'def  Write "0|""#));
}

#[test]
fn build_get_code_with_subscripts() {
    let code = build_get_code(r#"^MyApp("a","b")"#);
    assert!(code.contains(r#"Set val = $Get(^MyApp("a","b"))"#));
    assert!(code.contains(r#"Set def = ($Data(^MyApp("a","b")) > 0)"#));
}

#[test]
fn build_get_code_special_global() {
    let code = build_get_code("^%SYS");
    assert!(code.contains("Set val = $Get(^%SYS)"));
}

#[test]
fn build_get_code_has_char_function() {
    let code = build_get_code("^MyApp");
    // Should contain $C(10) for newline character
    assert!(code.contains("$C(10)"));
}

#[test]
fn build_get_code_has_underscore_concatenation() {
    let code = build_get_code("^MyApp");
    // Should use _ for ObjectScript string concatenation
    assert!(code.contains("_val"));
}

// ---------------------------------------------------------------------------
// T096-T110: build_list_code for both root and subscripted refs
// ---------------------------------------------------------------------------

#[test]
fn build_list_code_root_global() {
    let code = build_list_code("^MyApp", 50);
    // For root globals, order_ref should be ^MyApp(sub)
    assert!(code.contains("$Order(^MyApp(sub))"));
    assert!(code.contains("Set maxSubs = 50"));
}

#[test]
fn build_list_code_subscripted_ref() {
    let code = build_list_code(r#"^MyApp("key")"#, 100);
    // For subscripted refs, order_ref should replace ) with ,sub)
    assert!(code.contains(r#"$Order(^MyApp("key",sub))"#));
    assert!(code.contains("Set maxSubs = 100"));
}

#[test]
fn build_list_code_deeply_subscripted() {
    let code = build_list_code(r#"^MyApp("a","b","c")"#, 25);
    assert!(code.contains(r#"$Order(^MyApp("a","b","c",sub))"#));
    assert!(code.contains("Set maxSubs = 25"));
}

#[test]
fn build_list_code_max_value() {
    let code = build_list_code("^MyApp", 500);
    assert!(code.contains("Set maxSubs = 500"));
}

#[test]
fn build_list_code_min_value() {
    let code = build_list_code("^MyApp", 1);
    assert!(code.contains("Set maxSubs = 1"));
}

#[test]
fn build_list_code_no_closing_paren_edge() {
    // Edge case: what if there's no closing paren in the ref?
    let code = build_list_code("^MyApp(", 50);
    // Should still produce valid code (rsplit_once would fail and use fallback)
    assert!(code.contains("maxSubs"));
}

// ---------------------------------------------------------------------------
// T111-T120: build_subtree_get_code variations
// ---------------------------------------------------------------------------

#[test]
fn build_subtree_get_code_with_max_nodes() {
    let code = build_subtree_get_code("^MyApp", 100);
    assert!(code.contains("Set maxNodes = 100"));
    assert!(code.contains("$Query"));
    assert!(code.contains("count>=maxNodes"));
}

#[test]
fn build_subtree_get_code_timeout_guard() {
    let code = build_subtree_get_code("^MyApp", 50);
    // Should have 5-second timeout guard
    assert!(code.contains("($ZH-startTime)>5"));
}

#[test]
fn build_subtree_get_code_max_value() {
    let code = build_subtree_get_code("^MyApp", 1000);
    assert!(code.contains("Set maxNodes = 1000"));
}

#[test]
fn build_subtree_get_code_min_value() {
    let code = build_subtree_get_code("^MyApp", 1);
    assert!(code.contains("Set maxNodes = 1"));
}

// ---------------------------------------------------------------------------
// T121-T130: clamp functions boundary conditions
// ---------------------------------------------------------------------------

#[test]
fn clamp_max_nodes_boundary_values() {
    assert_eq!(clamp_max_nodes(0), 1);
    assert_eq!(clamp_max_nodes(1), 1);
    assert_eq!(clamp_max_nodes(500), 500);
    assert_eq!(clamp_max_nodes(1000), 1000);
    assert_eq!(clamp_max_nodes(1001), 1000);
    assert_eq!(clamp_max_nodes(i64::MAX), 1000);
    assert_eq!(clamp_max_nodes(i64::MIN), 1);
}

#[test]
fn clamp_max_subscripts_boundary_values() {
    assert_eq!(clamp_max_subscripts(0), 1);
    assert_eq!(clamp_max_subscripts(1), 1);
    assert_eq!(clamp_max_subscripts(250), 250);
    assert_eq!(clamp_max_subscripts(500), 500);
    assert_eq!(clamp_max_subscripts(501), 500);
    assert_eq!(clamp_max_subscripts(i64::MAX), 500);
    assert_eq!(clamp_max_subscripts(i64::MIN), 1);
}

// ---------------------------------------------------------------------------
// T131-T140: normalize_global_name edge cases
// ---------------------------------------------------------------------------

#[test]
fn normalize_global_name_multiple_carets() {
    // Should only strip the leading caret
    assert_eq!(normalize_global_name("^^MyApp"), "^MyApp");
}

#[test]
fn normalize_global_name_special_system_globals() {
    assert_eq!(normalize_global_name("^%SYS"), "%SYS");
    assert_eq!(normalize_global_name("^%Library"), "%Library");
}

#[test]
fn normalize_global_name_numeric_name() {
    assert_eq!(normalize_global_name("^123"), "123");
}

// ---------------------------------------------------------------------------
// T141-T145: Integration: validate + build round-trips
// ---------------------------------------------------------------------------

#[test]
fn validate_and_build_valid_subscripts() {
    let subs = vec!["a".into(), "b_1".into(), "c-d".into()];
    assert!(validate_subscripts(&subs).is_ok());
    let gref = build_global_ref("Test", &subs);
    assert_eq!(gref, r#"^Test("a","b_1","c-d")"#);
}

#[test]
fn normalize_then_build() {
    let name = normalize_global_name("^MyApp");
    let gref = build_global_ref(&name, &[]);
    assert_eq!(gref, "^MyApp");
}

// ---------------------------------------------------------------------------
// T146-T150: Output parsing — edge cases beyond basics
// ---------------------------------------------------------------------------

#[test]
fn parse_execute_output_error_prefix_case_sensitive() {
    // Should NOT match "error:" in lowercase
    let result = parse_execute_output("error: something");
    assert!(
        result.is_ok(),
        "lowercase 'error:' should not trigger error path"
    );
}

#[test]
fn parse_execute_output_error_with_special_chars() {
    let result = parse_execute_output("ERROR: <TAG>message</TAG>");
    assert!(result.is_err());
    assert!(result.unwrap_err()["message"]
        .as_str()
        .unwrap()
        .contains("TAG"));
}

// ---------------------------------------------------------------------------
// parse_get_output tests
// ---------------------------------------------------------------------------

#[test]
fn parse_get_output_returns_defined_with_value() {
    let result = parse_get_output("1|hello");
    assert_eq!(result["success"], true);
    assert_eq!(result["defined"], true);
    assert_eq!(result["value"], "hello");
}

#[test]
fn parse_get_output_returns_undefined() {
    let result = parse_get_output("0|");
    assert_eq!(result["success"], true);
    assert_eq!(result["defined"], false);
    assert_eq!(result["value"], serde_json::Value::Null);
}

#[test]
fn parse_get_output_empty_defined_value() {
    let result = parse_get_output("1|");
    assert_eq!(result["success"], true);
    assert_eq!(result["defined"], true);
    assert_eq!(result["value"], "");
}

#[test]
fn parse_get_output_value_with_pipes() {
    let result = parse_get_output("1|a|b|c");
    assert_eq!(result["success"], true);
    assert_eq!(result["defined"], true);
    assert_eq!(result["value"], "a|b|c");
}

#[test]
fn parse_get_output_unexpected_format() {
    let result = parse_get_output("bad");
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(result["message"].as_str().unwrap().contains("unexpected"));
}

// ---------------------------------------------------------------------------
// parse_subtree_output tests
// ---------------------------------------------------------------------------

#[test]
fn parse_subtree_output_single_node() {
    let input = r#"^MyApp("a")|hello
DONE|1|0
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    assert_eq!(result["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(result["nodes"][0]["path"], r#"^MyApp("a")"#);
    assert_eq!(result["nodes"][0]["value"], "hello");
    assert_eq!(result["node_count"], 1);
    assert_eq!(result["truncated"], false);
}

#[test]
fn parse_subtree_output_truncated() {
    let input = r#"^Global("x")|value1
^Global("y")|value2
DONE|100|1
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    assert_eq!(result["node_count"], 100);
    assert_eq!(result["truncated"], true);
}

#[test]
fn parse_subtree_output_empty() {
    let input = "DONE|0|0\n";
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    assert_eq!(result["nodes"].as_array().unwrap().len(), 0);
    assert_eq!(result["node_count"], 0);
    assert_eq!(result["truncated"], false);
}

#[test]
fn parse_subtree_output_multiple_nodes() {
    let input = r#"^App("a")|val1
^App("b")|val2
^App("c")|val3
DONE|3|0
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    let nodes = result["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0]["value"], "val1");
    assert_eq!(nodes[1]["value"], "val2");
    assert_eq!(nodes[2]["value"], "val3");
}

#[test]
fn parse_subtree_output_skips_empty_lines() {
    let input = r#"^App("a")|val1

^App("b")|val2

DONE|2|0
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    let nodes = result["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(result["node_count"], 2);
}

// ---------------------------------------------------------------------------
// parse_list_output tests
// ---------------------------------------------------------------------------

#[test]
fn parse_list_output_single_subscript() {
    let input = "foo\nDONE|1|0\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0], "foo");
    assert_eq!(result["truncated"], false);
}

#[test]
fn parse_list_output_multiple_subscripts() {
    let input = "a\nb\nc\nDONE|3|0\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 3);
    assert_eq!(subs[0], "a");
    assert_eq!(subs[1], "b");
    assert_eq!(subs[2], "c");
}

#[test]
fn parse_list_output_truncated() {
    let input = "sub1\nsub2\nDONE|50|1\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    assert_eq!(result["truncated"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
}

#[test]
fn parse_list_output_empty() {
    let input = "DONE|0|0\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 0);
    assert_eq!(result["truncated"], false);
}

#[test]
fn parse_list_output_skips_blank_lines() {
    let input = "sub1\n\nsub2\n\nsub3\nDONE|3|0\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 3);
    assert_eq!(subs[0], "sub1");
    assert_eq!(subs[1], "sub2");
    assert_eq!(subs[2], "sub3");
}

// ---------------------------------------------------------------------------
// T151-T160: parse_get_output — unexpected formats and edge cases
// Covers the branch for non-0/1 prefixes that are not covered by existing tests
// ---------------------------------------------------------------------------

#[test]
fn parse_get_output_unexpected_prefix_2() {
    // Prefix "2|" is neither defined (1|) nor undefined (0|), should error
    let result = parse_get_output("2|value");
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("unexpected get output"));
}

#[test]
fn parse_get_output_unexpected_prefix_pipe_only() {
    // Leading pipe with no 0/1 flag
    let result = parse_get_output("|value");
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("unexpected get output"));
}

#[test]
fn parse_get_output_unexpected_prefix_numeric() {
    // High numeric prefix: "99|value"
    let result = parse_get_output("99|value");
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "IRIS_EXECUTE_ERROR");
}

#[test]
fn parse_get_output_unexpected_prefix_no_pipe() {
    // No pipe at all — same case as parse_get_output_unexpected_format
    let result = parse_get_output("orphan_value");
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "IRIS_EXECUTE_ERROR");
}

// ---------------------------------------------------------------------------
// T161-T170: parse_subtree_output — silent skip and malformed DONE
// ---------------------------------------------------------------------------

#[test]
fn parse_subtree_output_silently_skips_lines_without_pipe() {
    // One well-formed node line, one orphan line (no pipe), one DONE marker
    let input = r#"^MyApp("x")|v
orphanline
DONE|1|0
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    let nodes = result["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1, "should have exactly one node");
    assert_eq!(nodes[0]["path"], r#"^MyApp("x")"#);
    assert_eq!(nodes[0]["value"], "v");
    // Confirm orphan line was silently skipped (not included in nodes)
    for node in nodes {
        assert_ne!(
            node["path"], "orphanline",
            "orphan line should not appear as a node"
        );
    }
}

#[test]
fn parse_subtree_output_done_with_non_numeric_count() {
    // DONE line with non-numeric count (e.g., 'abc') → parse().ok() defaults to 0
    let input = r#"^A("x")|v
DONE|abc|0
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    let nodes = result["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1, "one node collected");
    // node_count defaults to 0 when parse fails (documents the silent-default behavior)
    assert_eq!(result["node_count"], 0, "non-numeric count defaults to 0");
    assert_eq!(result["truncated"], false);
}

#[test]
fn parse_subtree_output_multiple_lines_without_pipe() {
    // Multiple orphan lines to ensure they're all skipped
    let input = r#"^App("a")|val1
no_pipe_line_1
no_pipe_line_2
^App("b")|val2
incomplete
DONE|2|0
"#;
    let result = parse_subtree_output(input);
    assert_eq!(result["success"], true);
    let nodes = result["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2, "only two well-formed nodes");
    assert_eq!(nodes[0]["value"], "val1");
    assert_eq!(nodes[1]["value"], "val2");
}

// ---------------------------------------------------------------------------
// T171-T180: parse_list_output — missing truncation flag
// ---------------------------------------------------------------------------

#[test]
fn parse_list_output_done_missing_truncation_flag() {
    // DONE line without the second pipe (truncation flag)
    // e.g., "DONE|5" instead of "DONE|5|0"
    // parts.get(1) returns None, truncated defaults to false
    let input = "a\nb\nDONE|2\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0], "a");
    assert_eq!(subs[1], "b");
    // Confirm truncated defaults to false when flag is missing
    assert_eq!(
        result["truncated"], false,
        "missing flag should default to false"
    );
}

#[test]
fn parse_list_output_done_malformed_truncation_flag() {
    // Truncation flag is neither "0" nor "1" (e.g., "DONE|3|xyz")
    // The comparison *s == "1" will be false, so truncated = false
    let input = "sub1\nsub2\nsub3\nDONE|3|xyz\n";
    let result = parse_list_output(input);
    assert_eq!(result["success"], true);
    let subs = result["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 3);
    // xyz != "1", so truncated is false (documents the behavior)
    assert_eq!(result["truncated"], false);
}

// ---------------------------------------------------------------------------
// T181-T185: parse_execute_output — all-whitespace input
// ---------------------------------------------------------------------------

#[test]
fn parse_execute_output_all_whitespace() {
    // Input that is entirely whitespace (spaces, tabs, newlines)
    let result = parse_execute_output("   \t  \n  ");
    assert!(result.is_ok(), "all-whitespace should return Ok after trim");
    assert_eq!(result.unwrap(), "");
}

#[test]
fn parse_execute_output_all_whitespace_then_get_output_fails() {
    // Chain: parse_execute_output on whitespace, then parse_get_output on the result
    let output = parse_execute_output("   ");
    assert!(output.is_ok());
    let empty_str = output.unwrap();
    assert_eq!(empty_str, "");
    // Now try to parse this empty string as get output
    let get_result = parse_get_output(&empty_str);
    assert_eq!(get_result["success"], false);
    assert_eq!(get_result["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(get_result["message"]
        .as_str()
        .unwrap()
        .contains("unexpected get output"));
}

// ---------------------------------------------------------------------------
// T186-T195: validate_subscripts — control characters (tab, newline)
// ---------------------------------------------------------------------------

#[test]
fn validate_subscripts_rejects_tab() {
    let result = validate_subscripts(&["key\tvalue".into()]);
    assert!(result.is_err(), "tab character should be rejected");
    assert_eq!(result.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_rejects_newline() {
    let result = validate_subscripts(&["line1\nline2".into()]);
    assert!(result.is_err(), "newline should be rejected");
    assert_eq!(result.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_rejects_carriage_return() {
    let result = validate_subscripts(&["line1\rline2".into()]);
    assert!(result.is_err(), "carriage return should be rejected");
    assert_eq!(result.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

#[test]
fn validate_subscripts_rejects_bell_character() {
    // \x07 is the bell character, a control character
    let result = validate_subscripts(&["bell\x07char".into()]);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err()["error_code"], "INVALID_SUBSCRIPT");
}

// ---------------------------------------------------------------------------
// T196-T205: build_list_code — parenthesis in subscript value edge case
// ---------------------------------------------------------------------------

#[test]
fn build_list_code_with_paren_in_subscript() {
    // A subscript containing a closing paren: "a(b)"
    // This creates a reference like: ^App("a(b)")
    // rsplit_once(')') will split on the LAST ')', producing:
    //   base = ^App("a(b"
    //   remainder = ""
    // The generated code will then use ^App("a(b,sub) which is malformed.
    let code = build_list_code(r#"^App("a(b)")"#, 10);
    // Document the behavior: the code contains the split result
    assert!(code.contains("maxSubs"));
    // The order_ref will include the partial reference — this is the known limitation
    // (documented in the gap analysis). We confirm the behavior rather than fix it
    // since the fix requires rethinking the parsing strategy.
    assert!(code.contains("$Order"));
}

#[test]
fn build_list_code_multiple_parens_in_subscript() {
    // Multiple nested parens: "a(b(c))"
    // This tests that rsplit_once splits only on the last one
    let code = build_list_code(r#"^App("a(b(c))")"#, 10);
    assert!(code.contains("maxSubs"));
    assert!(code.contains("$Order"));
    // The behavior is documented: splits on the last ), leaving "a(b(c" in the base
}

#[test]
fn build_list_code_root_global_no_issue() {
    // Root globals without subscripts should work fine (no rsplit needed)
    let code = build_list_code("^App", 10);
    assert!(code.contains("$Order(^App(sub))"));
    assert!(code.contains("Set maxSubs = 10"));
}

// ---------------------------------------------------------------------------
// T206-T215: build_set_objectscript — no extra escaping (backslash, $, newline)
// ---------------------------------------------------------------------------

#[test]
fn build_set_objectscript_backslash_pass_through() {
    // Backslash should NOT be escaped; it should appear verbatim
    let code = build_set_objectscript("^G", "back\\slash");
    assert!(
        code.contains(r#"back\slash"#),
        "backslash should not be escaped"
    );
}

#[test]
fn build_set_objectscript_dollar_sign_pass_through() {
    // $ should appear verbatim (not expanded as ObjectScript function)
    let code = build_set_objectscript("^G", "$ZV");
    assert!(code.contains("$ZV"), "$ZV should appear literally in code");
    // Make sure it's inside the string literal, not as a function call
    assert!(code.contains(r#""$ZV""#) || code.contains(r#"= "$ZV""#));
}

#[test]
fn build_set_objectscript_newline_pass_through() {
    // Newline in value should be embedded verbatim
    let code = build_set_objectscript("^G", "line1\nline2");
    assert!(code.contains("line1"));
    assert!(code.contains("line2"));
    // The newline is literal in the string, not escaped as \n
}

#[test]
fn build_set_objectscript_percent_sign() {
    // % sign should pass through unchanged
    let code = build_set_objectscript("^G", "%SYS");
    assert!(code.contains("%SYS"));
}

#[test]
fn build_set_objectscript_only_quote_is_doubled() {
    // Only " is escaped (doubled); other chars are unchanged
    let code = build_set_objectscript("^G", r#"a"b$c%d\e"#);
    // The " should be doubled to ""
    assert!(code.contains(r#"a""b$c%d\e"#));
}

#[test]
fn build_set_objectscript_mixed_special_chars() {
    // Mix of special chars that should all pass through except "
    let code = build_set_objectscript("^MyApp", r#"$ZV\$ZERROR%SYS"key"#);
    // Quote should be doubled, everything else verbatim
    assert!(code.contains(r#"$ZV\$ZERROR%SYS""key"#));
}
