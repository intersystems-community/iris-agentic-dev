//! Regression test for 076-interface-modernization User Story 1's Acceptance Scenario 2:
//! "the actual response validates against [the declared] schema."
//!
//! These are real tool calls through `call_for_test` — the same dispatch path the CLI and
//! MCP transport both use — not mocks. They're unit tests, not live-IRIS integration tests,
//! because every tool exercised here is one that genuinely needs no IRIS connection to
//! produce its real response (bundled skills read from disk, in-process call history, local
//! filesystem symbol scanning) — `IrisTools::new(None)` is this project's own supported
//! disconnected mode, not a workaround. Tools whose declared output schema can only be
//! exercised against a live container (`iris_symbols`, `docs_introspect`,
//! `debug_map_int_to_cls`, `debug_source_map`, `iris_ws_open`/`iris_ws_exec`/`iris_ws_close`)
//! are NOT covered here — per this project's testing policy, that coverage belongs in an
//! `--include-ignored` live test, not a mocked substitute.

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

fn tools() -> IrisTools {
    IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new")
}

async fn call(tools: &IrisTools, tool: &str, args: serde_json::Value) -> serde_json::Value {
    let result = tools
        .call_for_test(tool, args)
        .await
        .unwrap_or_else(|e| panic!("{tool} call failed: {e}"));
    for content in &result.content {
        if let Some(text) = content.as_text() {
            return serde_json::from_str(&text.text)
                .unwrap_or_else(|e| panic!("{tool} returned non-JSON text: {e}"));
        }
    }
    panic!("{tool} returned no text content");
}

#[tokio::test]
async fn test_skill_list_response_matches_declared_shape() {
    let body = call(&tools(), "skill_list", serde_json::json!({})).await;
    assert!(body["skills"].is_array(), "skills must be an array");
    assert!(
        body["count"].is_u64(),
        "count must be a non-negative integer"
    );
    assert!(
        body["sources"].is_object(),
        "sources must be an object (bundled/synthesized counts)"
    );
}

#[tokio::test]
async fn test_skill_community_list_response_matches_declared_shape() {
    let body = call(&tools(), "skill_community_list", serde_json::json!({})).await;
    assert!(body["skills"].is_array());
    assert!(body["kb_items"].is_array());
    assert!(body["skill_count"].is_u64());
    assert!(body["kb_count"].is_u64());
    assert!(body["hint"].is_string());
}

#[tokio::test]
async fn test_agent_stats_response_matches_declared_shape() {
    let body = call(&tools(), "agent_stats", serde_json::json!({})).await;
    assert!(body["status"].is_string());
    assert!(body["skill_count"].is_u64());
    assert!(body["session_calls"].is_u64());
    assert!(body["learning_enabled"].is_boolean());
}

#[tokio::test]
async fn test_agent_history_response_matches_declared_shape() {
    let body = call(&tools(), "agent_history", serde_json::json!({"limit": 5})).await;
    assert!(body["calls"].is_array());
    assert!(body["limit"].is_u64());
    // Each call entry, if any, must carry every field AgentHistoryCall declares.
    for c in body["calls"].as_array().unwrap() {
        assert!(c["tool"].is_string());
        assert!(c["success"].is_boolean());
        assert!(c["ago_secs"].is_u64());
        assert!(c["duration_ms"].is_u64());
        assert!(c["session_id"].is_string());
    }
}

#[tokio::test]
async fn test_kb_recall_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "kb_recall",
        serde_json::json!({"query": "objectscript"}),
    )
    .await;
    assert!(body["query"].is_string());
    assert!(body["results"].is_array());
    assert!(body["count"].is_u64());
    for hit in body["results"].as_array().unwrap() {
        assert!(hit["title"].is_string());
        assert!(hit["snippet"].is_string());
        assert!(hit["source"].is_string());
        assert!(hit["score"].is_number());
    }
}

