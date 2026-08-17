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
        if let Some(text) = content.raw.as_text() {
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

#[tokio::test]
async fn test_iris_message_body_no_connection_response_matches_declared_shape() {
    // dataPolicy defaults to "block" (PHI-gated) — must opt in past that check to reach the
    // connection check this test is actually exercising.
    let body = call(
        &tools(),
        "iris_message_body",
        serde_json::json!({"message_id": "1", "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    assert_eq!(body["success"], false);
    assert_eq!(body["error_code"], "IRIS_UNREACHABLE");
    assert!(body["error"].is_string());
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
