//! T027 — TOML round-trip tests for the `irisAudit` config key.
//!
//! All assertions parse a TOML **string** through the real deserializer. No struct
//! literals. This catches the #110 serde silent-drop pattern: if the field is
//! missing from the struct or the rename is wrong, the value parses to `false` and
//! every test below fails.

use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;

/// `irisAudit = true` on a policy block parses as `true`.
#[test]
fn iris_audit_true_parses() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.prod]
irisAudit = true
"#,
    )
    .expect("parse");
    let policy = cfg.policies.get("prod").expect("policy present");
    assert!(policy.iris_audit, "irisAudit = true must parse as true");
}

/// `irisAudit = false` on a policy block parses as `false`.
#[test]
fn iris_audit_false_parses() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.dev]
irisAudit = false
"#,
    )
    .expect("parse");
    let policy = cfg.policies.get("dev").expect("policy present");
    assert!(!policy.iris_audit, "irisAudit = false must parse as false");
}

/// Absent `irisAudit` defaults to `false` (off by default).
#[test]
fn iris_audit_absent_defaults_false() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.default]
mcpTemplate = "Dev"
"#,
    )
    .expect("parse");
    let policy = cfg.policies.get("default").expect("policy present");
    assert!(!policy.iris_audit, "absent irisAudit must default to false");
}

/// Wrong case (`irisaudit`) does NOT silently enable auditing.
#[test]
fn iris_audit_wrong_case_does_not_enable() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.typo]
irisaudit = true
"#,
    )
    .expect("parse");
    let policy = cfg.policies.get("typo").expect("policy present");
    assert!(
        !policy.iris_audit,
        "misspelled key irisaudit must not enable auditing"
    );
}

/// Underscore form (`iris_audit`) does NOT silently enable auditing.
#[test]
fn iris_audit_underscore_form_does_not_enable() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.underscore]
iris_audit = true
"#,
    )
    .expect("parse");
    let policy = cfg.policies.get("underscore").expect("policy present");
    assert!(
        !policy.iris_audit,
        "iris_audit (underscore form) must not enable auditing"
    );
}

/// `[policy.default]` with `irisAudit = true` is the catchall for non-ServerManager connections.
#[test]
fn iris_audit_default_policy_key_parses() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.default]
irisAudit = true
"#,
    )
    .expect("parse");
    let policy = cfg.policies.get("default").expect("default policy present");
    assert!(
        policy.iris_audit,
        "[policy.default] irisAudit = true must parse as true"
    );
}

/// `irisAudit = true` on one connection does not affect another.
#[test]
fn iris_audit_is_per_connection() {
    let cfg = load_fleet_config_from_str(
        r#"
[policy.enabled]
irisAudit = true

[policy.disabled]
mcpTemplate = "Dev"
"#,
    )
    .expect("parse");
    assert!(
        cfg.policies.get("enabled").expect("enabled").iris_audit,
        "enabled connection must have iris_audit = true"
    );
    assert!(
        !cfg.policies.get("disabled").expect("disabled").iris_audit,
        "disabled connection must have iris_audit = false"
    );
}
