//! Batch 3: `iris_admin`, the widest tool on the surface — 26 parameters across 25 actions.
//!
//! One flat struct, per research decision 4: JSON Schema can express "these six fields matter when
//! action is create_user" with `if`/`then`, but schemars will not derive that from a flat struct and
//! hand-writing it for 26 fields across 25 actions produces a schema no one will maintain. The
//! per-action requirement tables in `docs/tools.md` stay the normative statement, and
//! `tests/unit/test_docs_contract.rs` keeps checking them against the handler.
//!
//! `action` is the one required parameter in this feature. Without it there is nothing to dispatch
//! to, so requiring it turns a generic INVALID_ACTION into a schema a client can see before it calls.

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts, assert_source_contracts, require_iad_binary, ParamContract,
};

/// The types come from what the handler does with each value, not from what reads naturally:
/// `enabled` and `confirm` are `as_bool()`, `max_records`/`primary_port`/`async_member_type` are
/// `as_u64()`, and `time_range` is an object the handler indexes with `.get("from")`/`.get("to")`.
/// Declaring `max_records` as a string would be the `max_chars` bug with a different name.
const BATCH3: &[ParamContract] = &[ParamContract {
    tool: "iris_admin",
    params: &[
        ("action", "string"),
        ("server", "string"),
        ("type", "string"),
        ("username", "string"),
        ("path", "string"),
        ("resource", "string"),
        ("permission", "string"),
        ("password", "string"),
        ("full_name", "string"),
        ("roles", "string"),
        ("enabled", "boolean"),
        ("name", "string"),
        ("code_database", "string"),
        ("data_database", "string"),
        ("namespace", "string"),
        ("dispatch_class", "string"),
        ("global_pattern", "string"),
        ("time_range", "object"),
        ("max_records", "integer"),
        ("new_password", "string"),
        ("mirror_name", "string"),
        ("primary_host", "string"),
        ("primary_port", "integer"),
        ("instance_name", "string"),
        ("async_member_type", "integer"),
        ("confirm", "boolean"),
    ],
    required: &["action"],
}];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch3_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH3);
}

#[test]
fn batch3_handlers_read_their_contract() {
    assert_source_contracts(BATCH3);
}

/// `time_range` is the only nested object in the whole conversion, and a `$ref` to a definition is
/// not a contract a client can read without resolving it. It must arrive inlined, with the two
/// fields the handler indexes.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_admin_time_range_is_inlined_with_from_and_to() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = iris_agentic_dev_core::testing::advertised_schemas();
    let schema = schemas
        .get("iris_admin")
        .expect("iris_admin must appear in tools/list");
    let prop = iris_agentic_dev_core::testing::resolve_property(schema, "time_range");
    assert!(
        prop.get("$ref").is_none(),
        "time_range must be inlined, not referenced: {prop}"
    );
    let inner = prop
        .get("properties")
        .unwrap_or_else(|| panic!("time_range must declare its own properties: {prop}"));
    for field in ["from", "to"] {
        assert!(
            inner.get(field).is_some(),
            "time_range must declare `{field}`, which `journal_search_impl` reads: {prop}"
        );
    }
}
