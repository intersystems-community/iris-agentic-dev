//! Spec 172 batch 8 schema test: `iris_storage_map` declares exactly its contract.
//!
//! One tool, nine actions, twenty parameters. The `action` field is an enum of nine string
//! values; every other parameter appears in the schema exactly as declared in the params struct.

use iris_agentic_dev_core::testing::{
    advertised_properties, advertised_schemas, assert_advertised_contracts,
    assert_source_contracts, require_iad_binary, ParamContract,
};
use serde_json::Value;

const BATCH8: &[ParamContract] = &[ParamContract {
    tool: "iris_storage_map",
    params: &[
        ("action", "string"),
        ("class", "string"),
        ("map", "string"),
        ("global", "string"),
        ("namespaces", "array"),
        ("version", "string"),
        ("ops", "array"),
        ("model", "object"),
        ("confirm_removal", "boolean"),
        ("seed_counter", "boolean"),
        ("build_indexes", "boolean"),
        ("include_values", "boolean"),
        ("acknowledgePhi", "boolean"),
        ("max_nodes", "integer"),
        ("max_rows", "integer"),
        ("namespace", "string"),
        ("server", "string"),
        ("confirm", "boolean"),
        ("index_globals", "array"),
        ("field_names", "array"),
    ],
    required: &["action"],
}];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch8_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH8);
}

#[test]
fn batch8_handlers_read_their_contract() {
    assert_source_contracts(BATCH8);
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_storage_map_action_enum_has_nine_values() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let schema = schemas
        .get("iris_storage_map")
        .expect("`iris_storage_map` must appear in tools/list");

    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("schema must have properties");

    let action_schema = props.get("action").expect("action parameter must exist");

    let enum_values = action_schema
        .get("enum")
        .and_then(|e| e.as_array())
        .expect("action must have an enum constraint");

    let actions: Vec<String> = enum_values
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    assert_eq!(
        actions.len(),
        9,
        "action enum must have exactly 9 values; got {:?}",
        actions
    );

    let expected = vec![
        "read", "diagram", "uses", "compare", "preview", "propose", "check", "edit", "replace",
    ];
    assert_eq!(
        actions, expected,
        "action enum must match the contract exactly"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_storage_map_additional_properties_false() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let schema = schemas
        .get("iris_storage_map")
        .expect("`iris_storage_map` must appear in tools/list");

    let additional = schema
        .get("additionalProperties")
        .expect("schema must declare additionalProperties");

    assert_eq!(
        additional,
        &Value::Bool(false),
        "iris_storage_map must have additionalProperties: false"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_storage_map_declares_exactly_twenty_parameters() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let schema = schemas
        .get("iris_storage_map")
        .expect("`iris_storage_map` must appear in tools/list");

    let props = advertised_properties(schema);
    let names: Vec<&str> = props.iter().map(String::as_str).collect();

    assert_eq!(
        names.len(),
        20,
        "`iris_storage_map` must declare exactly 20 parameters; got {:?}",
        names
    );

    let expected = vec![
        "acknowledgePhi",
        "action",
        "build_indexes",
        "class",
        "confirm",
        "confirm_removal",
        "field_names",
        "global",
        "include_values",
        "index_globals",
        "map",
        "max_nodes",
        "max_rows",
        "model",
        "namespace",
        "namespaces",
        "ops",
        "seed_counter",
        "server",
        "version",
    ];

    assert_eq!(
        names, expected,
        "`iris_storage_map` parameters must match the contract exactly"
    );
}

/// T080 (FR-032): `iris_doc`'s description, as `tools/list` advertises it, says default storage
/// is off-limits and sends SQL storage maps to `iris_storage_map`.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_doc_description_separates_default_storage_from_sql_maps() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = iris_agentic_dev_core::testing::advertised_tools();
    let desc = tools
        .get("iris_doc")
        .and_then(|t| t["description"].as_str())
        .expect("`iris_doc` must appear in tools/list with a description")
        .to_string();
    assert!(
        desc.contains("default storage") && desc.contains("off-limits"),
        "iris_doc must say default storage is off-limits: {desc}"
    );
    assert!(
        desc.contains("SQL storage map") && desc.contains("iris_storage_map"),
        "iris_doc must send SQL storage maps to iris_storage_map: {desc}"
    );
}
