// Layer 1 tests for #103: iris_production schema gap.
// Verifies inputSchema documents `namespace` and tool description mentions it.
// No live IRIS required.

use iris_agentic_dev_core::tools::IrisTools;

#[test]
fn test_iris_production_input_schema_documents_namespace() {
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let schema = tools
        .tool_input_schema("iris_production")
        .expect("iris_production must be registered");

    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("inputSchema must have a properties object");

    assert!(
        props.contains_key("namespace"),
        "iris_production inputSchema must include a 'namespace' property; got keys: {:?}",
        props.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_iris_production_description_mentions_namespace() {
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let desc = tools
        .tool_description("iris_production")
        .expect("iris_production must be registered");

    assert!(
        desc.contains("namespace"),
        "iris_production description must mention 'namespace'; got: {desc}"
    );
}

#[test]
fn test_iris_production_params_namespace_defaults_to_none() {
    let json = serde_json::json!({"action": "status"});
    let p: iris_agentic_dev_core::tools::IrisProductionParams =
        serde_json::from_value(json).expect("deserialize");
    assert_eq!(p.action, "status");
    assert!(
        p.namespace.is_none(),
        "namespace must default to None when not provided"
    );
}

#[test]
fn test_iris_production_params_namespace_roundtrip() {
    let json = serde_json::json!({"action": "status", "namespace": "IRISAPP"});
    let p: iris_agentic_dev_core::tools::IrisProductionParams =
        serde_json::from_value(json).expect("deserialize");
    assert_eq!(p.namespace.as_deref(), Some("IRISAPP"));
}

#[test]
fn test_iris_production_params_action_defaults_to_status() {
    let json = serde_json::json!({});
    let p: iris_agentic_dev_core::tools::IrisProductionParams =
        serde_json::from_value(json).expect("deserialize");
    assert_eq!(p.action, "status");
}

#[test]
fn test_iris_production_params_all_fields_parse() {
    let json = serde_json::json!({
        "action": "stop",
        "production_name": "MyApp.Production",
        "namespace": "MYAPP",
        "timeout": 60,
        "force": true,
        "server": "prod"
    });
    let p: iris_agentic_dev_core::tools::IrisProductionParams =
        serde_json::from_value(json).expect("deserialize");
    assert_eq!(p.action, "stop");
    assert_eq!(p.production_name.as_deref(), Some("MyApp.Production"));
    assert_eq!(p.namespace.as_deref(), Some("MYAPP"));
    assert_eq!(p.timeout, 60);
    assert!(p.force);
    assert_eq!(p.server.as_deref(), Some("prod"));
}
