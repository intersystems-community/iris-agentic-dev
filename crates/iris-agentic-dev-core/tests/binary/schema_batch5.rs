//! Batch 5: the five interoperability tools — the message/log query dispatcher, production item
//! control, production diff, message body reader, and business rule inspection.
//!
//! Thirty-three parameter slots, the largest batch in the feature and the one with the most shapes:
//! two parameters that accept an integer *or* a decimal string, two objects, one array of strings,
//! two counts, and one boolean. `iris_interop_query` alone carries fifteen, which is why its
//! contract was only ever written in prose — a fifteen-parameter tool advertising
//! `{"type": "object"}` is the whole defect in one place.
//!
//! `session_id` and `since_id` are the FR-008 case: their handler reads
//! `as_i64().or_else(|| as_str()?.parse().ok())`, so `12345` and `"12345"` both work today.
//! Declaring `integer` alone would reject half of what currently succeeds, so they advertise
//! `["integer", "string"]` and the live suite calls each one both ways.
//!
//! Nothing in this batch is `required`. `iris_message_body.message_id` is the closest call: the
//! handler returns `INVALID_PARAMS: message_id is required` when it is absent, and that sentence is
//! a better answer than a serde failure naming a Rust struct (FR-004).

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts, assert_source_contracts, require_iad_binary, resolve_property,
    ParamContract,
};

const BATCH5: &[ParamContract] = &[
    ParamContract {
        tool: "iris_interop_query",
        params: &[
            ("what", "string"),
            ("component", "string"),
            ("log_type", "string"),
            ("limit", "integer"),
            ("namespace", "string"),
            ("source", "string"),
            ("target", "string"),
            ("message_class", "string"),
            ("session_id", "integer|string"),
            ("since_id", "integer|string"),
            ("body_class", "string"),
            ("body_where", "string"),
            ("body_select", "array"),
            ("search_table", "object"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_production_item",
        params: &[
            ("action", "string"),
            ("item", "string"),
            ("namespace", "string"),
            ("settings", "object"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_production_diff",
        params: &[
            ("production", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_message_body",
        params: &[
            ("message_id", "string"),
            ("namespace", "string"),
            ("max_bytes", "integer"),
            ("acknowledgePhi", "boolean"),
            ("dataPolicy", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_business_rule_info",
        params: &[
            ("action", "string"),
            ("rule_name", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch5_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH5);
}

#[test]
fn batch5_handlers_read_their_contract() {
    assert_source_contracts(BATCH5);
}

/// The FR-008 regression guard, stated on its own rather than only inside the contract table.
/// `StringOrI64`'s two-type declaration is a `#[schemars(extend(...))]` override; if it is ever
/// dropped the schema narrows to one branch and clients start rejecting `session_id: "12345"`
/// before the call leaves the client. The type array must also be exactly these two, in this order,
/// because that is what the unit test on `StringOrI64` pins.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn session_and_since_id_advertise_both_json_types() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = iris_agentic_dev_core::testing::advertised_schemas();
    let schema = schemas
        .get("iris_interop_query")
        .expect("iris_interop_query must appear in tools/list");

    for name in ["session_id", "since_id"] {
        let prop = resolve_property(schema, name);
        assert_eq!(
            prop.get("type"),
            Some(&serde_json::json!(["integer", "string"])),
            "`{name}` accepts a number or a decimal string today; the schema must say both or a \
             validating client will reject one of them: {prop}"
        );
    }
}

/// `search_table` is an object with a shape, not an opaque blob: the handler deserializes it into
/// `SearchTableFilter`, where `prop` is mandatory and the other four are optional. Advertising a
/// bare `object` would leave a caller guessing the key names from the description, which is the
/// prose dependency this feature removes — so the nested properties are asserted too.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn search_table_advertises_its_nested_shape() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = iris_agentic_dev_core::testing::advertised_schemas();
    let schema = schemas
        .get("iris_interop_query")
        .expect("iris_interop_query must appear in tools/list");
    let filter = resolve_property(schema, "search_table");

    let props = filter
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("`search_table` must declare its own properties: {filter}"));
    let names: Vec<&String> = props.keys().collect();
    assert_eq!(
        names,
        vec!["class", "extent", "prop", "value", "value_like"],
        "`search_table` must advertise the keys `SearchTableFilter` deserializes: {filter}"
    );
    assert_eq!(
        filter.get("required"),
        Some(&serde_json::json!(["prop"])),
        "`prop` has no default in `SearchTableFilter`, so a search_table without it fails to \
         deserialize; the schema must say so: {filter}"
    );
}
