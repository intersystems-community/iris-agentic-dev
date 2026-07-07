// Unit tests for scm.rs internal logic (no IRIS needed).
// Tests ScmAction enum, SCM_MENU constant, and ScmParams deserialization.

use iris_agentic_dev_core::tools::scm::{ScmAction, ScmParams, SCM_MENU};

#[test]
fn test_scm_menu_prefix_is_percent_source_menu() {
    assert_eq!(SCM_MENU, "%SourceMenu");
}

#[test]
fn test_scm_action_known_variants() {
    assert_eq!(ScmAction::from_id("%CheckOut"), ScmAction::CheckOut);
    assert_eq!(ScmAction::from_id("%UndoCheckout"), ScmAction::UndoCheckout);
    assert_eq!(ScmAction::from_id("%CheckIn"), ScmAction::CheckIn);
    assert_eq!(ScmAction::from_id("%GetLatest"), ScmAction::GetLatest);
    assert_eq!(
        ScmAction::from_id("%AddToSourceControl"),
        ScmAction::AddToSourceControl
    );
    assert_eq!(ScmAction::from_id("Diff"), ScmAction::Diff);
    assert_eq!(ScmAction::from_id("%Disconnect"), ScmAction::Disconnect);
    assert_eq!(ScmAction::from_id("%Reconnect"), ScmAction::Reconnect);
}

#[test]
fn test_scm_action_checkin_blocked() {
    // Both with and without % prefix must resolve to CheckIn
    assert_eq!(ScmAction::from_id("CheckIn"), ScmAction::CheckIn);
    assert_eq!(ScmAction::from_id("%CheckIn"), ScmAction::CheckIn);
}

#[test]
fn test_scm_action_unknown() {
    assert_eq!(
        ScmAction::from_id("SomeWeirdAction"),
        ScmAction::Unknown("SomeWeirdAction".to_string())
    );
}

#[test]
fn test_scm_params_action_required() {
    let p: ScmParams = serde_json::from_str(r#"{"action": "status"}"#).unwrap();
    assert_eq!(p.action, "status");
    assert_eq!(p.namespace, "USER"); // default_namespace
    assert!(p.document.is_none());
    assert!(p.action_id.is_none());
    assert!(p.answer.is_none());
    assert!(p.elicitation_id.is_none());
}

#[test]
fn test_scm_params_full() {
    let p: ScmParams = serde_json::from_str(
        r#"{
            "action": "execute",
            "document": "MyClass.cls",
            "action_id": "CheckOut",
            "namespace": "MYNAMESPACE"
        }"#,
    )
    .unwrap();
    assert_eq!(p.action, "execute");
    assert_eq!(p.document.as_deref(), Some("MyClass.cls"));
    assert_eq!(p.action_id.as_deref(), Some("CheckOut"));
    assert_eq!(p.namespace, "MYNAMESPACE");
}

#[test]
fn test_scm_params_with_elicitation_fields() {
    let p: ScmParams = serde_json::from_str(
        r#"{
            "action": "execute",
            "answer": "yes",
            "elicitation_id": "eid-abc123"
        }"#,
    )
    .unwrap();
    assert_eq!(p.action, "execute");
    assert_eq!(p.answer.as_deref(), Some("yes"));
    assert_eq!(p.elicitation_id.as_deref(), Some("eid-abc123"));
    assert_eq!(p.namespace, "USER");
}

#[test]
fn test_scm_params_missing_action_fails() {
    let result: Result<ScmParams, _> = serde_json::from_str(r#"{"document": "Foo.cls"}"#);
    assert!(
        result.is_err(),
        "action is required — should fail without it"
    );
}
