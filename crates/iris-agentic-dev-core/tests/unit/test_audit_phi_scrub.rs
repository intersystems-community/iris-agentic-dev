// Tests for audit log PHI scrubbing (051-phi-policy-env-gates, US4, FR-010).
//
// Verifies:
// - PHI-named global name in params is replaced with [REDACTED-PHI]
// - Non-PHI params are not modified
// - Credential/password fields are scrubbed (existing behaviour, not regressed)
// - Scrubbing applies to allowed calls (PHI may appear in params even when permitted)
// - Scrubbing applies to blocked calls too

use iris_agentic_dev_core::iris::audit_log::{scrub_params, AuditLog, AuditLogEntry};
use tempfile::TempDir;

fn make_entry_with_params(params: serde_json::Value) -> AuditLogEntry {
    AuditLogEntry {
        ts: "2026-06-29T11:00:00Z".to_string(),
        tool: "iris_global".to_string(),
        connection: "iris-health".to_string(),
        namespace: "HSLIB".to_string(),
        status: "allowed".to_string(),
        gate: None,
        allowed_categories: None,
        params,
    }
}

// ── scrub_params function tests ───────────────────────────────────────────────

#[test]
fn scrub_phi_global_name_field() {
    let params = serde_json::json!({"global_name": "PAPMI", "keys": []});
    let scrubbed = scrub_params(params);
    assert_eq!(
        scrubbed["global_name"], "[REDACTED-PHI]",
        "PHI global name must be redacted"
    );
}

#[test]
fn scrub_phi_global_name_with_hat_prefix() {
    // Some callers may pass ^PAPMI with the ^ prefix
    let params = serde_json::json!({"global_name": "^PAPMI"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["global_name"], "[REDACTED-PHI]");
}

#[test]
fn scrub_phi_global_name_case_insensitive() {
    let params = serde_json::json!({"global_name": "papmi1234"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["global_name"], "[REDACTED-PHI]");
}

#[test]
fn no_scrub_non_phi_global_name() {
    let params = serde_json::json!({"global_name": "MyAppData", "keys": ["k1"]});
    let scrubbed = scrub_params(params);
    assert_eq!(
        scrubbed["global_name"], "MyAppData",
        "non-PHI global name must not be modified"
    );
}

#[test]
fn scrub_password_field() {
    let params = serde_json::json!({"host": "prod.example.com", "password": "secret123"});
    let scrubbed = scrub_params(params);
    assert_eq!(
        scrubbed["password"], "[REDACTED]",
        "password field must be scrubbed"
    );
    assert_eq!(
        scrubbed["host"], "prod.example.com",
        "non-sensitive field must be preserved"
    );
}

#[test]
fn scrub_token_field() {
    let params = serde_json::json!({"token": "eyJhbGciOiJ...", "namespace": "USER"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["token"], "[REDACTED]");
    assert_eq!(scrubbed["namespace"], "USER");
}

#[test]
fn scrub_api_key_field() {
    let params = serde_json::json!({"api_key": "sk-abc123", "model": "gpt-4"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["api_key"], "[REDACTED]");
}

#[test]
fn no_scrub_unrelated_params() {
    let params = serde_json::json!({"target": "User.Foo.cls", "flags": 0, "namespace": "USER"});
    let scrubbed = scrub_params(params.clone());
    assert_eq!(scrubbed["target"], "User.Foo.cls");
    assert_eq!(scrubbed["flags"], 0);
    assert_eq!(scrubbed["namespace"], "USER");
}

// ── Integration: write() calls scrub before serializing ───────────────────────

#[test]
fn written_entry_has_phi_global_name_redacted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry = make_entry_with_params(serde_json::json!({"global_name": "PAPMI", "keys": []}));
    log.write(&entry).unwrap();

    let line = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(
        parsed["params"]["global_name"], "[REDACTED-PHI]",
        "written entry must have PHI global name redacted"
    );
}

#[test]
fn written_entry_preserves_non_phi_params() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry = make_entry_with_params(serde_json::json!({"target": "User.Foo.cls"}));
    log.write(&entry).unwrap();

    let line = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(parsed["params"]["target"], "User.Foo.cls");
}

#[test]
fn written_entry_has_password_redacted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());

    let entry =
        make_entry_with_params(serde_json::json!({"password": "secret", "host": "prod.com"}));
    log.write(&entry).unwrap();

    let line = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(parsed["params"]["password"], "[REDACTED]");
    assert_eq!(parsed["params"]["host"], "prod.com");
}

// ── edge cases for scrub_params ──────────────────────────────────────────────

#[test]
fn scrub_params_with_non_string_global_name() {
    // When global_name is not a string (e.g., number, null, object), it should be preserved
    let params = serde_json::json!({"global_name": 123});
    let scrubbed = scrub_params(params);
    assert_eq!(
        scrubbed["global_name"], 123,
        "non-string global_name must be preserved"
    );
}

#[test]
fn scrub_params_with_null_global_name() {
    // When global_name is null, it should be preserved
    let params = serde_json::json!({"global_name": serde_json::json!(null)});
    let scrubbed = scrub_params(params);
    assert!(
        scrubbed["global_name"].is_null(),
        "null global_name must be preserved"
    );
}

