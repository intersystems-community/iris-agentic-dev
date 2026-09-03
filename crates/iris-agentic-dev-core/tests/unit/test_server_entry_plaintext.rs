//! Unit tests for spec 095 — ServerEntry plaintext credential persistence fallback.
//!
//! Layer 1 (unit / serde round-trip): parse JSON strings, assert field presence.
//! Covers the serde silent-drop pattern (field missing from struct → JSON key
//! silently ignored on deserialize).

use iris_agentic_dev_core::iris::servers_config::ServerEntry;

// ── Serde round-trip ──────────────────────────────────────────────────────────

/// Credential field present in JSON → deserializes to Some("SYS").
#[test]
fn server_entry_credential_field_deserializes_some() {
    let json = r#"{
        "host": "localhost",
        "port": 52780,
        "namespace": "USER",
        "username": "_SYSTEM",
        "password": "SYS"
    }"#;
    let entry: ServerEntry = serde_json::from_str(json).expect("parse should succeed");
    assert_eq!(
        entry.password.as_deref(),
        Some("SYS"),
        "password field must deserialize to Some(\"SYS\")"
    );
}

/// Credential field absent in JSON → deserializes to None (backwards compat).
#[test]
fn server_entry_credential_field_absent_deserializes_none() {
    let json = r#"{
        "host": "localhost",
        "port": 52780,
        "namespace": "USER",
        "username": "_SYSTEM"
    }"#;
    let entry: ServerEntry = serde_json::from_str(json).expect("parse should succeed");
    assert!(
        entry.password.is_none(),
        "missing password field must deserialize to None"
    );
}

/// Serialize with credential Some → JSON contains "password" key.
#[test]
fn server_entry_credential_some_serializes_to_json() {
    let entry = ServerEntry {
        host: "localhost".to_string(),
        port: 52780,
        namespace: "USER".to_string(),
        username: "_SYSTEM".to_string(),
        description: None,
        scheme: None,
        password: Some("secret".to_string()),
    };
    let json = serde_json::to_string(&entry).expect("serialize should succeed");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        v["password"].as_str(),
        Some("secret"),
        "password must appear in serialized JSON when Some"
    );
}

/// Serialize with credential None → JSON does NOT contain "password" key
/// (skip_serializing_if = "Option::is_none").
#[test]
fn server_entry_credential_none_omitted_from_serialization() {
    let entry = ServerEntry {
        host: "localhost".to_string(),
        port: 52780,
        namespace: "USER".to_string(),
        username: "_SYSTEM".to_string(),
        description: None,
        scheme: None,
        password: None,
    };
    let json = serde_json::to_string(&entry).expect("serialize should succeed");
    assert!(
        !json.contains("\"password\""),
        "password key must be absent from serialized JSON when None, got: {json}"
    );
}

/// Full round-trip via save_to_path + load_from_path preserves the credential field.
#[test]
fn server_entry_credential_survives_file_roundtrip() {
    use iris_agentic_dev_core::iris::servers_config::{
        load_from_path, save_to_path, ServersConfig,
    };
    use std::collections::HashMap;

    let mut servers = HashMap::new();
    servers.insert(
        "dev".to_string(),
        ServerEntry {
            host: "localhost".to_string(),
            port: 52780,
            namespace: "USER".to_string(),
            username: "_SYSTEM".to_string(),
            description: None,
            scheme: None,
            password: Some("SYS".to_string()),
        },
    );
    let cfg = ServersConfig {
        version: 1,
        servers,
        default: None,
    };

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("servers.json");
    save_to_path(&cfg, &path).expect("save should succeed");

    let loaded = load_from_path(&path);
    let dev = loaded.servers.get("dev").expect("dev entry must exist");
    assert_eq!(
        dev.password.as_deref(),
        Some("SYS"),
        "credential must survive file round-trip"
    );
}

/// A legacy entry without a credential field deserializes without error.
#[test]
fn legacy_entry_without_credential_loads_cleanly() {
    use iris_agentic_dev_core::iris::servers_config::load_from_path;

    // Write raw JSON that deliberately omits the "password" key.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("servers.json");
    let legacy_json = r#"{"version":1,"servers":{"prod":{"host":"prod.example.com","port":443,"namespace":"PROD","username":"admin"}}}"#;
    std::fs::write(&path, legacy_json).unwrap();

    let loaded = load_from_path(&path);
    let prod = loaded.servers.get("prod").expect("prod entry must exist");
    assert!(
        prod.password.is_none(),
        "legacy entry must load with password=None, not cause a parse error"
    );
}
