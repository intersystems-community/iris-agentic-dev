//! Batch 7, the last of the thirty-one: HL7 schema browsing, the two Mermaid generators, storage
//! resolution, and stream inspection.
//!
//! `mermaid_class.depth` is the only non-string parameter in the batch, read with `as_u64()` and
//! clamped to 5. It is also the batch's best evidence that this feature does something: the handler
//! echoes the clamped value back, so a client can see 9 become 5 — but until now nothing told the
//! client that 9 was even the wrong shape of question to ask.
//!
//! `stream_inspect` is why the whole feature exists. It shipped documented with a `max_chars`
//! parameter that no code read, so a caller asking for 10,000 characters silently got the entire
//! stream. `stream_inspect_declares_exactly_three_parameters` is the standing guard: the tool takes
//! `oid`, `namespace` and `server`, and if a fourth appears in the schema it must be because a
//! handler reads it.

use iris_agentic_dev_core::testing::{
    advertised_properties, advertised_schemas, assert_advertised_contracts,
    assert_source_contracts, require_iad_binary, ParamContract,
};

const BATCH7: &[ParamContract] = &[
    ParamContract {
        tool: "hl7_schema_list",
        params: &[("namespace", "string"), ("server", "string")],
        required: &[],
    },
    ParamContract {
        tool: "hl7_schema_inspect",
        params: &[
            ("schema", "string"),
            ("segment", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "mermaid_class",
        params: &[
            ("class", "string"),
            ("depth", "integer"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "mermaid_production",
        params: &[
            ("production", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "resolve_storage",
        params: &[
            ("class", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "stream_inspect",
        params: &[
            ("oid", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch7_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH7);
}

#[test]
fn batch7_handlers_read_their_contract() {
    assert_source_contracts(BATCH7);
}

/// The `max_chars` regression guard.
///
/// `stream_inspect` was documented with a `max_chars` parameter that appeared nowhere in `crates/*/src`.
/// With an open schema there was nothing to contradict the prose; with a closed one, adding a
/// parameter to the documentation without adding it to the struct fails
/// `every_documented_tool_parameter_is_in_the_input_schema`, and adding it to the struct without
/// reading it fails this test.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn stream_inspect_declares_exactly_three_parameters() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let schema = schemas
        .get("stream_inspect")
        .expect("`stream_inspect` must appear in tools/list");
    let props = advertised_properties(schema);
    let names: Vec<&str> = props.iter().map(String::as_str).collect();
    assert_eq!(
        names,
        ["namespace", "oid", "server"],
        "`stream_inspect` reads exactly these three keys. A fourth here means either a parameter was \
         added to the schema without a handler reading it — the `max_chars` bug returning — or the \
         handler grew one and this test needs updating along with the docs."
    );
    assert!(
        !props.contains("max_chars"),
        "`max_chars` is back in the schema. It was documented for five minor versions and read by \
         nothing; if truncation is wanted, implement it in `stream_inspect_impl` first."
    );
}
