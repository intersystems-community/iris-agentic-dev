//! Unit tests for filter_records (T025). No live IRIS required.

use iris_agentic_dev_core::telemetry::{filter_records, ToolCallRecord};
use uuid::Uuid;

fn record(tool: &str, sid: Uuid, ts: &str) -> ToolCallRecord {
    let mut r = ToolCallRecord::now(tool, true, 1, sid);
    r.timestamp = ts.to_string();
    r
}

#[test]
fn filters_by_tool_name() {
    let sid = Uuid::new_v4();
    let records = vec![
        record("iris_compile", sid, "2026-01-01T00:00:00Z"),
        record("iris_execute", sid, "2026-01-01T00:00:01Z"),
    ];
    let (out, truncated) = filter_records(&records, Some("iris_compile"), None, None, None, 10);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tool, "iris_compile");
    assert!(!truncated);
}

#[test]
fn filters_by_session_id() {
    let sid_a = Uuid::new_v4();
    let sid_b = Uuid::new_v4();
    let records = vec![
        record("iris_compile", sid_a, "2026-01-01T00:00:00Z"),
        record("iris_compile", sid_b, "2026-01-01T00:00:01Z"),
    ];
    let (out, _) = filter_records(&records, None, Some(sid_a), None, None, 10);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].session_id, sid_a);
}

#[test]
fn filters_by_time_range_combination() {
    let sid = Uuid::new_v4();
    let records = vec![
        record("a", sid, "2026-01-01T00:00:00Z"),
        record("b", sid, "2026-06-01T00:00:00Z"),
        record("c", sid, "2026-12-01T00:00:00Z"),
    ];
    let (out, _) = filter_records(
        &records,
        None,
        None,
        Some("2026-03-01T00:00:00Z"),
        Some("2026-09-01T00:00:00Z"),
        10,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tool, "b");
}

#[test]
fn truncated_flag_set_only_when_matches_exceed_limit() {
    let sid = Uuid::new_v4();
    let records: Vec<_> = (0..5)
        .map(|i| record(&format!("t{i}"), sid, "2026-01-01T00:00:00Z"))
        .collect();
    let (out, truncated) = filter_records(&records, None, None, None, None, 3);
    assert_eq!(out.len(), 3);
    assert!(truncated);
    let (out2, truncated2) = filter_records(&records, None, None, None, None, 5);
    assert_eq!(out2.len(), 5);
    assert!(!truncated2);
}
