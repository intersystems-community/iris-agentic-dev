//! Unit tests for iris_global tool — no IRIS connection required.

use iris_agentic_dev_core::tools::global::{
    build_get_code, build_global_ref, build_kill_code, build_list_code, build_set_objectscript,
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

// ---------------------------------------------------------------------------
// T032-T050: parse_get_output edge cases and state combinations
// ---------------------------------------------------------------------------

#[test]
fn parse_get_output_defined_with_value() {
    // Internal helper test — imports for testing internal functions
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("1|hello-052");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "1|hello-052");
}

#[test]
fn parse_get_output_undefined() {
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("0|");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "0|");
}

#[test]
fn parse_get_output_empty_string_value() {
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("1|");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "1|");
}

#[test]
fn parse_get_output_with_pipe_in_value() {
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("1|value|with|pipes");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "1|value|with|pipes");
}

#[test]
fn parse_execute_output_error_with_whitespace() {
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("  ERROR: some error msg  ");
    assert!(result.is_err());
    let v = result.unwrap_err();
    assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    assert!(v["message"].as_str().unwrap().contains("some error msg"));
}

#[test]
fn parse_execute_output_multiline_with_error_prefix() {
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("ERROR: <SYNTAX>");
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
    use iris_agentic_dev_core::tools::global;
    // Should NOT match "error:" in lowercase
    let result = global::parse_execute_output("error: something");
    assert!(
        result.is_ok(),
        "lowercase 'error:' should not trigger error path"
    );
}

#[test]
fn parse_execute_output_error_with_special_chars() {
    use iris_agentic_dev_core::tools::global;
    let result = global::parse_execute_output("ERROR: <TAG>message</TAG>");
    assert!(result.is_err());
    assert!(result.unwrap_err()["message"]
        .as_str()
        .unwrap()
        .contains("TAG"));
}
