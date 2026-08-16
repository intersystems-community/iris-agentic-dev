//! Regression test: tools with a declared output schema (076-interface-modernization,
//! User Story 1) actually advertise it via `list_tools`, and it stays declared regardless
//! of which toolset (Baseline/Nostub/Merged) is active.
//!
//! This is a static-router check, not a live-IRIS one — it never calls a tool, so it
//! belongs alongside the other pure-logic tests in this file's neighbors
//! (`test_toolset.rs`, `test_tool_category_coverage.rs`), no container required.

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

/// Every tool given an `output_schema` attribute so far — see
/// `crates/iris-agentic-dev-core/src/tools/output_schemas.rs` for the response shapes and the
/// reasoning behind which tools are (and are not yet) covered.
const TOOLS_WITH_DECLARED_OUTPUT_SCHEMA: &[&str] = &[
    "iris_servers",
    "skill_list",
    "skill_community_list",
    "skill_forget",
    "agent_stats",
    "agent_history",
    "kb_recall",
    "iris_symbols",
    "iris_symbols_local",
    "docs_introspect",
    "debug_map_int_to_cls",
    "debug_source_map",
    "iris_ws_open",
    "iris_ws_exec",
    "iris_ws_close",
];

#[test]
fn test_declared_tools_advertise_output_schema_in_baseline() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    for name in TOOLS_WITH_DECLARED_OUTPUT_SCHEMA {
        assert!(
            tools.tool_declares_output_schema(name),
            "'{name}' should declare a non-null output_schema in Baseline's list_tools"
        );
    }
}

#[test]
fn test_declared_tools_advertise_output_schema_in_merged() {
    // debug_map_int_to_cls and debug_source_map are consolidated into iris_debug in Merged
    // (see with_registry_and_toolset's merged_only removal list) — absent there entirely, not
    // "present but missing a schema," so they're excluded from this toolset's check only.
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    for name in TOOLS_WITH_DECLARED_OUTPUT_SCHEMA
        .iter()
        .filter(|n| !["debug_map_int_to_cls", "debug_source_map"].contains(n))
    {
        assert!(
            tools.tool_declares_output_schema(name),
            "'{name}' should declare a non-null output_schema in Merged's list_tools too"
        );
    }
}

#[test]
fn test_a_tool_without_a_declared_schema_reports_false_not_a_panic() {
    // iris_compile hasn't been given an output_schema yet (spec 076 US1 batch 2+) — confirms
    // the accessor distinguishes "no schema" from "not found" without special-casing either.
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    assert!(!tools.tool_declares_output_schema("iris_compile"));
    assert!(!tools.tool_declares_output_schema("not_a_real_tool_name"));
}
