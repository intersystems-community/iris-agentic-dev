//! Unit tests for dispatch-trace aggregation (T032). No live IRIS required.

use iris_agentic_dev_core::telemetry::trace_export::{aggregate_trace, NO_CALLER_SENTINEL, VIA};
use iris_agentic_dev_core::telemetry::ToolCallRecord;
use uuid::Uuid;

fn record(tool: &str, ts: &str) -> ToolCallRecord {
    let mut r = ToolCallRecord::now(tool, true, 1, Uuid::new_v4());
    r.timestamp = ts.to_string();
    r
}

#[test]
fn repeated_identical_calls_aggregate_with_incremented_count() {
    let records = vec![
        record("iris_compile", "2026-07-01T10:00:00Z"),
        record("iris_compile", "2026-07-01T10:00:05Z"),
    ];
    let out = aggregate_trace(&records);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].count, 2);
    assert_eq!(out[0].ts, "2026-07-01T10:00:05Z");
}

#[test]
fn varied_calls_produce_one_record_per_combination() {
    let records = vec![
        record("iris_compile", "2026-07-01T10:00:00Z"),
        record("iris_execute", "2026-07-01T10:00:01Z"),
    ];
    let out = aggregate_trace(&records);
    assert_eq!(out.len(), 2);
}

#[test]
fn output_has_exactly_from_to_via_count_ts_fields() {
    let records = vec![record("iris_compile", "2026-07-01T10:00:00Z")];
    let out = aggregate_trace(&records);
    let json = serde_json::to_value(&out[0]).unwrap();
    let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["count", "from", "to", "ts", "via"]);
    assert_eq!(out[0].from, NO_CALLER_SENTINEL);
    assert_eq!(out[0].via, VIA);
}
