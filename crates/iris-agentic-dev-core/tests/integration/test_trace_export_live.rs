//! Integration test for dispatch-trace export (059-tool-telemetry-benchmark, US3).
//! Requires live IRIS on iris-dev-iris (port from IRIS_HOST/IRIS_WEB_PORT env). Run with:
//!   cargo test -p iris-agentic-dev-core --test test_trace_export_live -- --ignored

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::telemetry::trace_export::aggregate_trace;
use iris_agentic_dev_core::telemetry::{read_durable, write_durable, ToolCallRecord};
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

/// US3 Acceptance Scenarios 1–3: repeated + varied tool calls in a live session, exported
/// via aggregate_trace over durable-sink data, produce the exact
/// `{from, to, via, count, ts}` shape with correct aggregation.
#[tokio::test]
#[ignore]
async fn live_trace_export_aggregates_repeated_calls_and_matches_058_contract_shape() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let config_dir = std::env::temp_dir().join("iad_test_trace_export_live");
    let session_id = Uuid::new_v4();

    for _ in 0..3 {
        write_durable(
            &ToolCallRecord::now("iris_compile", true, 1, session_id),
            Some(Arc::clone(&iris)),
            &client,
            &config_dir,
        )
        .await;
    }
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
    assert_eq!(records.len(), 4);

    let traces = aggregate_trace(&records);
    assert_eq!(
        traces.len(),
        2,
        "one record per distinct (from,to,via) combination"
    );

    let compile_trace = traces.iter().find(|t| t.to == "iris_compile").unwrap();
    assert_eq!(
        compile_trace.count, 3,
        "3 repeated calls aggregate into one record with count=3"
    );

    let execute_trace = traces.iter().find(|t| t.to == "iris_execute").unwrap();
    assert_eq!(execute_trace.count, 1);

    for t in &traces {
        let json = serde_json::to_value(t).unwrap();
        assert!(json["from"].is_string());
        assert!(json["to"].is_string());
        assert!(json["via"].is_string());
        assert!(json["count"].is_u64() || json["count"].is_number());
        assert!(json["ts"].is_string());
    }

    let cleanup = format!("kill ^IRISDEV(\"telemetry\",\"{session_id}\")\n");
    let _ = iris.execute_via_generator(&cleanup, "USER", &client).await;
}
