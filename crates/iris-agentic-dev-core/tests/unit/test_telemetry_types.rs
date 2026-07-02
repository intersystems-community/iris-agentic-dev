//! Unit tests for telemetry core types (T007). No live IRIS required.

use iris_agentic_dev_core::telemetry::{Session, ToolCallRecord};
use uuid::Uuid;

#[test]
fn tool_call_record_round_trips_via_serde_json() {
    let sid = Uuid::new_v4();
    let record = ToolCallRecord::now("iris_compile", true, 42, sid);
    let json = serde_json::to_string(&record).unwrap();
    let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool, "iris_compile");
    assert!(back.success);
    assert_eq!(back.duration_ms, 42);
    assert_eq!(back.session_id, sid);
}

#[test]
fn session_new_produces_non_nil_uuid() {
    let s = Session::new();
    assert_ne!(s.id, Uuid::nil());
}

#[test]
fn two_sessions_produce_distinct_ids() {
    let a = Session::new();
    let b = Session::new();
    assert_ne!(a.id, b.id);
}
