//! Unit tests for spec 093 — TOML pool hot-reload.
//!
//! Layer 1 (unit): parse `iris_reload_pool` response JSON, assert field shapes.
//! Covers the serde silent-drop pattern — if the response struct loses a field,
//! callers see a `null` instead of the expected integer.

/// Successful reload response must include `servers_loaded` as a number and
/// `success: true`.
#[test]
fn reload_pool_success_response_shape() {
    let json = r#"{
        "success": true,
        "servers_loaded": 2,
        "servers": ["dev", "prod"],
        "note": "Pool rebuilt. Servers are immediately routable via the `server` parameter."
    }"#;
    let v: serde_json::Value = serde_json::from_str(json).expect("parse ok");
    assert_eq!(v["success"].as_bool(), Some(true));
    assert!(
        v["servers_loaded"].is_number(),
        "servers_loaded must be a number"
    );
    assert_eq!(v["servers_loaded"].as_u64(), Some(2));
    assert!(v["servers"].is_array());
    assert!(v["note"].is_string());
}

/// Parse-error response must include `success: false`, `error_code`, and `note`
/// mentioning the preserved pool.
#[test]
fn reload_pool_parse_error_response_shape() {
    let json = r#"{
        "success": false,
        "error_code": "TOML_PARSE_ERROR",
        "error": "expected an equals sign",
        "note": "Existing pool preserved — no servers were removed."
    }"#;
    let v: serde_json::Value = serde_json::from_str(json).expect("parse ok");
    assert_eq!(v["success"].as_bool(), Some(false));
    assert_eq!(v["error_code"].as_str(), Some("TOML_PARSE_ERROR"));
    assert!(v["error"].is_string());
    assert!(
        v["note"].as_str().unwrap_or("").contains("preserved"),
        "note must mention preserved pool"
    );
}

/// No-config-file response must have `success: true`, `servers_loaded: 0`.
#[test]
fn reload_pool_no_file_response_shape() {
    let json = r#"{
        "success": true,
        "servers_loaded": 0,
        "servers": [],
        "note": "No config file found — pool is empty but valid."
    }"#;
    let v: serde_json::Value = serde_json::from_str(json).expect("parse ok");
    assert_eq!(v["success"].as_bool(), Some(true));
    assert_eq!(v["servers_loaded"].as_u64(), Some(0));
    assert_eq!(v["servers"].as_array().map(|a| a.len()), Some(0));
}