#[test]
fn scrub_params_with_array_global_name() {
    // When global_name is an array, it should be preserved
    let params = serde_json::json!({"global_name": ["A", "B"]});
    let scrubbed = scrub_params(params);
    assert!(
        scrubbed["global_name"].is_array(),
        "array global_name must be preserved"
    );
}

#[test]
fn scrub_params_with_object_global_name() {
    // When global_name is an object, it should be preserved
    let params = serde_json::json!({"global_name": {"nested": "value"}});
    let scrubbed = scrub_params(params);
    assert!(
        scrubbed["global_name"].is_object(),
        "object global_name must be preserved"
    );
}

#[test]
fn scrub_params_with_non_object_input() {
    // When input is not a JSON object (e.g., string, number, array), it should be returned unchanged
    let params = serde_json::json!("not an object");
    let scrubbed = scrub_params(params.clone());
    assert_eq!(
        scrubbed, params,
        "non-object input must be returned unchanged"
    );
}

#[test]
fn scrub_params_with_array_input() {
    // When input is an array, it should be returned unchanged
    let params = serde_json::json!(["a", "b", "c"]);
    let scrubbed = scrub_params(params.clone());
    assert_eq!(scrubbed, params, "array input must be returned unchanged");
}

#[test]
fn scrub_params_with_null_input() {
    // When input is null, it should be returned unchanged
    let params = serde_json::json!(null);
    let scrubbed = scrub_params(params.clone());
    assert_eq!(scrubbed, params, "null input must be returned unchanged");
}

#[test]
fn scrub_params_with_number_input() {
    // When input is a number, it should be returned unchanged
    let params = serde_json::json!(42);
    let scrubbed = scrub_params(params.clone());
    assert_eq!(scrubbed, params, "number input must be returned unchanged");
}

#[test]
fn scrub_params_credential_field_secret() {
    let params = serde_json::json!({"secret": "my-secret-value", "app": "myapp"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["secret"], "[REDACTED]");
    assert_eq!(scrubbed["app"], "myapp");
}

#[test]
fn scrub_params_credential_field_access_token() {
    let params = serde_json::json!({"access_token": "eyJhbGc...", "user": "john"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["access_token"], "[REDACTED]");
    assert_eq!(scrubbed["user"], "john");
}

#[test]
fn scrub_params_credential_field_auth_token() {
    let params = serde_json::json!({"auth_token": "abc123", "session": "xyz"});
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["auth_token"], "[REDACTED]");
    assert_eq!(scrubbed["session"], "xyz");
}

#[test]
fn scrub_params_preserves_empty_object() {
    let params = serde_json::json!({});
    let scrubbed = scrub_params(params.clone());
    assert_eq!(scrubbed, params, "empty object must be preserved");
}

#[test]
fn scrub_params_preserves_large_object() {
    let params = serde_json::json!({
        "field1": "value1",
        "field2": 42,
        "field3": true,
        "field4": ["a", "b"],
        "field5": {"nested": "obj"}
    });
    let scrubbed = scrub_params(params.clone());
    assert_eq!(scrubbed["field1"], "value1");
    assert_eq!(scrubbed["field2"], 42);
    assert_eq!(scrubbed["field3"], true);
    assert!(scrubbed["field4"].is_array());
    assert!(scrubbed["field5"].is_object());
}

#[test]
fn scrub_params_multiple_phi_and_credential_fields() {
    // Test combination of PHI global_name + credential fields
    let params = serde_json::json!({
        "global_name": "PAPMI",
        "password": "pwd",
        "api_key": "key",
        "namespace": "HSLIB",
        "host": "server.com"
    });
    let scrubbed = scrub_params(params);
    assert_eq!(scrubbed["global_name"], "[REDACTED-PHI]");
    assert_eq!(scrubbed["password"], "[REDACTED]");
    assert_eq!(scrubbed["api_key"], "[REDACTED]");
    assert_eq!(scrubbed["namespace"], "HSLIB");
    assert_eq!(scrubbed["host"], "server.com");
}

#[test]
fn scrub_params_phi_pattern_matching_case_variations() {
    // Test various case combinations for PHI patterns
    // Note: patterns use wildcards, e.g., "^PAPER*" matches "PAPERWORK"
    let test_cases = vec![
        ("PAPMI", true),     // Matches PAPMI* pattern
        ("papmi", true),     // Lowercase, matches case-insensitively
        ("PaPmI", true),     // Mixed case, matches case-insensitively
        ("papmi1234", true), // With numbers, matches PAPMI* pattern
        ("PAPERWORK", true), // Matches PAPER* pattern
        ("ORDER123", true),  // Matches ORDER* pattern
        ("MYDATA", false),   // Regular global, no PHI pattern match
        ("NOTAPHI", false),  // Does not match any PHI pattern
    ];

    for (name, should_be_redacted) in test_cases {
        let params = serde_json::json!({"global_name": name});
        let scrubbed = scrub_params(params);
        if should_be_redacted {
            assert_eq!(
                scrubbed["global_name"], "[REDACTED-PHI]",
                "name '{}' should be redacted",
                name
            );
        } else {
            assert_eq!(
                scrubbed["global_name"], name,
                "name '{}' should not be redacted",
                name
            );
        }
    }
}
