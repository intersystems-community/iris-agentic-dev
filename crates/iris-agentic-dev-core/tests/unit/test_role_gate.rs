// Tests for role-gate logic: subject instances require confirm:true for destructive ops.
// source_control write ops on subject are hard-blocked regardless of confirm.

use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};

// ── compile / execute / admin (FR-019) ───────────────────────────────────────

#[test]
fn test_iris_compile_subject_no_confirm_returns_role_gate() {
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        false,
        "prod",
        false,
    );
    assert!(
        result.is_some(),
        "subject without confirm must return role_gate error"
    );
    let v = result.unwrap();
    assert_eq!(v["role_gate"].as_bool(), Some(true));
    assert_eq!(v["role"].as_str(), Some("subject"));
    assert_eq!(v["instance"].as_str(), Some("prod"));
    assert_eq!(v["required_confirmation"].as_str(), Some("iris_compile"));
    assert!(
        v["hard_block"].as_bool() != Some(true),
        "compile gate must not be a hard block"
    );
}

#[test]
fn test_iris_compile_subject_with_confirm_proceeds() {
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        true,
        "prod",
        false,
    );
    assert!(
        result.is_none(),
        "subject with confirm=true must return None (proceed)"
    );
}

#[test]
fn test_iris_execute_subject_no_confirm_returns_role_gate() {
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_execute",
        false,
        "prod",
        false,
    );
    assert!(
        result.is_some(),
        "subject without confirm must return role_gate error"
    );
    let v = result.unwrap();
    assert_eq!(v["role_gate"].as_bool(), Some(true));
    assert_eq!(v["required_confirmation"].as_str(), Some("iris_execute"));
}

#[test]
fn test_iris_execute_subject_with_confirm_proceeds() {
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_execute",
        true,
        "prod",
        false,
    );
    assert!(result.is_none(), "subject with confirm=true must proceed");
}

#[test]
fn test_iris_compile_workspace_role_no_gate() {
    let result = check_role_gate(
        &ConnectionRole::Workspace,
        "iris_compile",
        false,
        "local",
        false,
    );
    assert!(result.is_none(), "workspace role must never be gated");
}

#[test]
fn test_iris_compile_control_plane_role_no_gate() {
    let result = check_role_gate(
        &ConnectionRole::ControlPlane,
        "iris_compile",
        false,
        "ctrl",
        false,
    );
    assert!(result.is_none(), "control-plane role must never be gated");
}

#[test]
fn test_iris_query_select_subject_no_gate() {
    // SELECT is always permitted on subject
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_query:SELECT",
        false,
        "prod",
        false,
    );
    assert!(result.is_none(), "SELECT on subject must not be gated");
}

#[test]
fn test_iris_query_insert_subject_no_confirm_role_gate() {
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_query:INSERT",
        false,
        "prod",
        false,
    );
    assert!(
        result.is_some(),
        "INSERT on subject without confirm must be gated"
    );
    let v = result.unwrap();
    assert_eq!(v["role_gate"].as_bool(), Some(true));
}

// ── source control hard block (FR-020) ────────────────────────────────────────

#[test]
fn test_iris_source_control_commit_subject_hard_blocked() {
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:commit",
        false,
        "prod",
        true,
    );
    assert!(
        result.is_some(),
        "source_control write on subject must be hard blocked"
    );
    let v = result.unwrap();
    assert_eq!(v["role_gate"].as_bool(), Some(true));
    assert_eq!(
        v["hard_block"].as_bool(),
        Some(true),
        "must be a hard block"
    );
}

#[test]
fn test_iris_source_control_commit_subject_confirm_still_blocked() {
    // confirm=true has no effect on hard_block
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:commit",
        true,
        "prod",
        true,
    );
    assert!(
        result.is_some(),
        "hard block must not be bypassable with confirm=true"
    );
    let v = result.unwrap();
    assert_eq!(v["hard_block"].as_bool(), Some(true));
}

#[test]
fn test_iris_source_control_status_subject_not_blocked() {
    // read ops use tool_name without write marker — callers pass non-hard-block
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:status",
        false,
        "prod",
        false,
    );
    assert!(result.is_none(), "read ops on subject must not be gated");
}

#[test]
fn test_iris_source_control_commit_workspace_not_blocked() {
    let result = check_role_gate(
        &ConnectionRole::Workspace,
        "iris_source_control:commit",
        false,
        "local",
        true,
    );
    assert!(
        result.is_none(),
        "workspace role must not be gated even for write ops"
    );
}
