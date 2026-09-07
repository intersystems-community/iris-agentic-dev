//! Batch 4: the six read-only administration tools — SystemPerformance control, mirror status,
//! journal search, audit log, and the two access-inspection tools.
//!
//! Nineteen parameter slots. Two of them are the reason this batch exists: `journal_search`'s
//! `max_entries` and `query_audit_log`'s `limit` are read with `as_u64()`, so a caller who sends
//! `"50"` gets the default 100 today and no indication why. Declaring them `integer` turns that
//! silent discard into a refusal.
//!
//! `mode` is documented as required for `iris_system_performance`, and it is not in `required` here.
//! The handler reads it with `unwrap_or("")` and answers a missing mode with
//! `unknown mode ''; valid values: start, status, last_runid`. Putting it in `required` would
//! replace that sentence with a serde deserialization failure, which is a worse answer to the same
//! mistake (FR-004). `action` on `iris_admin` is the one exception in this feature, and it is an
//! exception because a missing action has no handler to produce a better message.

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts, assert_source_contracts, require_iad_binary, ParamContract,
};

const BATCH4: &[ParamContract] = &[
    ParamContract {
        tool: "iris_system_performance",
        params: &[
            ("server", "string"),
            ("mode", "string"),
            ("run_id", "string"),
            ("profile", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_mirror_status",
        params: &[("server", "string")],
        required: &[],
    },
    ParamContract {
        tool: "journal_search",
        params: &[
            ("start", "string"),
            ("end", "string"),
            ("global_pattern", "string"),
            ("max_entries", "integer"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "query_audit_log",
        params: &[
            ("user", "string"),
            ("event_type", "string"),
            ("start", "string"),
            ("end", "string"),
            ("limit", "integer"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "my_access",
        params: &[("server", "string")],
        required: &[],
    },
    ParamContract {
        tool: "capability_matrix",
        params: &[("user", "string"), ("server", "string")],
        required: &[],
    },
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch4_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH4);
}

#[test]
fn batch4_handlers_read_their_contract() {
    assert_source_contracts(BATCH4);
}

/// `profile` looks like a closed set — the docs list six names and so does the error message — but
/// `sysperf_profile_or_default` accepts any name made of letters, digits and underscore, because an
/// instance can carry site-defined profiles. Declaring it as an enum would reject calls that work
/// today, so it stays a plain string, and US2 must not add one.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn system_performance_profile_is_not_declared_as_an_enum() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = iris_agentic_dev_core::testing::advertised_schemas();
    let schema = schemas
        .get("iris_system_performance")
        .expect("iris_system_performance must appear in tools/list");
    let prop = iris_agentic_dev_core::testing::resolve_property(schema, "profile");
    assert!(
        prop.get("enum").is_none(),
        "`profile` accepts any alphanumeric_underscore name (site profiles), so an advertised enum \
         would reject working calls: {prop}"
    );
}
