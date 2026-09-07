//! Batch 6: the four write-gated interoperability tools — credential list and management, lookup
//! table entries, and lookup table import/export.
//!
//! Fifteen parameter slots, every one a plain string. What makes this batch different is the gate:
//! three of the four tools are classified `Write` or `Destructive` in `write_gate.rs`, so on a
//! default-policy instance the gate answers before the handler ever reads a parameter. The live
//! tests therefore run two sessions — one with the gates closed, to prove the refusal still arrives
//! now that params are typed, and one with them open, to prove the parameters still reach IRIS.
//!
//! None of the four reads `server`. That is not an oversight in the conversion — it is what the
//! handlers do, and `server_is_absent_from_every_tool_in_this_batch` pins it so nobody adds the
//! property from the pattern of the surrounding tools without also wiring it. Recorded as F6 in
//! `specs/113-typed-tool-schemas/parameter-audit.md`.

use iris_agentic_dev_core::testing::{
    advertised_schemas, assert_advertised_contracts, assert_source_contracts, require_iad_binary,
    ParamContract,
};

const BATCH6: &[ParamContract] = &[
    ParamContract {
        tool: "iris_credential_list",
        params: &[("namespace", "string")],
        required: &[],
    },
    ParamContract {
        tool: "iris_credential_manage",
        params: &[
            ("action", "string"),
            ("id", "string"),
            ("username", "string"),
            ("password", "string"),
            ("namespace", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_lookup_manage",
        params: &[
            ("action", "string"),
            ("table", "string"),
            ("key", "string"),
            ("value", "string"),
            ("namespace", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_lookup_transfer",
        params: &[
            ("action", "string"),
            ("table", "string"),
            ("xml", "string"),
            ("namespace", "string"),
        ],
        required: &[],
    },
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch6_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH6);
}

#[test]
fn batch6_handlers_read_their_contract() {
    assert_source_contracts(BATCH6);
}

/// Every other tool that reaches IRIS takes `server`; these four do not, because their handlers call
/// `self.iris_arc()` directly instead of going through the pool. Declaring the property anyway would
/// advertise instance selection that does not happen — a caller passing `server` would be answered
/// from the default connection and never told.
///
/// The absence is asserted rather than left implicit so the suite-wide `server` consistency check in
/// US3 has something to disagree with if the handlers change.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn server_is_absent_from_every_tool_in_this_batch() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();

    for contract in BATCH6 {
        let schema = schemas
            .get(contract.tool)
            .unwrap_or_else(|| panic!("`{}` must appear in tools/list", contract.tool));
        let has_server = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|p| p.contains_key("server"));
        assert!(
            !has_server,
            "`{}` does not read `server` — it uses the default connection. Declaring the property \
             would promise instance selection the handler does not perform; wire the handler to the \
             pool first, then delete this assertion: {schema}",
            contract.tool
        );
    }
}
