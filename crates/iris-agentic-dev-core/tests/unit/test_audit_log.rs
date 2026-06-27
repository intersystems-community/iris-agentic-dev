// Tests for append-only JSONL audit log (US3, 044-servermanager-discovery).

use iris_agentic_dev_core::iris::audit_log::{AuditLog, AuditLogEntry};
use std::fs;
use tempfile::TempDir;

fn make_entry(tool: &str, status: &str, blocked: bool) -> AuditLogEntry {
    AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: tool.to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: status.to_string(),
        gate: if blocked {
            Some("policy".to_string())
        } else {
            None
        },
        allowed_categories: if blocked {
            Some(vec!["query".to_string(), "docs".to_string()])
        } else {
            None
        },
        params: serde_json::json!({"target": "User.Foo.cls"}),
    }
}

// ── append behaviour ─────────────────────────────────────────────────────────

#[test]
fn append_creates_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = make_entry("iris_compile", "blocked", true);
    log.write(&entry).expect("write should succeed");
    assert!(path.exists(), "audit.jsonl must be created");
}

#[test]
fn second_append_does_not_overwrite() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    log.write(&make_entry("iris_compile", "blocked", true))
        .unwrap();
    log.write(&make_entry("iris_query", "allowed", false))
        .unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 2, "two writes must produce 2 lines");
    // First entry must still be present
    assert!(lines[0].contains("iris_compile"));
    assert!(lines[1].contains("iris_query"));
}

// ── entry field correctness ──────────────────────────────────────────────────

#[test]
fn blocked_entry_includes_gate_and_allowed_categories() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = make_entry("iris_compile", "blocked", true);
    log.write(&entry).unwrap();
    let line = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(parsed["status"], "blocked");
    assert_eq!(parsed["gate"], "policy");
    assert!(
        parsed["allowed_categories"].is_array(),
        "allowed_categories must be present when blocked"
    );
}

#[test]
fn allowed_entry_omits_gate_and_allowed_categories() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = make_entry("iris_query", "allowed", false);
    log.write(&entry).unwrap();
    let line = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(parsed["status"], "allowed");
    assert!(
        parsed["gate"].is_null() || !parsed.as_object().unwrap().contains_key("gate"),
        "gate must be absent for allowed entries"
    );
    assert!(
        parsed["allowed_categories"].is_null()
            || !parsed
                .as_object()
                .unwrap()
                .contains_key("allowed_categories"),
        "allowed_categories must be absent for allowed entries"
    );
}

#[test]
fn entry_contains_required_fields() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    log.write(&make_entry("iris_compile", "blocked", true))
        .unwrap();
    let line = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    for field in &["ts", "tool", "connection", "namespace", "status", "params"] {
        assert!(
            parsed.get(*field).is_some(),
            "required field '{field}' must be present in audit entry"
        );
    }
}

#[test]
fn params_are_full_json_object() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    log.write(&make_entry("iris_compile", "allowed", false))
        .unwrap();
    let line = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert!(parsed["params"].is_object(), "params must be a JSON object");
    assert_eq!(parsed["params"]["target"], "User.Foo.cls");
}

// ── non-blocking write failure ───────────────────────────────────────────────

#[test]
fn write_to_nonexistent_dir_returns_ok() {
    // Non-blocking: write failure must not propagate as error
    let log = AuditLog::new(std::path::PathBuf::from(
        "/nonexistent/deeply/nested/path/audit.jsonl",
    ));
    let entry = make_entry("iris_compile", "blocked", true);
    // Must return Ok(()) even though dir doesn't exist
    let result = log.write(&entry);
    assert!(
        result.is_ok(),
        "write failure must be non-blocking (return Ok): {result:?}"
    );
}

// ── no-policy connection produces no entry ────────────────────────────────────

#[test]
fn no_policy_connection_does_not_write() {
    // This is enforced at the handler level (only write when connection has policy block).
    // Here we verify that AuditLog::should_write returns false when no policy active.
    // The wiring in handlers calls write() only when connection has active policy.
    // Test the predicate directly.
    assert!(
        !AuditLog::should_write(None),
        "no policy block → should_write must return false"
    );
}

#[test]
fn policy_connection_should_write_returns_true() {
    use iris_agentic_dev_core::iris::workspace_config::ConnectionPolicy;
    let policy = ConnectionPolicy {
        server_name: "prod".to_string(),
        allow: Some(vec![]),
    };
    assert!(
        AuditLog::should_write(Some(&policy)),
        "connection with policy block → should_write must return true"
    );
}

// ── SC-002: audit write latency < 100ms ──────────────────────────────────────

#[test]
fn audit_write_latency_under_100ms() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path);
    let entry = make_entry("iris_compile", "blocked", true);
    let start = std::time::Instant::now();
    log.write(&entry).unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "AuditLog::write must complete in < 100ms, took {}ms",
        elapsed.as_millis()
    );
}
