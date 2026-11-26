//! Spec 173 T015: the `iris_storage_map` schema a client sees over the wire names the new
//! `propose` inputs with their types, and stays closed.

use iris_agentic_dev_core::testing::{advertised_schemas, require_iad_binary, resolve_property};

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_advertised_schema_has_index_globals_and_field_names() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let schema = &schemas["iris_storage_map"];
    for name in ["index_globals", "field_names"] {
        let prop = resolve_property(schema, name);
        assert_eq!(prop["type"], "array", "{name}: {prop}");
        assert_eq!(prop["items"]["type"], "string", "{name}: {prop}");
    }
    assert_eq!(
        schema["additionalProperties"],
        serde_json::json!(false),
        "the schema must stay closed: {schema}"
    );
}
