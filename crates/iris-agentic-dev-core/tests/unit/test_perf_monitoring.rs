//! Unit tests for iris_mirror_status and iris_database_list free space (089).
//! No IRIS connection required.

use iris_agentic_dev_core::tools::admin_tools::parse_max_size_mb;

// ── parse_max_size_mb ─────────────────────────────────────────────────────────

#[test]
fn max_size_unlimited_returns_none() {
    assert_eq!(parse_max_size_mb("Unlimited"), None);
}

#[test]
fn max_size_unlimited_case_insensitive() {
    assert_eq!(parse_max_size_mb("unlimited"), None);
    assert_eq!(parse_max_size_mb("UNLIMITED"), None);
}

#[test]
fn max_size_mb_suffix() {
    assert_eq!(parse_max_size_mb("500MB"), Some(500));
    assert_eq!(parse_max_size_mb("1024MB"), Some(1024));
    assert_eq!(parse_max_size_mb("128MB"), Some(128));
}

#[test]
fn max_size_gb_converts_to_mb() {
    assert_eq!(parse_max_size_mb("2GB"), Some(2048));
    assert_eq!(parse_max_size_mb("1GB"), Some(1024));
}

#[test]
fn max_size_empty_returns_none() {
    assert_eq!(parse_max_size_mb(""), None);
}

#[test]
fn max_size_unrecognized_returns_none() {
    assert_eq!(parse_max_size_mb("System Default"), None);
    assert_eq!(parse_max_size_mb("???"), None);
}

// ── normalize_mirror_type ─────────────────────────────────────────────────────

use iris_agentic_dev_core::tools::admin_tools::normalize_mirror_type;

#[test]
fn not_member_string_normalizes_to_none() {
    assert_eq!(normalize_mirror_type("Not Member"), None);
}

#[test]
fn empty_string_normalizes_to_none() {
    assert_eq!(normalize_mirror_type(""), None);
}

#[test]
fn primary_passes_through() {
    assert_eq!(
        normalize_mirror_type("primary"),
        Some("primary".to_string())
    );
}

#[test]
fn backup_passes_through() {
    assert_eq!(normalize_mirror_type("backup"), Some("backup".to_string()));
}

#[test]
fn async_member_passes_through() {
    assert_eq!(normalize_mirror_type("async"), Some("async".to_string()));
}

// ── mirror_status JSON shape (non-member) ─────────────────────────────────────

use iris_agentic_dev_core::tools::admin_tools::build_mirror_status_json;

#[test]
fn non_member_shape_has_false_and_nulls() {
    let v = build_mirror_status_json(false, "", "Not Member", false);
    assert_eq!(v["is_member"], serde_json::Value::Bool(false));
    assert_eq!(v["is_primary"], serde_json::Value::Bool(false));
    assert_eq!(v["mirror_name"], serde_json::Value::Null);
    assert_eq!(v["member_type"], serde_json::Value::Null);
}

#[test]
fn member_shape_has_name_and_type() {
    let v = build_mirror_status_json(true, "MIRROR1", "primary", true);
    assert_eq!(v["is_member"], serde_json::Value::Bool(true));
    assert_eq!(v["is_primary"], serde_json::Value::Bool(true));
    assert_eq!(v["mirror_name"], serde_json::json!("MIRROR1"));
    assert_eq!(v["member_type"], serde_json::json!("primary"));
}

#[test]
fn backup_member_is_not_primary() {
    let v = build_mirror_status_json(true, "MIRROR1", "backup", false);
    assert_eq!(v["is_member"], serde_json::Value::Bool(true));
    assert_eq!(v["is_primary"], serde_json::Value::Bool(false));
    assert_eq!(v["member_type"], serde_json::json!("backup"));
}
