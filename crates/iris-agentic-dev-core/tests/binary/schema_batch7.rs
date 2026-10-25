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
//! stream. Spec 141 T199 made the cap real. `stream_inspect_declares_exactly_four_parameters` is the
//! standing guard: the tool takes `oid`, `max_chars`, `namespace` and `server`, and if a fifth
//! appears in the schema it must be because a handler reads it.

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
            ("max_chars", "integer"),
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

/// The `max_chars` guard, schema half.
///
/// `stream_inspect` was documented with a `max_chars` parameter that appeared nowhere in
/// `crates/*/src`. The schema now declares it as an integer because `stream_inspect_impl` reads it
/// (spec 141 T199); a fifth key, or `max_chars` as anything but an integer, means the schema and
/// the reader have come apart (its type, `integer`, is pinned by `batch7_advertises_its_contract`).
/// The docs and source halves are
/// `max_chars_is_documented_declared_and_read` in `tests/unit/test_docs_contract.rs`.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn stream_inspect_declares_exactly_four_parameters() {
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
        ["max_chars", "namespace", "oid", "server"],
        "`stream_inspect` reads exactly these four keys. A fifth here means a parameter was added to \
         the schema without a handler reading it — the `max_chars` bug returning."
    );
}
