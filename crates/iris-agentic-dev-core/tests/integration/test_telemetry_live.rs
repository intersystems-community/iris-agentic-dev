//! Integration test for durable telemetry (059-tool-telemetry-benchmark, US2).
//! Requires live IRIS on iris-dev-iris (port from IRIS_HOST/IRIS_WEB_PORT env). Run with:
//!   cargo test -p iris-agentic-dev-core --test test_telemetry_live -- --ignored

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::workspace_config::DataPolicy;
use iris_agentic_dev_core::telemetry::prune::prune_sessions;
use iris_agentic_dev_core::telemetry::{
    filter_records, read_durable, write_durable, ToolCallRecord,
};
use std::sync::Arc;
use uuid::Uuid;

fn live_iris() -> Arc<IrisConnection> {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    Arc::new(IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    ))
}

/// US2 Acceptance Scenario 1/2: more than 50 tool calls in one session persist to the
/// durable IRIS-global sink and are all readable back — not capped at the old 50-entry
/// in-memory limit, and still present after a simulated "restart" (a fresh read against
/// the same durable sink, independent of any in-memory ring buffer).
#[tokio::test]
#[ignore]
async fn live_more_than_50_calls_persist_and_are_fully_readable_after_restart() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let config_dir = std::env::temp_dir().join("iad_test_telemetry_live");
    let session_id = Uuid::new_v4();

    for i in 0..60 {
        let record = ToolCallRecord::now(&format!("iris_execute_{i}"), true, 5, session_id);
        write_durable(&record, Some(Arc::clone(&iris)), &client, &config_dir).await;
    }

    // Simulate "process restart": read back via a fresh call, no in-memory state reused.
    let records = read_durable(
        Some(session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    assert_eq!(
        records.len(),
        60,
        "all 60 calls must be present — not capped at the old 50-entry limit"
    );

    let cleanup = format!("kill ^IRISDEV(\"telemetry\",\"{session_id}\")\n");
    let _ = iris.execute_via_generator(&cleanup, "USER", &client).await;
}

/// US2 Acceptance Scenario 3: a call recorded under a redacting policy has params
/// redacted in the durable record, while tool/success/duration are still recorded.
#[tokio::test]
#[ignore]
async fn live_redacted_params_are_none_in_durable_record_but_other_fields_present() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let config_dir = std::env::temp_dir().join("iad_test_telemetry_live_redact");
    let session_id = Uuid::new_v4();

    let mut record = ToolCallRecord::now("iris_query", true, 12, session_id);
    let params = serde_json::json!({"query": "SELECT * FROM Sensitive"});
    record.params =
        iris_agentic_dev_core::telemetry::redact::redact_params(&params, &DataPolicy::Block);
    assert!(record.params.is_none());

    write_durable(&record, Some(Arc::clone(&iris)), &client, &config_dir).await;
    let records = read_durable(
        Some(session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tool, "iris_query");
    assert!(records[0].success);
    assert_eq!(records[0].duration_ms, 12);
    assert!(records[0].params.is_none());

    let cleanup = format!("kill ^IRISDEV(\"telemetry\",\"{session_id}\")\n");
    let _ = iris.execute_via_generator(&cleanup, "USER", &client).await;
}

/// US2 Acceptance Scenario 4: querying by a specific tool name returns only matching
/// entries.
#[tokio::test]
#[ignore]
async fn live_query_filters_by_tool_name() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let config_dir = std::env::temp_dir().join("iad_test_telemetry_live_filter");
    let session_id = Uuid::new_v4();

    write_durable(
        &ToolCallRecord::now("iris_compile", true, 1, session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    write_durable(
        &ToolCallRecord::now("iris_execute", true, 1, session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;

    let records = read_durable(
        Some(session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    let (matches, _truncated) =
        filter_records(&records, Some("iris_compile"), None, None, None, 100);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].tool, "iris_compile");

    let cleanup = format!("kill ^IRISDEV(\"telemetry\",\"{session_id}\")\n");
    let _ = iris.execute_via_generator(&cleanup, "USER", &client).await;
}

/// FR-011: prune_sessions actually removes durable-sink data for the given session ids
/// from the IRIS-global sink (best-effort deletion, verified via a subsequent read).
#[tokio::test]
#[ignore]
async fn live_prune_sessions_removes_iris_global_data() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let config_dir = std::env::temp_dir().join("iad_test_telemetry_live_prune");
    let session_id = Uuid::new_v4();

    write_durable(
        &ToolCallRecord::now("iris_compile", true, 1, session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    let before = read_durable(
        Some(session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    assert_eq!(before.len(), 1);

    prune_sessions(&[session_id], Some(Arc::clone(&iris)), &client, &config_dir).await;

    let after = read_durable(
        Some(session_id),
        Some(Arc::clone(&iris)),
        &client,
        &config_dir,
    )
    .await;
    assert_eq!(after.len(), 0, "pruned session's records must be gone");
}

/// FR-011: prune_sessions on the local-file sink removes the session's JSONL file.
#[tokio::test]
#[ignore]
async fn live_prune_sessions_removes_local_file_data() {
    let client = IrisConnection::http_client().unwrap();
    let config_dir = std::env::temp_dir().join("iad_test_telemetry_live_prune_local");
    let session_id = Uuid::new_v4();

    write_durable(
        &ToolCallRecord::now("iris_execute", true, 1, session_id),
        None,
        &client,
        &config_dir,
    )
    .await;
    let before = read_durable(Some(session_id), None, &client, &config_dir).await;
    assert_eq!(before.len(), 1);

    prune_sessions(&[session_id], None, &client, &config_dir).await;

    let after = read_durable(Some(session_id), None, &client, &config_dir).await;
    assert_eq!(after.len(), 0, "pruned session's local file must be gone");

    // Pruning a session with no existing file must not error (best-effort).
    prune_sessions(&[Uuid::new_v4()], None, &client, &config_dir).await;
}

/// End-to-end via the MCP dispatch layer: a real tool call's durable write actually
/// completes against live IRIS, and both telemetry_query and telemetry_export_trace
/// (which only read the durable sink, not the in-memory buffer) see it.
#[cfg(feature = "testing")]
#[tokio::test]
#[ignore]
async fn live_telemetry_query_and_export_trace_reflect_recorded_calls() {
    use iris_agentic_dev_core::iris::connection::DiscoverySource;
    use iris_agentic_dev_core::tools::IrisTools;

    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let iris = IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::EnvVar,
    );
    let tools = IrisTools::new(Some(iris)).expect("IrisTools::new should succeed");

    for _ in 0..3 {
        let _ = tools
            .call_for_test(
                "iris_search",
                serde_json::json!({"query": "x", "namespace": "USER"}),
            )
            .await;
    }
    // record_call's durable write is fire-and-forget (tokio::spawn) — give it a moment.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let query_result = tools
        .call_for_test(
            "telemetry_query",
            serde_json::json!({"tool_name": "iris_search"}),
        )
        .await
        .expect("telemetry_query should succeed");
    let text = query_result
        .content
        .first()
        .unwrap()
        .raw
        .as_text()
        .unwrap()
        .text
        .clone();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let records = json["records"].as_array().unwrap();
    assert!(records.iter().any(|r| r["tool"] == "iris_search"));

    let trace_result = tools
        .call_for_test("telemetry_export_trace", serde_json::json!({}))
        .await
        .expect("telemetry_export_trace should succeed");
    let text = trace_result
        .content
        .first()
        .unwrap()
        .raw
        .as_text()
        .unwrap()
        .text
        .clone();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let traces = json["traces"].as_array().unwrap();
    let iris_search_trace = traces.iter().find(|t| t["to"] == "iris_search");
    assert!(iris_search_trace.is_some());
    assert!(iris_search_trace.unwrap()["count"].as_u64().unwrap() >= 3);
}
