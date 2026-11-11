//! Storage map call_shape tests (T034)

use iris_agentic_dev_core::tools::storage_map::{call_shape, Action};
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

#[test]
fn test_call_shape_case_insensitive() {
    // Action is case-insensitive
    let params = json!({"action": "Read"});
    assert_eq!(call_shape(&params).action, Action::Read);

    let params = json!({"action": "EDIT"});
    assert_eq!(call_shape(&params).action, Action::Edit);
}

#[test]
fn test_call_shape_whitespace() {
    // Whitespace is trimmed, then lowercased
    let params = json!({"action": " EDIT "});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    // But since the raw action wasn't in the list before trim, opt-ins may differ
    // Actually, re-reading the spec: trim happens before the match, so this should work
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
