//! Unit tests for the local-file telemetry path (write_durable/read_durable with iris=None).
//! No live IRIS required — exercises write_jsonl and read_durable_local.

use iris_agentic_dev_core::telemetry::{read_durable, write_durable, ToolCallRecord};
use tempfile::TempDir;
use uuid::Uuid;

fn make_reqwest_client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn write_durable_local_creates_jsonl_file() {
    let dir = TempDir::new().expect("tempdir");
    let sid = Uuid::new_v4();
    let client = make_reqwest_client();
    let record = ToolCallRecord::now("iris_compile", true, 42, sid);

    write_durable(&record, None, &client, dir.path()).await;

    let telemetry_dir = dir.path().join("telemetry");
    assert!(
        telemetry_dir.exists(),
        "telemetry directory should be created"
    );
    let jsonl_path = telemetry_dir.join(format!("{}.jsonl", sid));
    assert!(
        jsonl_path.exists(),
        "jsonl file should be created for session"
    );
}

#[tokio::test]
async fn write_then_read_durable_local_round_trips() {
    let dir = TempDir::new().expect("tempdir");
    let sid = Uuid::new_v4();
    let client = make_reqwest_client();
    let record = ToolCallRecord::now("iris_execute", false, 100, sid);

    write_durable(&record, None, &client, dir.path()).await;

    let records = read_durable(Some(sid), None, &client, dir.path()).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tool, "iris_execute");
    assert!(!records[0].success);
    assert_eq!(records[0].duration_ms, 100);
    assert_eq!(records[0].session_id, sid);
}

#[tokio::test]
async fn read_durable_local_returns_empty_for_unknown_session() {
    let dir = TempDir::new().expect("tempdir");
    let sid = Uuid::new_v4();
    let client = make_reqwest_client();

    let records = read_durable(Some(sid), None, &client, dir.path()).await;
    assert!(records.is_empty(), "no records for an unknown session");
}

#[tokio::test]
async fn read_durable_local_all_sessions_returns_all_records() {
    let dir = TempDir::new().expect("tempdir");
    let client = make_reqwest_client();
    let sid_a = Uuid::new_v4();
    let sid_b = Uuid::new_v4();

    write_durable(
        &ToolCallRecord::now("tool_a", true, 1, sid_a),
        None,
        &client,
        dir.path(),
    )
    .await;
    write_durable(
        &ToolCallRecord::now("tool_b", true, 2, sid_b),
        None,
        &client,
        dir.path(),
    )
    .await;

    let all = read_durable(None, None, &client, dir.path()).await;
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn write_durable_local_multiple_records_same_session() {
    let dir = TempDir::new().expect("tempdir");
    let sid = Uuid::new_v4();
    let client = make_reqwest_client();

    write_durable(
        &ToolCallRecord::now("tool1", true, 10, sid),
        None,
        &client,
        dir.path(),
    )
    .await;
    write_durable(
        &ToolCallRecord::now("tool2", false, 20, sid),
        None,
        &client,
        dir.path(),
    )
    .await;
    write_durable(
        &ToolCallRecord::now("tool3", true, 30, sid),
        None,
        &client,
        dir.path(),
    )
    .await;

    let records = read_durable(Some(sid), None, &client, dir.path()).await;
    assert_eq!(records.len(), 3);
    let tools: Vec<&str> = records.iter().map(|r| r.tool.as_str()).collect();
    assert!(tools.contains(&"tool1"));
    assert!(tools.contains(&"tool2"));
    assert!(tools.contains(&"tool3"));
}

#[tokio::test]
async fn read_durable_local_non_existent_dir_returns_empty() {
    let dir = TempDir::new().expect("tempdir");
    let client = make_reqwest_client();
    // Pass a path that has no telemetry subdirectory
    let records = read_durable(None, None, &client, dir.path()).await;
    assert!(records.is_empty());
}
