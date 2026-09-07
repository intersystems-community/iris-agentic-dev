//! Batch 2: namespace and database administration, plus container selection.
//!
//! Nine parameter slots across five tools. tasks.md counted twelve — it assumed all five take
//! `server`, and `iris_containers` does not: it selects a Docker container, not a registered
//! instance. The count in the census is what governs, and the census sees nine.

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts, assert_source_contracts, require_iad_binary, ParamContract,
};

const BATCH2: &[ParamContract] = &[
    ParamContract {
        tool: "iris_namespace_list",
        params: &[("server", "string")],
        required: &[],
    },
    ParamContract {
        tool: "iris_namespace_create",
        params: &[
            ("name", "string"),
            ("db_path", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
    ParamContract {
        tool: "iris_database_list",
        params: &[("server", "string")],
        required: &[],
    },
    ParamContract {
        tool: "iris_database_stats",
        params: &[("db", "string"), ("server", "string")],
        required: &[],
    },
    // `action` has a closed value set (list/select/start) that US2 turns into an advertised enum.
    // Here it is a plain string, because the batches convert and US2 constrains.
    ParamContract {
        tool: "iris_containers",
        params: &[("action", "string"), ("name", "string")],
        required: &[],
    },
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn batch2_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(BATCH2);
}

#[test]
fn batch2_handlers_read_their_contract() {
    assert_source_contracts(BATCH2);
}
