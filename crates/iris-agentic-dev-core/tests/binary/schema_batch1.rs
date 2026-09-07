//! Batch 1 of the `AnyParams` conversion: `compare_document`, `compare_namespace`,
//! `global_preview`, `global_kill`.
//!
//! Ten parameter slots, and the two most dangerous tools on the surface. `global_kill` deletes a
//! global; before this batch it accepted any JSON object at all, so a caller who sent
//! `{"globl": "^X", "confirm_token": "…"}` got `global` defaulted to `""` and the typo went
//! unmentioned.
//!
//! The two assertions come from opposite directions — one reads the listing a spawned server emits,
//! the other scans the handler source. Neither alone is enough: the schema could declare a parameter
//! nothing reads, and the handler could read one nothing declares.

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts, assert_source_contracts, require_iad_binary, ParamContract,
};

/// Nothing here is `required`. Every one of these handlers reads its parameters with
/// `unwrap_or("")`, so omitting one is accepted today and produces a specific downstream error
/// (`SERVER_NOT_FOUND`, `CONFIRM_REQUIRED`). Marking them required would replace those errors with a
/// deserialization failure, which FR-004 forbids and which would make the messages worse.
const BATCH1: &[ParamContract] = &[
    ParamContract {
        tool: "compare_document",
        params: &[
            ("document", "string"),
            ("server_a", "string"),
            ("server_b", "string"),
            ("namespace", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "compare_namespace",
        params: &[
            ("namespace", "string"),
            ("server_a", "string"),
            ("server_b", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "global_preview",
        params: &[
            ("global", "string"),
            ("server", "string"),
            // `count` is read with `as_u64()`, so it must advertise `integer`. A caller sending
            // `"count": "50"` gets the default 20 today, silently.
            ("count", "integer"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "global_kill",
        params: &[
            ("global", "string"),
            ("server", "string"),
            ("confirm_token", "string"),
        ],
        required: &[],
    },
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch1_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH1);
}

#[test]
fn batch1_handlers_read_their_contract() {
    assert_source_contracts(BATCH1);
}