/// The four stub tools (skill_propose/skill_optimize/skill_share/skill_community_install)
/// unconditionally return err_json("NOT_IMPLEMENTED", ...) regardless of IRIS connectivity —
/// a real, deterministic response with no live IRIS needed and no side effects, unlike the
/// server-config mutation tools in batch 2 that were deliberately left uncovered here.
#[tokio::test]
async fn test_skill_propose_response_matches_declared_shape() {
    let body = call(&tools(), "skill_propose", serde_json::json!({})).await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "NOT_IMPLEMENTED");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_skill_optimize_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "skill_optimize",
        serde_json::json!({"name": "objectscript-tdd"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "NOT_IMPLEMENTED");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_skill_share_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "skill_share",
        serde_json::json!({"name": "objectscript-tdd"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "NOT_IMPLEMENTED");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_skill_community_install_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "skill_community_install",
        serde_json::json!({"name": "some-package"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "NOT_IMPLEMENTED");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_iris_symbols_local_response_matches_declared_shape() {
    // No IRIS connection needed — scans the local filesystem. workspace_path defaults to cwd.
    let body = call(
        &tools(),
        "iris_symbols_local",
        serde_json::json!({"query": "*"}),
    )
    .await;
    assert!(body["source"] == "local_filesystem");
    assert!(body["symbols"].is_array());
    assert!(body["count"].is_u64());
    assert!(body["query_hint"].is_string());
    assert!(body["parse_warnings"].is_array());
}

#[tokio::test]
async fn test_skill_describe_not_found_response_matches_declared_shape() {
    // Bundled-skill lookup path needs no IRIS connection; a name that matches no bundled or
    // synthesized skill deterministically hits the NOT_FOUND branch.
    let body = call(
        &tools(),
        "skill_describe",
        serde_json::json!({"name": "definitely-not-a-real-skill-name"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "NOT_FOUND");
    assert!(body["error"].is_string());
    assert!(body["sources"].is_object());
    assert!(body["note"].is_string());
}

#[tokio::test]
async fn test_skill_search_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "skill_search",
        serde_json::json!({"query": "objectscript"}),
    )
    .await;
    assert!(body["query"].is_string());
    assert!(body["results"].is_array());
    assert!(body["count"].is_u64());
    assert!(body["sources"].is_object());
}

#[tokio::test]
async fn test_iris_get_log_list_response_matches_declared_shape() {
    // No `id` — the listing path, backed entirely by the in-process LogStore, no IRIS needed.
    let body = call(&tools(), "iris_get_log", serde_json::json!({})).await;
    assert_eq!(body["success"], true);
    assert!(body["logs"].is_array());
}

#[tokio::test]
async fn test_iris_credential_manage_no_connection_response_matches_declared_shape() {
    // interop_credential_manage_impl takes Option<&IrisConnection> and returns a real,
    // deterministic IRIS_UNREACHABLE error when there's no connection — no live IRIS needed.
    let body = call(
        &tools(),
        "iris_credential_manage",
        serde_json::json!({"action": "create", "id": "test", "username": "u", "password": "p"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_iris_lookup_manage_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_lookup_manage",
        serde_json::json!({"action": "list_tables"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_iris_lookup_transfer_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_lookup_transfer",
        serde_json::json!({"action": "export", "table": "SomeTable"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

// batch 5: iris_message_body/iris_business_rule_info/iris_production_diff all resolve their
// connection via `self.iris_arc()` (never `resolve_server`/`get_iris_reloaded`, which would
// fail via `?` instead) when no `server` param is given — with no connection, that's `None`,
// and each impl function's own `Option<&IrisConnection>` match returns a real, deterministic
// IRIS_UNREACHABLE error, not a mock.

/// `iris_message_body` is a bulk-PHI tool, so it never reaches its connection check here: gate [2]
/// blocks it first. Passing `dataPolicy: "allow"` in the params used to get past that gate, which
/// is the self-authorization hole 1.3.2 closed — the policy is read from `[policy.<server>]`, never
/// from the caller. The shape contract is the point of this test either way, so assert it on the
/// response the tool actually returns.
#[tokio::test]
async fn test_iris_message_body_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_message_body",
        serde_json::json!({"message_id": "1", "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(
        body["error_code"], "DATA_POLICY_BLOCKED",
        "caller-supplied dataPolicy must not unlock a bulk-PHI tool: {body}"
    );
    assert!(body["message"].is_string() || body["error"].is_string());
}

#[tokio::test]
async fn test_iris_business_rule_info_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_business_rule_info",
        serde_json::json!({"action": "list"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_iris_production_diff_no_connection_response_matches_declared_shape() {
    let body = call(&tools(), "iris_production_diff", serde_json::json!({})).await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

// batch 16: iris_interop_query resolves the same way (`self.iris_arc()`, never
// `resolve_server`) for all three of its `what` sub-actions.

#[tokio::test]
async fn test_iris_interop_query_logs_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_interop_query",
        serde_json::json!({"what": "logs"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_iris_interop_query_queues_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_interop_query",
        serde_json::json!({"what": "queues"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_iris_interop_query_messages_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_interop_query",
        serde_json::json!({"what": "messages"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

// batch 17: iris_production_item resolves its connection via `self.iris_arc()` as well.

#[tokio::test]
async fn test_iris_production_item_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_production_item",
        serde_json::json!({"action": "get_settings", "item": "SomeItem"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

// batch 18: iris_production resolves its connection via `self.iris_arc()` as well.

#[tokio::test]
async fn test_iris_production_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_production",
        serde_json::json!({"action": "status"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

// batch 19: iris_admin resolves its connection via `self.iris_arc()` as well (only the
// Merged toolset advertises this tool, hence Toolset::Merged in `tools()`'s own default).

#[tokio::test]
async fn test_iris_admin_no_connection_response_matches_declared_shape() {
    let body = call(
        &tools(),
        "iris_admin",
        serde_json::json!({"action": "list_namespaces"}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
}

// batch 22: check_config never touches IRIS at all (reads only in-process connection
// state) — unlike every other tool in this file, its single response shape is exercised
// directly rather than via a deterministic no-connection error, since there is no error
// path to hit: this is the tool's one and only real shape.
#[tokio::test]
async fn test_check_config_response_matches_declared_shape() {
    let body = call(&tools(), "check_config", serde_json::json!({})).await;
    assert!(body["connected"].is_boolean());
    assert!(body["connection_source"].is_string());
    assert!(body["host"].is_string());
    assert!(body["port"].is_u64());
    assert!(body["namespace"].is_string());
    // container/config_file/config_loaded_at/iris_version/config_watch_path/
    // objectscript_workspace are all present but nullable — just confirm the keys exist.
    for key in [
        "container",
        "config_file",
        "config_loaded_at",
        "iris_version",
        "config_watch_path",
        "objectscript_workspace",
    ] {
        assert!(body.get(key).is_some(), "{key} must be present (nullable)");
    }
    assert!(body["write_tools_enabled"].is_boolean());
    assert!(body["capabilities"].is_object());
    assert!(body["capabilities"]["private_web_server"].is_boolean());
    assert!(body["capabilities"]["atelier_rest"].is_boolean());
    assert!(body["capabilities"]["compile_path"].is_string());
    assert!(body["server_manager"].is_object());
    assert!(body["server_manager"]["available"].is_boolean());
}

/// T027 (085). `check_config` is how an operator finds out what the gate is, so its four gate
/// fields have to be in the payload *and* in the declared schema. Those are two different places
/// and they have already disagreed: `server_version` has been in the body since v1.0.0, is
/// advertised first in the tool's own description, and was never in `CheckConfigOk` — a declared
/// contract that omits the field it tells you to read. Asserting the body alone would let the same
/// gap open again for the gate fields.
#[tokio::test]
async fn test_check_config_reports_the_gate_and_its_source() {
    let body = call(&tools(), "check_config", serde_json::json!({})).await;

    assert!(
        body["server_version"].is_string(),
        "server_version is how an operator identifies the running build: {body}"
    );
    assert!(
        body["write_tools_enabled"].is_boolean(),
        "write_tools_enabled must stay a bool — existing probes parse it: {body}"
    );
    assert!(
        body["destructive_tools_enabled"].is_boolean(),
        "destructive_tools_enabled: the key has been accepted since v1.0.0 and never reported: \
         {body}"
    );

    // The source fields are the whole point of FR-004: a future mismatch between what the operator
    // declared and what the server decided has to be diagnosable from one call, not from four
    // rounds of issue comments. So an empty or absent string is a failure, and the value has to be
    // one the data model actually defines.
    const SOURCES: &[&str] = &[
        "operator_env",
        "config_file",
        "legacy_allow_prod",
        "inferred_system_mode",
        "inferred_namespace",
        "inferred_default",
        "fail_closed",
    ];
    for key in ["write_tools_source", "destructive_tools_source"] {
        let got = body[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} must be a non-null string: {body}"));
        assert!(
            SOURCES.contains(&got),
            "{key} = {got:?} is not a GateSource wire value; expected one of {SOURCES:?}"
        );
    }

    // The data-model invariant, from the response an operator actually sees.
    if body["destructive_tools_enabled"] == serde_json::json!(true) {
        assert_eq!(
            body["write_tools_enabled"],
            serde_json::json!(true),
            "the destructive tier cannot be on with writes off: {body}"
        );
    }
}

/// The declared half of the same contract. The payload says nothing about what the schema promises,
/// so this reads the `outputSchema` the router actually serves on `tools/list`. A field in the body
/// but not the schema is the `server_version` defect; a field in the schema but not the body is the
/// inverse and just as wrong, so both directions are checked against the live payload.
#[tokio::test]
async fn test_check_config_declared_schema_carries_the_gate_fields() {
    let tools = tools();
    let schema = tools
        .tool_output_schema("check_config")
        .expect("check_config must declare an output schema");
    let props = schema["properties"]
        .as_object()
        .expect("check_config's output schema must have properties");

    for field in [
        "server_version",
        "write_tools_enabled",
        "write_tools_source",
        "destructive_tools_enabled",
        "destructive_tools_source",
    ] {
        assert!(
            props.contains_key(field),
            "check_config's declared schema omits {field}, so the schema and the payload disagree. \
             Declared: {:?}",
            props.keys().collect::<Vec<_>>()
        );
    }

    // And the other direction: nothing the response carries may be missing from the schema.
    let body = call(&tools, "check_config", serde_json::json!({})).await;
    let undeclared: Vec<&String> = body
        .as_object()
        .expect("check_config must return an object")
        .keys()
        .filter(|k| !props.contains_key(*k))
        .collect();
    assert!(
        undeclared.is_empty(),
        "check_config returns fields its declared schema never mentions: {undeclared:?}"
    );
}
