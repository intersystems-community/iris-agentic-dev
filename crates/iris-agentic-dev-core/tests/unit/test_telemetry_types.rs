//! Unit tests for telemetry core types (T007). No live IRIS required.

use iris_agentic_dev_core::telemetry::{
    ago_secs, eval_session_from_env, now_rfc3339, Session, ToolCallRecord,
};
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

#[test]
fn now_rfc3339_produces_non_empty_string() {
    let ts = now_rfc3339();
    assert!(!ts.is_empty());
    // Should be parseable as RFC3339
    assert!(chrono::DateTime::parse_from_rfc3339(&ts).is_ok());
}

#[test]
fn ago_secs_returns_zero_for_invalid_timestamp() {
    assert_eq!(ago_secs("not-a-timestamp"), 0);
    assert_eq!(ago_secs(""), 0);
}

#[test]
fn ago_secs_returns_nonzero_for_past_timestamp() {
    // A timestamp far in the past should produce a large positive value
    let old_ts = "2020-01-01T00:00:00Z";
    let secs = ago_secs(old_ts);
    assert!(secs > 0, "past timestamp should have positive ago_secs");
}

#[test]
fn tool_call_record_with_params_serializes() {
    let sid = Uuid::new_v4();
    let mut record = ToolCallRecord::now("iris_query", true, 55, sid);
    record.params = Some(serde_json::json!({"query": "SELECT 1"}));
    let json = serde_json::to_string(&record).unwrap();
    let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.params.unwrap()["query"], "SELECT 1");
}

#[test]
fn test_eval_session_absent_when_env_not_set() {
    // Remove env vars if set by a parent process
    std::env::remove_var("GAUNTLET_RUN_ID");
    std::env::remove_var("GAUNTLET_TASK_ID");
    std::env::remove_var("GAUNTLET_CONDITION");
    let (run_id, task_id, condition) = eval_session_from_env();
    assert!(
        run_id.is_none(),
        "run_id should be None when env var absent"
    );
    assert!(
        task_id.is_none(),
        "task_id should be None when env var absent"
    );
    assert!(
        condition.is_none(),
        "condition should be None when env var absent"
    );
}

#[test]
fn test_eval_session_present_when_env_set() {
    // Serialize env mutations — env is process-global state
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_MUTEX.lock().unwrap();

    std::env::set_var("GAUNTLET_RUN_ID", "run-abc123");
    std::env::set_var("GAUNTLET_TASK_ID", "task-42");
    std::env::set_var("GAUNTLET_CONDITION", "harness");
    let (run_id, task_id, condition) = eval_session_from_env();
    std::env::remove_var("GAUNTLET_RUN_ID");
    std::env::remove_var("GAUNTLET_TASK_ID");
    std::env::remove_var("GAUNTLET_CONDITION");

    assert_eq!(run_id.as_deref(), Some("run-abc123"));
    assert_eq!(task_id.as_deref(), Some("task-42"));
    assert_eq!(condition.as_deref(), Some("harness"));
}

#[test]
fn test_eval_session_fields_round_trip_serde() {
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_MUTEX.lock().unwrap();

    std::env::set_var("GAUNTLET_RUN_ID", "run-serde-test");
    std::env::set_var("GAUNTLET_TASK_ID", "task-serde");
    std::env::set_var("GAUNTLET_CONDITION", "test-condition");
    let record = ToolCallRecord::now("iris_execute", true, 10, Uuid::new_v4());
    std::env::remove_var("GAUNTLET_RUN_ID");
    std::env::remove_var("GAUNTLET_TASK_ID");
    std::env::remove_var("GAUNTLET_CONDITION");

    let json = serde_json::to_string(&record).unwrap();
    let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.eval_run_id.as_deref(), Some("run-serde-test"));
    assert_eq!(back.eval_task_id.as_deref(), Some("task-serde"));
    assert_eq!(back.eval_condition.as_deref(), Some("test-condition"));

    // Verify absent fields do not appear in serialized output when None
    let sid = Uuid::new_v4();
    let no_env = ToolCallRecord::now("iris_info", true, 1, sid);
    let no_env_json = serde_json::to_string(&no_env).unwrap();
    assert!(
        !no_env_json.contains("eval_run_id"),
        "eval_run_id should be absent from JSON when None"
    );
}
