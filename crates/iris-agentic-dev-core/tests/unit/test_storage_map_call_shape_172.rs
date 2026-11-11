//! Storage map call_shape tests (T034, T035, T036)

use iris_agentic_dev_core::tools::exec::needs_service_account;
use iris_agentic_dev_core::tools::storage_map::{call_shape, Action};
use iris_agentic_dev_core::tools::write_gate::{classify, is_scratch_call, WriteClass};
use serde_json::json;

#[test]
fn test_call_shape_all_actions() {
    // Test each action
    let params = json!({"action": "read"});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Read);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);

    let params = json!({"action": "diagram"});
    assert_eq!(call_shape(&params).action, Action::Diagram);

    let params = json!({"action": "uses"});
    assert_eq!(call_shape(&params).action, Action::Uses);

    let params = json!({"action": "compare"});
    assert_eq!(call_shape(&params).action, Action::Compare);

    let params = json!({"action": "preview"});
    assert_eq!(call_shape(&params).action, Action::Preview);

    let params = json!({"action": "propose"});
    assert_eq!(call_shape(&params).action, Action::Propose);

    let params = json!({"action": "check"});
    assert_eq!(call_shape(&params).action, Action::Check);

    let params = json!({"action": "edit"});
    assert_eq!(call_shape(&params).action, Action::Edit);

    let params = json!({"action": "replace"});
    assert_eq!(call_shape(&params).action, Action::Replace);
}

/// T034: a case or whitespace variant is not the action it resembles. `"Read"` must not be
/// judged as a read, because the handler would not run it as one.
#[test]
fn test_call_shape_case_and_whitespace_variants_are_unreadable() {
    for a in [
        "Read", "READ", " read", "read ", " EDIT ", "Edit", "dIagram", "uses\n", "edit\t",
    ] {
        let shape = call_shape(&json!({"action": a, "acknowledgePhi": true}));
        assert!(shape.unreadable, "{a:?}");
        assert_eq!(shape.action, Action::Edit, "{a:?}");
        assert!(shape.seed_counter && shape.build_indexes, "{a:?}");
        assert!(
            !shape.acknowledge_phi,
            "an unreadable call acknowledges nothing: {a:?}"
        );
    }
    let shape = call_shape(&json!({"action": "read", "acknowledgePhi": true}));
    assert!(!shape.unreadable);
    assert_eq!(shape.action, Action::Read);
    assert!(shape.acknowledge_phi);
    assert!(!call_shape(&json!({"action": "read", "acknowledgePhi": "true"})).acknowledge_phi);
}

#[test]
fn test_call_shape_unreadable() {
    // Missing action becomes Edit with opt-ins true
    let params = json!({});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);

    // Number instead of string becomes Edit with opt-ins
    let params = json!({"action": 123});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);

    // Unknown string becomes Edit with opt-ins
    let params = json!({"action": "unknown"});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);
}

#[test]
fn test_call_shape_opt_ins() {
    // Opt-ins as true
    let params = json!({
        "action": "edit",
        "seed_counter": true,
        "build_indexes": true,
        "include_values": true
    });
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);
    assert!(shape.include_values);

    // Opt-ins as false
    let params = json!({
        "action": "edit",
        "seed_counter": false,
        "build_indexes": false,
        "include_values": false
    });
    let shape = call_shape(&params);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);
    assert!(!shape.include_values);

    // Opt-ins absent default to false
    let params = json!({"action": "edit"});
    let shape = call_shape(&params);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);
    assert!(!shape.include_values);

    // String "true" is NOT true (only boolean true counts)
    let params = json!({
        "action": "edit",
        "seed_counter": "true",
        "build_indexes": "true"
    });
    let shape = call_shape(&params);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);
}

// ── T035: classify integration ─────────────────────────────────────────────────

#[test]
fn test_classify_iris_storage_map_read() {
    let args = json!({"action": "read"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_diagram() {
    let args = json!({"action": "diagram"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_uses() {
    let args = json!({"action": "uses"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_check() {
    let args = json!({"action": "check"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_edit() {
    let args = json!({"action": "edit"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_replace() {
    let args = json!({"action": "replace"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_compare() {
    let args = json!({"action": "compare"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_preview() {
    let args = json!({"action": "preview"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_propose() {
    let args = json!({"action": "propose"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_unreadable() {
    // Unreadable action becomes Edit with opt-ins (seed_counter + build_indexes = true)
    // Edit with build_indexes is Destructive
    let args = json!({"action": "unknown"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Destructive));
}

// ── T035: is_scratch_call integration ──────────────────────────────────────────

#[test]
fn test_is_scratch_call_read() {
    let args = json!({"action": "read"}).as_object().cloned();
    assert!(!is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_diagram() {
    let args = json!({"action": "diagram"}).as_object().cloned();
    assert!(!is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_uses() {
    let args = json!({"action": "uses"}).as_object().cloned();
    assert!(!is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_check() {
    let args = json!({"action": "check"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_edit() {
    let args = json!({"action": "edit"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_replace() {
    let args = json!({"action": "replace"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_compare() {
    let args = json!({"action": "compare"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_preview() {
    let args = json!({"action": "preview"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_propose() {
    let args = json!({"action": "propose"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

// ── T036: needs_service_account integration ────────────────────────────────────

#[test]
fn test_needs_service_account_read() {
    let args = json!({"action": "read"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_diagram() {
    let args = json!({"action": "diagram"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_uses() {
    let args = json!({"action": "uses"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_check() {
    let args = json!({"action": "check"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_edit() {
    let args = json!({"action": "edit"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_replace() {
    let args = json!({"action": "replace"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_compare() {
    let args = json!({"action": "compare"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_preview() {
    let args = json!({"action": "preview"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_propose() {
    let args = json!({"action": "propose"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_with_seed_counter() {
    let args = json!({"action": "edit", "seed_counter": true});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_with_build_indexes() {
    let args = json!({"action": "replace", "build_indexes": true});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn classify_with_no_arguments_is_destructive_and_scratch() {
    use iris_agentic_dev_core::tools::write_gate::{classify, is_scratch_call, WriteClass};
    assert_eq!(
        classify("iris_storage_map", None),
        Some(WriteClass::Destructive)
    );
    assert!(is_scratch_call("iris_storage_map", None));
}
