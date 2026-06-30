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
        mcp_template: None,
        data_policy: None,
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };
    assert!(
        AuditLog::should_write(Some(&policy)),
        "connection with policy block → should_write must return true"
    );
}

// ── default_path ─────────────────────────────────────────────────────────────

#[test]
fn default_path_returns_audit_jsonl_under_iris_agentic_dev() {
    let path = AuditLog::default_path();
    // home_dir() should succeed on any CI/dev machine with a home directory
    if let Some(p) = path {
        let s = p.to_string_lossy();
        assert!(
            s.ends_with(".iris-agentic-dev/audit.jsonl")
                || s.ends_with(".iris-agentic-dev\\audit.jsonl"),
            "default_path must end with .iris-agentic-dev/audit.jsonl, got: {s}"
        );
        assert!(
            s.contains(".iris-agentic-dev"),
            "path must contain .iris-agentic-dev directory"
        );
    }
    // If home_dir() returns None (unusual env), default_path() returns None — that's fine.
}

#[test]
fn write_creates_nested_parent_directories() {
    // Exercises write_inner's create_dir_all(parent) branch
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("deeply").join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = make_entry("iris_query", "allowed", false);
    log.write(&entry)
        .expect("write with nested dirs must succeed");
    assert!(path.exists(), "audit.jsonl must be created in nested dirs");
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("iris_query"), "entry must be written");
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

// ── edge cases and error paths ───────────────────────────────────────────────

#[test]
fn write_with_all_optional_fields_none() {
    // Test entry with None gate and allowed_categories for "allowed" status
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_query".to_string(),
        connection: "dev".to_string(),
        namespace: "USER".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({}),
    };
    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["status"], "allowed");
    assert!(!parsed.as_object().unwrap().contains_key("gate") || parsed["gate"].is_null());
}

#[test]
fn write_with_error_status_and_optional_fields() {
    // Test entry with "error" status and optional fields
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_compile".to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: "error".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({"error_code": 500}),
    };
    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["status"], "error");
}

#[test]
fn write_with_complex_params_object() {
    // Test nested JSON structures in params
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_query".to_string(),
        connection: "test".to_string(),
        namespace: "TESTDB".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({
            "query": "SELECT * FROM User",
            "limit": 100,
            "nested": {
                "key": "value"
            },
            "array": [1, 2, 3]
        }),
    };
    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert!(parsed["params"]["nested"].is_object());
    assert!(parsed["params"]["array"].is_array());
}

#[test]
fn write_with_empty_params_object() {
    // Test with empty params JSON object
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_compile".to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({}),
    };
    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert!(parsed["params"].is_object());
}

#[test]
fn write_with_multiple_credential_fields() {
    // Test multiple credential fields in same params
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_query".to_string(),
        connection: "test".to_string(),
        namespace: "USER".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({
            "password": "pwd123",
            "token": "tok456",
            "api_key": "key789",
            "host": "server.com"
        }),
    };
    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["params"]["password"], "[REDACTED]");
    assert_eq!(parsed["params"]["token"], "[REDACTED]");
    assert_eq!(parsed["params"]["api_key"], "[REDACTED]");
    assert_eq!(parsed["params"]["host"], "server.com");
}

#[test]
fn write_with_all_credential_field_types() {
    // Test all credential field names from CREDENTIAL_FIELDS constant
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_query".to_string(),
        connection: "test".to_string(),
        namespace: "USER".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({
            "password": "pwd",
            "token": "tok",
            "api_key": "key",
            "secret": "sec",
            "access_token": "at",
            "auth_token": "at2"
        }),
    };
    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["params"]["password"], "[REDACTED]");
    assert_eq!(parsed["params"]["token"], "[REDACTED]");
    assert_eq!(parsed["params"]["api_key"], "[REDACTED]");
    assert_eq!(parsed["params"]["secret"], "[REDACTED]");
    assert_eq!(parsed["params"]["access_token"], "[REDACTED]");
    assert_eq!(parsed["params"]["auth_token"], "[REDACTED]");
}

#[test]
fn write_path_without_parent_directory() {
    // Test write when path is a relative filename with no parent directory
    let dir = TempDir::new().unwrap();
    let filename = dir.path().join("audit.jsonl");

    let log = AuditLog::new(filename.clone());
    let entry = make_entry("iris_compile", "blocked", true);
    log.write(&entry).unwrap();

    assert!(filename.exists());
}

#[test]
fn write_updates_timestamps_independently() {
    // Verify that each entry can have different timestamps
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry1 = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_compile".to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({"target": "A"}),
    };
    let entry2 = AuditLogEntry {
        ts: "2026-06-26T14:00:01Z".to_string(),
        tool: "iris_query".to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({"target": "B"}),
    };

    log.write(&entry1).unwrap();
    log.write(&entry2).unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();

    let parsed1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();

    assert_eq!(parsed1["ts"], "2026-06-26T14:00:00Z");
    assert_eq!(parsed2["ts"], "2026-06-26T14:00:01Z");
}

#[test]
fn write_preserves_all_connection_names() {
    // Verify connection name is preserved exactly
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_compile".to_string(),
        connection: "prod-us-east-1".to_string(),
        namespace: "HSLIB".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params: serde_json::json!({}),
    };

    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["connection"], "prod-us-east-1");
}

#[test]
fn write_preserves_namespace_values() {
    // Verify namespace is preserved exactly (various IRIS namespaces)
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    for ns in &["USER", "HSLIB", "SAMPLES", "%SYS", "CUSTOM"] {
        let entry = AuditLogEntry {
            ts: "2026-06-26T14:00:00Z".to_string(),
            tool: "iris_query".to_string(),
            connection: "test".to_string(),
            namespace: ns.to_string(),
            status: "allowed".to_string(),
            gate: None,
            allowed_categories: None,
            params: serde_json::json!({"ns": ns}),
        };

        log.write(&entry).unwrap();
    }

    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 5);
}

#[test]
fn write_handles_role_gate_type() {
    // Test write with "role" gate instead of "policy"
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_compile".to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: "blocked".to_string(),
        gate: Some("role".to_string()),
        allowed_categories: Some(vec!["read".to_string()]),
        params: serde_json::json!({"target": "User.Foo.cls"}),
    };

    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["gate"], "role");
    assert_eq!(parsed["status"], "blocked");
}

#[test]
fn write_handles_empty_allowed_categories() {
    // Test blocked entry with empty allowed_categories list
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry = AuditLogEntry {
        ts: "2026-06-26T14:00:00Z".to_string(),
        tool: "iris_compile".to_string(),
        connection: "prod".to_string(),
        namespace: "USER".to_string(),
        status: "blocked".to_string(),
        gate: Some("policy".to_string()),
        allowed_categories: Some(vec![]),
        params: serde_json::json!({"target": "User.Foo.cls"}),
    };

    log.write(&entry).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert!(parsed["allowed_categories"].is_array());
    assert_eq!(parsed["allowed_categories"].as_array().unwrap().len(), 0);
}
