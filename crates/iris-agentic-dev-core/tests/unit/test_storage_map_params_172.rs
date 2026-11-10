// Layer 1 test for spec 172: IrisStorageMapParams structure validation.
// Tests parameter parsing, `deny_unknown_fields`, and the action enum.

use iris_agentic_dev_core::tools::params::batch8::IrisStorageMapParams;

#[test]
fn test_storage_map_params_all_fields_parse() {
    let json = serde_json::json!({
        "action": "read",
        "class": "Pkg.Class",
        "map": "map_name",
        "global": "^Global",
        "namespaces": ["NS1", "NS2"],
        "version": "abc123",
        "ops": [{"set": ["/path", "value"]}],
        "model": {"key": "value"},
        "confirm_removal": true,
        "seed_counter": false,
        "build_indexes": true,
        "include_values": false,
        "acknowledgePhi": true,
        "max_nodes": 500,
        "max_rows": 100,
        "namespace": "IRISAPP",
        "server": "prod",
        "confirm": true
    });

    let p: IrisStorageMapParams = serde_json::from_value(json).expect("deserialize");

    assert_eq!(p.action, "read");
    assert_eq!(p.class.as_deref(), Some("Pkg.Class"));
    assert_eq!(p.map.as_deref(), Some("map_name"));
    assert_eq!(p.global.as_deref(), Some("^Global"));
    assert_eq!(p.namespaces.as_ref().map(|v| v.len()), Some(2));
    assert_eq!(p.version.as_deref(), Some("abc123"));
    assert_eq!(p.ops.as_ref().map(|v| v.len()), Some(1));
    assert!(p.model.is_some());
    assert!(p.confirm_removal);
    assert!(!p.seed_counter);
    assert!(p.build_indexes);
    assert!(!p.include_values);
    assert!(p.acknowledge_phi);
    assert_eq!(p.max_nodes, Some(500));
    assert_eq!(p.max_rows, Some(100));
    assert_eq!(p.namespace.as_deref(), Some("IRISAPP"));
    assert_eq!(p.server.as_deref(), Some("prod"));
    assert!(p.confirm);
}

#[test]
fn test_storage_map_params_unknown_field_rejected() {
    let json = serde_json::json!({
        "action": "read",
        "unknown_param": "should_fail"
    });

    let result: Result<IrisStorageMapParams, _> = serde_json::from_value(json);

    assert!(
        result.is_err(),
        "unknown_param should be rejected by deny_unknown_fields"
    );

    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("unknown_param"),
        "error should mention unknown_param"
    );
}

#[test]
fn test_storage_map_params_minimal_json_needs_only_action() {
    let json = serde_json::json!({"action": "read"});

    let p: IrisStorageMapParams = serde_json::from_value(json).expect("deserialize minimal");

    // All optional fields should be None or false/default
    assert!(p.class.is_none());
    assert!(p.map.is_none());
    assert!(p.global.is_none());
    assert!(p.namespaces.is_none());
    assert!(p.version.is_none());
    assert!(p.ops.is_none());
    assert!(p.model.is_none());
    assert!(!p.confirm_removal);
    assert!(!p.seed_counter);
    assert!(!p.build_indexes);
    assert!(!p.include_values);
    assert!(!p.acknowledge_phi);
    assert!(p.max_nodes.is_none());
    assert!(p.max_rows.is_none());
    assert!(p.namespace.is_none());
    assert!(p.server.is_none());
    assert!(!p.confirm);
}

#[test]
fn test_storage_map_params_action_is_required() {
    let json = serde_json::json!({});
    assert!(serde_json::from_value::<IrisStorageMapParams>(json).is_err());
}

#[test]
fn test_storage_map_params_acknowledge_phi_rename() {
    // Test that "acknowledgePhi" (camelCase) maps to "acknowledge_phi" (snake_case)
    let json = serde_json::json!({
        "action": "compare",
        "acknowledgePhi": true
    });

    let p: IrisStorageMapParams = serde_json::from_value(json).expect("deserialize with camelCase");

    assert!(p.acknowledge_phi);
}

#[test]
fn test_storage_map_params_action_values() {
    // Test that each action value is valid and parses correctly
    let actions = vec![
        "read", "diagram", "uses", "compare", "preview", "propose", "check", "edit", "replace",
    ];

    for action in actions {
        let json = serde_json::json!({"action": action});
        let p: IrisStorageMapParams =
            serde_json::from_value(json).unwrap_or_else(|_| panic!("action {}", action));
        assert_eq!(p.action, action);
    }
}
